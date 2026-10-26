//! Foods tab: your library of ingredients (kcal and macros per 100 g, a default serving) and
//! meals built from them. Anything here can be logged by amount, from this page, from the
//! Habits calorie card, by typing "1 serving of kvarg" in the entry bar, or through Claude.

use super::{calorie_color, claude_color, theme, Action};
use crate::foods::{Amount, Food, Ingredient, Library, Meal, Portionable};
use crate::store::Data;
use eframe::egui::{self, RichText, Ui};
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Ingredients,
    Meals,
    Favorites,
}

pub struct FoodsUi {
    search: String,
    filter: Filter,
    selected: Option<String>,
    confirm_delete: bool,
    /// Amount in the log box, and whether it's in grams (else servings).
    amount: f32,
    in_grams: bool,
    /// Aliases being edited: (food or meal id, comma-separated text).
    aliases: Option<(String, String)>,
    /// Ingredient search in the meal editor.
    add_search: String,
    /// Claude estimating an ingredient: (food id, reply).
    estimating: Option<(String, Receiver<Result<String, String>>)>,
    /// Claude reading a label photo.
    reading: Option<Receiver<Result<crate::label::LabelFood, String>>>,
    /// What Claude was unsure about on the last label: (food id, note).
    label_note: Option<(String, String)>,
}

impl Default for FoodsUi {
    fn default() -> Self {
        FoodsUi {
            search: String::new(),
            filter: Filter::All,
            selected: None,
            confirm_delete: false,
            amount: 1.0,
            in_grams: false,
            aliases: None,
            add_search: String::new(),
            estimating: None,
            reading: None,
            label_note: None,
        }
    }
}

fn macros_text(protein: Option<f32>, carbs: Option<f32>, fat: Option<f32>) -> Option<String> {
    let parts: Vec<String> = [("P", protein), ("C", carbs), ("F", fat)].iter().filter_map(|(l, v)| v.map(|g| format!("{l} {g:.0} g"))).collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// A grams field that can be empty ("–" = unknown).
fn opt_grams(ui: &mut Ui, v: &mut Option<f32>, max: f32) -> bool {
    let mut x = v.unwrap_or(-1.0);
    let r = ui.add(
        egui::DragValue::new(&mut x)
            .range(-1.0..=max)
            .speed(0.2)
            .custom_formatter(|n, _| if n < 0.0 { "–".into() } else { format!("{n:.1} g") })
            .custom_parser(|s| {
                let s = s.trim().trim_end_matches('g').trim();
                if s.is_empty() || s == "-" || s == "–" {
                    Some(-1.0)
                } else {
                    s.replace(',', ".").parse().ok()
                }
            }),
    );
    if r.changed() {
        *v = (x >= 0.0).then_some(x);
        true
    } else {
        false
    }
}

/// Short line like "155 kcal / jar (250 g)".
fn per_serving(p: &Portionable) -> String {
    format!("{} kcal / {} ({:.0} g)", p.kcal(Amount::Servings(1.0)), p.serving_name, p.serving_g)
}

impl FoodsUi {
    /// Finishes a running Claude estimate (called every frame on every tab).
    pub fn poll(&mut self, data: &mut Data) {
        if let Some(rx) = &self.reading {
            if let Ok(res) = rx.try_recv() {
                self.reading = None;
                match res {
                    Ok(label) => {
                        let id = label.food.id.clone();
                        data.status = format!("Read from the label: {}. Check the numbers against the photo.", label.food.name);
                        data.foods.foods.push(label.food);
                        data.save_foods();
                        self.select(Some(id.clone()), data);
                        if !label.note.is_empty() {
                            self.label_note = Some((id, label.note));
                        }
                    }
                    Err(e) => data.status = e,
                }
            }
        }
        let Some((id, rx)) = &self.estimating else { return };
        let Ok(res) = rx.try_recv() else { return };
        let id = id.clone();
        self.estimating = None;
        match res.and_then(|text| parse_estimate(&text)) {
            Ok(est) => {
                if let Some(f) = data.foods.foods.iter_mut().find(|f| f.id == id) {
                    f.kcal_per_100g = est.kcal_per_100g;
                    f.protein_per_100g = est.protein_per_100g;
                    f.carbs_per_100g = est.carbs_per_100g;
                    f.fat_per_100g = est.fat_per_100g;
                    if let Some(g) = est.serving_g {
                        f.serving_g = g;
                    }
                    if let Some(n) = est.serving_name {
                        f.serving_name = n;
                    }
                    let name = f.name.clone();
                    data.save_foods();
                    data.status = format!("Claude filled in {name}. Check the numbers against the package if you have it.");
                }
            }
            Err(e) => data.status = format!("Estimate failed: {e}"),
        }
    }

    pub fn page(&mut self, ctx: &egui::Context, data: &mut Data) {
        if self.estimating.is_some() || self.reading.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        // a label photo dropped onto the window
        let dropped: Option<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).find(|p| is_photo(p)));
        if let Some(path) = dropped {
            self.read_label(data, &path);
        }
        egui::SidePanel::left("foods_side").resizable(false).exact_width(320.0).show(ctx, |ui| self.sidebar(ui, data));
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(820.0);
                ui.add_space(10.0);
                if self.reading.is_some() {
                    theme::card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Claude is reading the label…").color(claude_color()));
                        });
                    });
                    ui.add_space(8.0);
                }
                let sel = self.selected.clone();
                if let Some(i) = sel.as_ref().and_then(|id| data.foods.foods.iter().position(|f| f.id == *id)) {
                    self.food_editor(ui, data, i);
                } else if let Some(i) = sel.as_ref().and_then(|id| data.foods.meals.iter().position(|m| m.id == *id)) {
                    self.meal_editor(ui, data, i);
                } else {
                    self.overview(ui, data);
                }
                ui.add_space(30.0);
            });
        });
    }

    fn select(&mut self, id: Option<String>, data: &Data) {
        self.selected = id;
        self.confirm_delete = false;
        self.aliases = None;
        self.add_search.clear();
        self.in_grams = false;
        self.amount = 1.0;
        let _ = data;
    }

    fn sidebar(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        ui.add_space(10.0);
        ui.heading("Foods");
        let (nf, nm) = (data.foods.foods.len(), data.foods.meals.len());
        ui.label(RichText::new(format!("{nf} ingredient{} · {nm} meal{}", if nf == 1 { "" } else { "s" }, if nm == 1 { "" } else { "s" })).weak().size(12.5));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("+ Ingredient").clicked() {
                let food = Food { id: uuid::Uuid::new_v4().to_string(), name: "New ingredient".into(), serving_g: 100.0, serving_name: "serving".into(), ..Default::default() };
                let id = food.id.clone();
                data.foods.foods.push(food);
                data.save_foods();
                self.select(Some(id), data);
            }
            if ui
                .add_enabled(self.reading.is_none(), egui::Button::new(RichText::new("📷 From label").color(claude_color())))
                .on_hover_text("Pick a photo of a nutrition label (Swedish or English) and Claude makes an ingredient from it. You can also drop a photo onto this page.")
                .clicked()
            {
                let file = rfd::FileDialog::new().set_title("Photo of a nutrition label").add_filter("Photos", crate::label::EXTENSIONS).pick_file();
                if let Some(path) = file {
                    self.read_label(data, &path);
                }
            }
            if ui.button("+ Meal").clicked() {
                let meal = Meal { id: uuid::Uuid::new_v4().to_string(), name: "New meal".into(), servings: 1.0, ..Default::default() };
                let id = meal.id.clone();
                data.foods.meals.push(meal);
                data.save_foods();
                self.select(Some(id), data);
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search foods…").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            for (f, label) in [(Filter::All, "All"), (Filter::Ingredients, "Ingredients"), (Filter::Meals, "Meals"), (Filter::Favorites, "★")] {
                ui.selectable_value(&mut self.filter, f, label);
            }
        });
        ui.separator();
        let favorite = |id: &str| data.foods.food(id).map(|f| f.favorite).or_else(|| data.foods.meals.iter().find(|m| m.id == id).map(|m| m.favorite)).unwrap_or(false);
        let mut pick = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            // recently logged, when not searching
            if self.search.trim().is_empty() && self.filter == Filter::All {
                let recent = recent_foods(data, 6);
                if !recent.is_empty() {
                    ui.label(RichText::new("RECENTLY LOGGED").size(11.5).color(t.text.muted));
                    for p in &recent {
                        if row(ui, p, self.selected.as_deref() == Some(p.id.as_str()), favorite(&p.id)).clicked() {
                            pick = Some(p.id.clone());
                        }
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("ALL").size(11.5).color(t.text.muted));
                }
            }
            let list: Vec<Portionable> = data
                .foods
                .search(&self.search)
                .into_iter()
                .filter(|p| match self.filter {
                    Filter::All => true,
                    Filter::Ingredients => !p.is_meal,
                    Filter::Meals => p.is_meal,
                    Filter::Favorites => favorite(&p.id),
                })
                .collect();
            if list.is_empty() {
                let msg = if data.foods.foods.is_empty() && data.foods.meals.is_empty() { "Your library is empty. Add an ingredient to start." } else { "Nothing matches." };
                ui.label(RichText::new(msg).weak());
            }
            for p in &list {
                if row(ui, p, self.selected.as_deref() == Some(p.id.as_str()), favorite(&p.id)).clicked() {
                    pick = Some(p.id.clone());
                }
            }
            });
        });
        if let Some(id) = pick {
            self.select(Some(id), data);
        }
    }

    fn overview(&mut self, ui: &mut Ui, data: &Data) {
        let t = theme::th();
        ui.label(RichText::new("Your food library").font(theme::bold(22.0)));
        ui.add_space(4.0);
        ui.label(RichText::new(
            "Save the foods you eat often with their calories per 100 g and a usual serving, and build meals from them. \
             Then log by amount anywhere, and the calories are exact every time:",
        ).color(t.text.muted));
        ui.add_space(6.0);
        for ex in [
            "\"1 serving of kvarg\", \"2 jars of kvarg\", \"200 g chili\", \"had half a portion of lasagna\": in the entry bar (Local mode)",
            "\"add 1 serving of kvarg to my calorie count\": in Claude mode; Claude uses your library's numbers",
            "the \"From your foods\" picker on the Habits calorie card",
            "the Log box on each food here",
            "\"📷 From label\" (or drop a photo here, or scan one on your phone): Claude reads a nutrition label and makes the ingredient",
        ] {
            ui.label(format!("•  {ex}"));
        }
        ui.add_space(10.0);
        if data.foods.foods.is_empty() && data.foods.meals.is_empty() {
            super::streaks::empty_state(ui, "No foods yet", "Press \"+ Ingredient\" on the left. Not sure of the numbers? Type the name and press \"Estimate with Claude\".");
        } else {
            ui.label(RichText::new("Pick a food on the left to edit or log it.").weak());
        }
    }

    // ------------------------------------------------------------------ ingredient

    fn food_editor(&mut self, ui: &mut Ui, data: &mut Data, i: usize) {
        let t = theme::th();
        let mut changed = false;
        let mut delete = false;
        let used_in = {
            let id = &data.foods.foods[i].id;
            data.foods.meals.iter().filter(|m| m.ingredients.iter().any(|g| g.food == *id)).count()
        };
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let f = &mut data.foods.foods[i];
            ui.horizontal(|ui| {
                ui.label(RichText::new("INGREDIENT").size(11.5).color(t.text.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_delete {
                        if ui.small_button("No").clicked() {
                            self.confirm_delete = false;
                        }
                        if ui.small_button("Yes, delete").clicked() {
                            delete = true;
                        }
                        let note = if used_in > 0 { format!("Delete? It's used in {used_in} meal(s); it will be removed from them.") } else { "Delete this ingredient?".into() };
                        ui.label(RichText::new(note).color(t.status.warning).size(12.5));
                    } else if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                        self.confirm_delete = true;
                    }
                    let star = if f.favorite { RichText::new("★").color(t.data.gold) } else { RichText::new("☆") };
                    if ui.small_button(star).on_hover_text("Favorite: shown first on the Habits calorie card").clicked() {
                        f.favorite = !f.favorite;
                        changed = true;
                    }
                });
            });
            changed |= ui.add(egui::TextEdit::singleline(&mut f.name).font(egui::FontId::proportional(20.0)).desired_width(f32::INFINITY)).changed();
            changed |= aliases_field(ui, &mut self.aliases, &f.id, &mut f.aliases);
            ui.add_space(6.0);
            egui::Grid::new("food_grid").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                ui.label("Calories");
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::DragValue::new(&mut f.kcal_per_100g).range(0.0..=900.0).speed(1.0).suffix(" kcal")).changed();
                    ui.label(RichText::new("per 100 g (or 100 ml)").weak());
                });
                ui.end_row();
                ui.label("Serving");
                ui.horizontal(|ui| {
                    ui.label("1");
                    changed |= ui.add(egui::TextEdit::singleline(&mut f.serving_name).desired_width(90.0).hint_text("serving")).changed();
                    ui.label("=");
                    changed |= ui.add(egui::DragValue::new(&mut f.serving_g).range(1.0..=5000.0).speed(1.0).suffix(" g")).changed();
                    ui.label(RichText::new("e.g. jar, slice, cup").weak().size(12.0));
                });
                ui.end_row();
                ui.label("Per 100 g");
                ui.horizontal(|ui| {
                    ui.label("protein");
                    changed |= opt_grams(ui, &mut f.protein_per_100g, 100.0);
                    ui.label("carbs");
                    changed |= opt_grams(ui, &mut f.carbs_per_100g, 100.0);
                    ui.label("fat");
                    changed |= opt_grams(ui, &mut f.fat_per_100g, 100.0);
                    ui.label(RichText::new("optional").weak().size(12.0));
                });
                ui.end_row();
                ui.label("Notes");
                changed |= ui.add(egui::TextEdit::multiline(&mut f.notes).desired_rows(2).desired_width(420.0).hint_text("brand, where to buy, …")).changed();
                ui.end_row();
            });
            ui.add_space(4.0);
            if let Some((_, note)) = self.label_note.as_ref().filter(|(id, _)| *id == f.id) {
                ui.label(RichText::new(format!("Claude: {note}")).italics().color(claude_color()));
            }
            let busy = self.estimating.as_ref().is_some_and(|(id, _)| *id == f.id);
            let photo = crate::label::photo_path(&data.paths.root, &f.id);
            ui.horizontal(|ui| {
                if photo.exists() && ui.button("📷 Label photo").on_hover_text("Open the photo these numbers were read from").clicked() {
                    let _ = open::that(&photo);
                }
                if busy {
                    ui.spinner();
                    ui.label("Claude is estimating…");
                } else if ui
                    .button(RichText::new("✨ Estimate with Claude").color(claude_color()))
                    .on_hover_text("Fills in calories, macros and a typical serving from the name (uses a little of your Claude plan). Package numbers are more exact.")
                    .clicked()
                {
                    self.estimate(data_paths(&data.paths), &data.settings, f.id.clone(), f.name.clone());
                }
            });
        });
        if delete {
            let id = data.foods.foods.remove(i).id;
            for m in &mut data.foods.meals {
                m.ingredients.retain(|g| g.food != id);
            }
            data.save_foods();
            self.select(None, data);
            return;
        }
        if changed {
            data.save_foods();
        }
        ui.add_space(10.0);
        let p = data.foods.foods[i].portionable();
        self.log_box(ui, data, &p);
    }

    /// Starts Claude reading a label photo from disk.
    fn read_label(&mut self, data: &mut Data, path: &std::path::Path) {
        if self.reading.is_some() {
            return;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                data.status = format!("Couldn't open the photo: {e}");
                return;
            }
        };
        let (settings, root) = (data.settings.clone(), data.paths.root.clone());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::label::read_photo(&settings, &root, &bytes));
        });
        self.reading = Some(rx);
        self.select(None, data);
        data.status = "Claude is reading the label…".into();
    }

    fn estimate(&mut self, root: std::path::PathBuf, settings: &crate::store::Settings, id: String, name: String) {
        let Some(cli) = crate::claude::find_cli(settings) else {
            self.estimating = None;
            return;
        };
        let prompt = format!(
            "Give typical nutrition values for this food: \"{name}\". Use common values for the usual product (if it's a \
             Swedish product name, think of what's sold in Swedish shops). When unsure, pick the higher end. Reply with \
             ONLY this JSON, no other text: {{\"kcal_per_100g\": number, \"protein_per_100g\": number, \"carbs_per_100g\": \
             number, \"fat_per_100g\": number, \"serving_g\": number, \"serving_name\": \"one word like serving, jar, slice, cup, piece\"}}"
        );
        let (tx, rx) = mpsc::channel();
        let model = settings.claude_model.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::claude::ask(&cli, &model, &prompt, &root));
        });
        self.estimating = Some((id, rx));
    }

    // ------------------------------------------------------------------ meal

    fn meal_editor(&mut self, ui: &mut Ui, data: &mut Data, i: usize) {
        let t = theme::th();
        let mut changed = false;
        let mut delete = false;
        let mut remove: Option<usize> = None;
        let mut add: Option<Ingredient> = None;
        let mut create: Option<String> = None;
        let lib_snapshot: Library = data.foods.clone();
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let m = &mut data.foods.meals[i];
            ui.horizontal(|ui| {
                ui.label(RichText::new("MEAL").size(11.5).color(t.text.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_delete {
                        if ui.small_button("No").clicked() {
                            self.confirm_delete = false;
                        }
                        if ui.small_button("Yes, delete").clicked() {
                            delete = true;
                        }
                        ui.label(RichText::new("Delete this meal? (its ingredients stay)").color(t.status.warning).size(12.5));
                    } else if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                        self.confirm_delete = true;
                    }
                    let star = if m.favorite { RichText::new("★").color(t.data.gold) } else { RichText::new("☆") };
                    if ui.small_button(star).on_hover_text("Favorite: shown first on the Habits calorie card").clicked() {
                        m.favorite = !m.favorite;
                        changed = true;
                    }
                });
            });
            changed |= ui.add(egui::TextEdit::singleline(&mut m.name).font(egui::FontId::proportional(20.0)).desired_width(f32::INFINITY)).changed();
            changed |= aliases_field(ui, &mut self.aliases, &m.id, &mut m.aliases);
            ui.add_space(6.0);
            ui.label(RichText::new("Ingredients (raw weights)").strong());
            egui::Grid::new("meal_ings").num_columns(4).spacing([14.0, 6.0]).show(ui, |ui| {
                for (k, ing) in m.ingredients.iter_mut().enumerate() {
                    match lib_snapshot.food(&ing.food) {
                        Some(f) => {
                            ui.label(&f.name);
                            changed |= ui.add(egui::DragValue::new(&mut ing.grams).range(0.0..=10000.0).speed(1.0).suffix(" g")).changed();
                            let kcal = f.kcal_per_100g * ing.grams / 100.0;
                            ui.label(RichText::new(format!("{kcal:.0} kcal")).color(calorie_color()));
                            if f.kcal_per_100g <= 0.0 {
                                ui.label(RichText::new("no calories set").color(t.status.warning).size(12.0));
                            }
                        }
                        None => {
                            ui.label(RichText::new("(deleted ingredient)").color(t.status.warning));
                            ui.label("");
                            ui.label("");
                        }
                    }
                    if ui.small_button("✖").on_hover_text("Remove").clicked() {
                        remove = Some(k);
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Add");
                ui.add(egui::TextEdit::singleline(&mut self.add_search).hint_text("search your ingredients…").desired_width(220.0));
            });
            if !self.add_search.trim().is_empty() {
                let matches: Vec<&Food> = lib_snapshot.foods.iter().filter(|f| crate::foods::norm(&f.name).contains(&crate::foods::norm(&self.add_search))).take(8).collect();
                ui.horizontal_wrapped(|ui| {
                    for f in &matches {
                        if ui.button(format!("+ {} ({:.0} g)", f.name, f.serving_g)).clicked() {
                            add = Some(Ingredient { food: f.id.clone(), grams: f.serving_g });
                        }
                    }
                    if !matches.iter().any(|f| crate::foods::norm(&f.name) == crate::foods::norm(&self.add_search)) {
                        if ui.button(RichText::new(format!("+ New ingredient \"{}\"", self.add_search.trim())).color(t.widget.link)).on_hover_text("Creates it in the library; set its calories afterwards").clicked() {
                            create = Some(self.add_search.trim().to_string());
                        }
                    }
                });
            }
            ui.add_space(6.0);
            egui::Grid::new("meal_grid").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                ui.label("Makes");
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::DragValue::new(&mut m.servings).range(0.5..=50.0).speed(0.1).suffix(" servings")).changed();
                });
                ui.end_row();
                ui.label("Cooked weight");
                ui.horizontal(|ui| {
                    changed |= opt_grams(ui, &mut m.cooked_g, 20000.0);
                    ui.label(RichText::new("optional: weigh the finished dish to log exact grams of it").weak().size(12.0));
                });
                ui.end_row();
                ui.label("Notes");
                changed |= ui.add(egui::TextEdit::multiline(&mut m.notes).desired_rows(3).desired_width(460.0).hint_text("steps, tips, …")).changed();
                ui.end_row();
            });
            let (total, raw) = lib_snapshot.meal_totals(m);
            let p = lib_snapshot.meal_portionable(m);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("Whole recipe: {:.0} kcal · {:.0} g raw", total.kcal, raw)).strong());
                ui.label(RichText::new(format!("Per serving: {} kcal ({:.0} g)", p.kcal(Amount::Servings(1.0)), p.serving_g)).strong().color(calorie_color()));
                ui.label(RichText::new(format!("{:.0} kcal / 100 g", p.per100.kcal)).weak());
                if let Some(mx) = macros_text(p.nutrition(Amount::Servings(1.0)).protein, p.nutrition(Amount::Servings(1.0)).carbs, p.nutrition(Amount::Servings(1.0)).fat) {
                    ui.label(RichText::new(format!("per serving {mx}")).weak());
                }
            });
        });
        if let Some(name) = create {
            let food = Food { id: uuid::Uuid::new_v4().to_string(), name, serving_g: 100.0, serving_name: "serving".into(), ..Default::default() };
            add = Some(Ingredient { food: food.id.clone(), grams: 100.0 });
            data.foods.foods.push(food);
        }
        if let Some(a) = add {
            data.foods.meals[i].ingredients.push(a);
            self.add_search.clear();
            changed = true;
        }
        if let Some(k) = remove {
            data.foods.meals[i].ingredients.remove(k);
            changed = true;
        }
        if delete {
            data.foods.meals.remove(i);
            data.save_foods();
            self.select(None, data);
            return;
        }
        if changed {
            data.save_foods();
        }
        ui.add_space(10.0);
        let p = data.foods.meal_portionable(&data.foods.meals[i]);
        self.log_box(ui, data, &p);
    }

    // ------------------------------------------------------------------ log box

    fn log_box(&mut self, ui: &mut Ui, data: &mut Data, p: &Portionable) {
        let t = theme::th();
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Log it").strong());
            ui.horizontal(|ui| {
                if self.in_grams {
                    ui.add(egui::DragValue::new(&mut self.amount).range(1.0..=5000.0).speed(1.0).suffix(" g"));
                } else {
                    ui.add(egui::DragValue::new(&mut self.amount).range(0.25..=20.0).speed(0.05).max_decimals(2));
                }
                let unit = if self.in_grams { "grams".to_string() } else { crate::foods::plural(&p.serving_name) };
                egui::ComboBox::from_id_salt("log_unit").selected_text(unit).width(110.0).show_ui(ui, |ui| {
                    if ui.selectable_label(!self.in_grams, crate::foods::plural(&p.serving_name)).clicked() && self.in_grams {
                        self.in_grams = false;
                        self.amount = (self.amount / p.serving_g * 4.0).round().max(1.0) / 4.0;
                    }
                    if ui.selectable_label(self.in_grams, "grams").clicked() && !self.in_grams {
                        self.in_grams = true;
                        self.amount = (self.amount * p.serving_g).round();
                    }
                });
                let amount = if self.in_grams { Amount::Grams(self.amount) } else { Amount::Servings(self.amount) };
                let n = p.nutrition(amount);
                ui.label(RichText::new(format!("= {} kcal", p.kcal(amount))).strong().color(calorie_color()));
                if let Some(mx) = macros_text(n.protein, n.carbs, n.fat) {
                    ui.label(RichText::new(mx).weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(p.per100.kcal > 0.0, egui::Button::new(RichText::new("Log to today").strong())).clicked() {
                        data.log_food(p, amount);
                    }
                });
            });
            ui.label(RichText::new(format!("Or type \"{} {}\" in the entry bar.", if self.in_grams { format!("{:.0} g", self.amount) } else { format!("{} {}", self.amount, p.serving_name) }, p.name.to_lowercase())).color(t.text.muted).size(12.0));
        });
    }

    // ------------------------------------------------------------------ Habits picker

    /// "From your foods" on the Habits calorie card: favorites or search matches, one click
    /// logs a serving.
    pub fn picker(ui: &mut Ui, data: &Data, query: &mut String, actions: &mut Vec<Action>) {
        if data.foods.foods.is_empty() && data.foods.meals.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Foods:").weak());
            ui.add(egui::TextEdit::singleline(query).hint_text("search your foods…").desired_width(ui.available_width()));
        });
        let list: Vec<Portionable> = if query.trim().is_empty() {
            let mut fav: Vec<Portionable> = data
                .foods
                .all()
                .into_iter()
                .filter(|p| data.foods.food(&p.id).map(|f| f.favorite).or_else(|| data.foods.meals.iter().find(|m| m.id == p.id).map(|m| m.favorite)).unwrap_or(false))
                .collect();
            for r in recent_foods(data, 6) {
                if !fav.iter().any(|f| f.id == r.id) {
                    fav.push(r);
                }
            }
            fav.truncate(6);
            fav
        } else {
            data.foods.search(query).into_iter().take(6).collect()
        };
        ui.horizontal_wrapped(|ui| {
            for p in list {
                let k = p.kcal(Amount::Servings(1.0));
                if ui.button(format!("{} +{k}", p.name)).on_hover_text(format!("Log 1 {} ({:.0} g) = {k} kcal", p.serving_name, p.serving_g)).clicked() {
                    actions.push(Action::LogFood(p.id.clone(), Amount::Servings(1.0)));
                    query.clear();
                }
            }
        });
    }
}

fn is_photo(p: &std::path::Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| crate::label::EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

fn data_paths(paths: &crate::store::Paths) -> std::path::PathBuf {
    paths.root.clone()
}

/// One row in the food list.
fn row(ui: &mut Ui, p: &Portionable, selected: bool, favorite: bool) -> egui::Response {
    let t = theme::th();
    let text = format!("{}{}{}", if p.is_meal { "🍴 " } else { "" }, p.name, if favorite { "  ★" } else { "" });
    let r = ui.add(egui::SelectableLabel::new(selected, RichText::new(text)));
    let rect = r.rect;
    ui.painter().text(
        rect.right_center() - egui::vec2(6.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        format!("{} kcal", p.kcal(Amount::Servings(1.0))),
        egui::FontId::proportional(12.0),
        t.text.muted,
    );
    r.on_hover_text(per_serving(p))
}

/// The aliases text field; returns true when they changed.
fn aliases_field(ui: &mut Ui, state: &mut Option<(String, String)>, id: &str, aliases: &mut Vec<String>) -> bool {
    if state.as_ref().is_none_or(|(sid, _)| sid != id) {
        *state = Some((id.to_string(), aliases.join(", ")));
    }
    let (_, text) = state.as_mut().unwrap();
    let r = ui.add(egui::TextEdit::singleline(text).hint_text("other names, comma separated (e.g. quark, kvarg 0.2%)").desired_width(f32::INFINITY));
    if r.changed() {
        *aliases = text.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        true
    } else {
        false
    }
}

/// Library foods logged most recently, newest first.
pub fn recent_foods(data: &Data, n: usize) -> Vec<Portionable> {
    let mut out: Vec<Portionable> = Vec::new();
    let mut entries: Vec<&crate::model::CalorieEntry> = data.calories.iter().filter(|c| c.food.is_some()).collect();
    entries.sort_by_key(|c| std::cmp::Reverse((c.date, c.time)));
    for c in entries {
        if let Some(p) = c.food.as_deref().and_then(|name| data.foods.find(name)) {
            if !out.iter().any(|o| o.id == p.id) {
                out.push(p);
            }
        }
        if out.len() >= n {
            break;
        }
    }
    out
}

struct Estimate {
    kcal_per_100g: f32,
    protein_per_100g: Option<f32>,
    carbs_per_100g: Option<f32>,
    fat_per_100g: Option<f32>,
    serving_g: Option<f32>,
    serving_name: Option<String>,
}

fn parse_estimate(text: &str) -> Result<Estimate, String> {
    let start = text.find('{').ok_or("Claude didn't reply with numbers")?;
    let end = text.rfind('}').ok_or("Claude didn't reply with numbers")?;
    let v: serde_json::Value = serde_json::from_str(&text[start..=end]).map_err(|e| e.to_string())?;
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32).filter(|x| x.is_finite() && *x >= 0.0);
    Ok(Estimate {
        kcal_per_100g: num("kcal_per_100g").filter(|k| *k <= 900.0).ok_or("no kcal_per_100g in the reply")?,
        protein_per_100g: num("protein_per_100g"),
        carbs_per_100g: num("carbs_per_100g"),
        fat_per_100g: num("fat_per_100g"),
        serving_g: num("serving_g").filter(|g| *g >= 1.0),
        serving_name: v.get("serving_name").and_then(|s| s.as_str()).map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty() && s.len() < 20),
    })
}
