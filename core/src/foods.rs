//! Food library: ingredients (kcal and optional macros per 100 g, plus a default serving)
//! and meals built from them. Stored in foods.json. Logging "1 serving of kvarg" or
//! "200 g chili" looks the food up here and computes the calories, so they're exact and
//! consistent instead of estimated each time.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Food {
    pub id: String,
    pub name: String,
    /// Other names it may be called by ("quark", "kvarg 0.2%").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub kcal_per_100g: f32,
    /// Grams in one serving.
    pub serving_g: f32,
    /// What one serving is called: "serving", "jar", "slice", "cup"...
    #[serde(default = "serving")]
    pub serving_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protein_per_100g: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carbs_per_100g: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fat_per_100g: Option<f32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favorite: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

fn serving() -> String {
    "serving".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ingredient {
    /// Id of a food in the library.
    pub food: String,
    pub grams: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Meal {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub ingredients: Vec<Ingredient>,
    /// How many servings the recipe makes.
    pub servings: f32,
    /// Weight of the finished dish, if you weighed it (cooking changes the weight). With it,
    /// "300 g chili" is exact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooked_g: Option<f32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favorite: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub foods: Vec<Food>,
    #[serde(default)]
    pub meals: Vec<Meal>,
}

/// kcal and macros (grams) for some amount of food. Macros are None when not known.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Nutrition {
    pub kcal: f32,
    pub protein: Option<f32>,
    pub carbs: Option<f32>,
    pub fat: Option<f32>,
}

impl Nutrition {
    fn scaled(self, f: f32) -> Nutrition {
        Nutrition { kcal: self.kcal * f, protein: self.protein.map(|v| v * f), carbs: self.carbs.map(|v| v * f), fat: self.fat.map(|v| v * f) }
    }
    fn add(self, o: Nutrition) -> Nutrition {
        let sum = |a: Option<f32>, b: Option<f32>| Some(a? + b?);
        Nutrition { kcal: self.kcal + o.kcal, protein: sum(self.protein, o.protein), carbs: sum(self.carbs, o.carbs), fat: sum(self.fat, o.fat) }
    }
}

/// Something you can log an amount of: an ingredient or a meal, reduced to per-100 g values
/// and a serving size.
#[derive(Clone, Debug, PartialEq)]
pub struct Portionable {
    pub id: String,
    pub name: String,
    pub is_meal: bool,
    pub per100: Nutrition,
    pub serving_g: f32,
    pub serving_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Amount {
    Servings(f32),
    Grams(f32),
}

impl Portionable {
    pub fn grams(&self, a: Amount) -> f32 {
        match a {
            Amount::Servings(n) => n * self.serving_g,
            Amount::Grams(g) => g,
        }
    }

    pub fn nutrition(&self, a: Amount) -> Nutrition {
        self.per100.scaled(self.grams(a) / 100.0)
    }

    /// Whole kcal, rounded up (better to overcount).
    pub fn kcal(&self, a: Amount) -> u32 {
        (self.nutrition(a).kcal - 1e-3).ceil().max(0.0) as u32
    }

    /// "1 serving (250 g)", "2 jars (500 g)", "200 g".
    pub fn describe(&self, a: Amount) -> String {
        match a {
            Amount::Servings(n) => {
                let unit = if n <= 1.0 + 1e-3 { self.serving_name.clone() } else { plural(&self.serving_name) };
                format!("{} {unit} ({:.0} g)", fmt_num(n), self.grams(a))
            }
            Amount::Grams(g) => format!("{g:.0} g"),
        }
    }
}

fn fmt_num(n: f32) -> String {
    if (n - n.round()).abs() < 1e-3 {
        format!("{:.0}", n)
    } else if (n * 2.0 - (n * 2.0).round()).abs() < 1e-3 && n < 1.0 {
        "½".into()
    } else {
        format!("{n:.1}")
    }
}

pub fn plural(w: &str) -> String {
    if w.ends_with('s') {
        w.to_string()
    } else if w.ends_with("ch") || w.ends_with("sh") || w.ends_with('x') {
        format!("{w}es")
    } else {
        format!("{w}s")
    }
}

impl Food {
    pub fn per100(&self) -> Nutrition {
        Nutrition { kcal: self.kcal_per_100g, protein: self.protein_per_100g, carbs: self.carbs_per_100g, fat: self.fat_per_100g }
    }

    pub fn portionable(&self) -> Portionable {
        Portionable {
            id: self.id.clone(),
            name: self.name.clone(),
            is_meal: false,
            per100: self.per100(),
            serving_g: self.serving_g.max(1.0),
            serving_name: if self.serving_name.trim().is_empty() { serving() } else { self.serving_name.clone() },
        }
    }
}

impl Library {
    pub fn food(&self, id: &str) -> Option<&Food> {
        self.foods.iter().find(|f| f.id == id)
    }

    /// The whole recipe: total nutrition and total raw weight (unknown foods count as 0).
    pub fn meal_totals(&self, m: &Meal) -> (Nutrition, f32) {
        let mut n = Nutrition { kcal: 0.0, protein: Some(0.0), carbs: Some(0.0), fat: Some(0.0) };
        let mut grams = 0.0;
        for ing in &m.ingredients {
            if let Some(f) = self.food(&ing.food) {
                n = n.add(f.per100().scaled(ing.grams / 100.0));
            }
            grams += ing.grams;
        }
        (n, grams)
    }

    pub fn meal_portionable(&self, m: &Meal) -> Portionable {
        let (total, raw_g) = self.meal_totals(m);
        let weight = m.cooked_g.filter(|g| *g > 0.0).unwrap_or(raw_g).max(1.0);
        let servings = m.servings.max(0.1);
        Portionable {
            id: m.id.clone(),
            name: m.name.clone(),
            is_meal: true,
            per100: total.scaled(100.0 / weight),
            serving_g: weight / servings,
            serving_name: serving(),
        }
    }

    /// Every food and meal, as things you can log.
    pub fn all(&self) -> Vec<Portionable> {
        self.foods.iter().map(Food::portionable).chain(self.meals.iter().map(|m| self.meal_portionable(m))).collect()
    }

    pub fn by_id(&self, id: &str) -> Option<Portionable> {
        self.food(id).map(Food::portionable).or_else(|| self.meals.iter().find(|m| m.id == id).map(|m| self.meal_portionable(m)))
    }

    fn names_of(&self, id: &str) -> Vec<String> {
        let (name, aliases) = match (self.food(id), self.meals.iter().find(|m| m.id == id)) {
            (Some(f), _) => (&f.name, &f.aliases),
            (_, Some(m)) => (&m.name, &m.aliases),
            _ => return Vec::new(),
        };
        std::iter::once(name).chain(aliases).map(|s| norm(s)).filter(|s| !s.is_empty()).collect()
    }

    /// Finds a food or meal by name: exact name or alias first, then singular/plural, then
    /// the longest name contained in the query as whole words.
    pub fn find(&self, query: &str) -> Option<Portionable> {
        let q = norm(query);
        if q.is_empty() {
            return None;
        }
        let all = self.all();
        let singular = |s: &str| s.strip_suffix("es").or_else(|| s.strip_suffix('s')).unwrap_or(s).to_string();
        for p in &all {
            let names = self.names_of(&p.id);
            if names.iter().any(|n| *n == q || singular(n) == singular(&q)) {
                return Some(p.clone());
            }
        }
        let padded = format!(" {q} ");
        all.into_iter()
            .filter_map(|p| {
                let best = self.names_of(&p.id).into_iter().filter(|n| padded.contains(&format!(" {n} "))).map(|n| n.len()).max()?;
                Some((best, p))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, p)| p)
    }

    /// Search for the list and pickers: name or alias containing every word.
    pub fn search(&self, query: &str) -> Vec<Portionable> {
        let q = norm(query);
        let mut v: Vec<Portionable> = self
            .all()
            .into_iter()
            .filter(|p| q.is_empty() || self.names_of(&p.id).iter().any(|n| q.split_whitespace().all(|w| n.contains(w))))
            .collect();
        v.sort_by_key(|p| p.name.to_lowercase());
        v
    }
}

/// Lowercase, single spaces, without punctuation that doesn't matter for names.
pub fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '%' || c == '.' || c == '-' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn word_number(w: &str) -> Option<f32> {
    Some(match w {
        "a" | "an" | "one" => 1.0,
        "half" | "½" => 0.5,
        "two" => 2.0,
        "three" => 3.0,
        "four" => 4.0,
        "five" => 5.0,
        "quarter" => 0.25,
        _ => return w.replace(',', ".").parse::<f32>().ok().filter(|n| n.is_finite() && *n > 0.0),
    })
}

/// "1 serving of kvarg", "200 g chili", "had 2 jars of kvarg", "add half a portion of
/// lasagna to my calories", "ate kvarg", "kvarg x2". Returns the food and the amount when the
/// text names something in the library. Without a verb (ate, had, log, add…) an amount is
/// required, so a task like "buy kvarg" is never taken for food.
pub fn parse_log(text: &str, lib: &Library) -> Option<(Portionable, Amount)> {
    if lib.foods.is_empty() && lib.meals.is_empty() {
        return None;
    }
    let mut s = norm(text);
    let mut verb = false;
    for lead in ["i just ", "i ", "just "] {
        if let Some(r) = s.strip_prefix(lead) {
            s = r.to_string();
        }
    }
    for v in ["ate ", "had ", "eaten ", "drank ", "log ", "add ", "eat ", "have "] {
        if let Some(r) = s.strip_prefix(v) {
            s = r.to_string();
            verb = true;
            break;
        }
    }
    for tail in [" to my calorie count", " to my calories", " to calories", " to my log", " to the log", " today", " for breakfast", " for lunch", " for dinner", " as a snack", " for a snack"] {
        if let Some(r) = s.strip_suffix(tail) {
            s = r.to_string();
        }
    }
    let toks: Vec<&str> = s.split_whitespace().collect();
    if toks.is_empty() {
        return None;
    }
    // amount first: "<n> [unit] [of] food"
    let mut i = 0;
    let mut n = None;
    if let Some(v) = word_number(toks[0]) {
        n = Some(v);
        i = 1;
        // "half a serving", "half an apple"
        if toks.get(1).is_some_and(|w| *w == "a" || *w == "an") && toks[0] == "half" {
            i = 2;
        }
    } else if let Some(g) = toks[0].strip_suffix('g').and_then(|g| g.replace(',', ".").parse::<f32>().ok()) {
        // "200g kvarg"
        let rest = toks[1..].join(" ");
        let rest = rest.strip_prefix("of ").unwrap_or(&rest);
        return lib.find(rest).map(|p| (p, Amount::Grams(g)));
    }
    if let Some(n) = n {
        let unit = toks.get(i).copied().unwrap_or("");
        let after = |k: usize| {
            let rest = toks[k.min(toks.len())..].join(" ");
            rest.strip_prefix("of ").map(str::to_string).unwrap_or(rest)
        };
        if matches!(unit, "g" | "gram" | "grams" | "gr" | "ml") {
            return lib.find(&after(i + 1)).map(|p| (p, Amount::Grams(n)));
        }
        if matches!(unit, "serving" | "servings" | "portion" | "portions" | "x") {
            return lib.find(&after(i + 1)).map(|p| (p, Amount::Servings(n)));
        }
        // "2 slices of bread": the food's own serving name, or any unit followed by "of"
        if toks.get(i + 1) == Some(&"of") {
            return lib.find(&after(i + 2)).map(|p| (p, Amount::Servings(n)));
        }
        if let Some(p) = lib.find(&after(i + 1)) {
            let named = norm(&p.serving_name);
            if unit == named || unit == plural(&named) {
                return Some((p, Amount::Servings(n)));
            }
        }
        // "2 kvarg", "an apple"
        return lib.find(&after(i)).map(|p| (p, Amount::Servings(n)));
    }
    // amount last: "kvarg 200 g", "kvarg 200g", "kvarg x2"
    let last = *toks.last().unwrap();
    if toks.len() >= 2 {
        let head = toks[..toks.len() - 1].join(" ");
        if let Some(g) = last.strip_suffix('g').and_then(|g| g.parse::<f32>().ok()) {
            return lib.find(&head).map(|p| (p, Amount::Grams(g)));
        }
        if let Some(x) = last.strip_prefix('x').and_then(|x| x.parse::<f32>().ok()) {
            return lib.find(&head).map(|p| (p, Amount::Servings(x)));
        }
        if toks.len() >= 3 && matches!(last, "g" | "grams") {
            if let Ok(g) = toks[toks.len() - 2].parse::<f32>() {
                return lib.find(&toks[..toks.len() - 2].join(" ")).map(|p| (p, Amount::Grams(g)));
            }
        }
    }
    // just a name, after a verb: one serving
    if verb {
        return lib.find(&s).map(|p| (p, Amount::Servings(1.0)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> Library {
        let food = |id: &str, name: &str, kcal: f32, serving: f32, sname: &str| Food {
            id: id.into(),
            name: name.into(),
            kcal_per_100g: kcal,
            serving_g: serving,
            serving_name: sname.into(),
            protein_per_100g: Some(10.0),
            ..Default::default()
        };
        let mut kvarg = food("k", "Kvarg", 62.0, 250.0, "jar");
        kvarg.aliases = vec!["quark".into()];
        Library {
            version: 1,
            foods: vec![kvarg, food("r", "Rice", 130.0, 150.0, "serving"), food("m", "Minced beef", 250.0, 100.0, "serving"), food("b", "Bread", 260.0, 40.0, "slice")],
            meals: vec![Meal {
                id: "c".into(),
                name: "Chili".into(),
                ingredients: vec![Ingredient { food: "m".into(), grams: 500.0 }, Ingredient { food: "r".into(), grams: 300.0 }],
                servings: 4.0,
                cooked_g: Some(1000.0),
                ..Default::default()
            }],
        }
    }

    #[test]
    fn logging_phrases() {
        let l = lib();
        let p = |t: &str| parse_log(t, &l).map(|(f, a)| (f.name, a));
        assert_eq!(p("add 1 serving of kvarg to my calorie count"), Some(("Kvarg".into(), Amount::Servings(1.0))));
        assert_eq!(p("200g kvarg"), Some(("Kvarg".into(), Amount::Grams(200.0))));
        assert_eq!(p("had 2 jars of quark"), Some(("Kvarg".into(), Amount::Servings(2.0))));
        assert_eq!(p("2 jars kvarg"), Some(("Kvarg".into(), Amount::Servings(2.0))));
        assert_eq!(p("I ate half a portion of chili for dinner"), Some(("Chili".into(), Amount::Servings(0.5))));
        assert_eq!(p("2 slices of bread"), Some(("Bread".into(), Amount::Servings(2.0))));
        assert_eq!(p("ate kvarg"), Some(("Kvarg".into(), Amount::Servings(1.0))));
        assert_eq!(p("chili 300 g"), Some(("Chili".into(), Amount::Grams(300.0))));
        assert_eq!(p("kvarg x2"), Some(("Kvarg".into(), Amount::Servings(2.0))));
        assert_eq!(p("buy kvarg"), None); // a task, not food
        assert_eq!(p("ate a burrito"), None); // not in the library
        assert_eq!(p("kvarg"), None); // no verb, no amount
    }

    #[test]
    fn calories_for_foods_and_meals() {
        let l = lib();
        let kvarg = l.find("kvarg").unwrap();
        assert_eq!(kvarg.kcal(Amount::Servings(1.0)), 155); // 250 g × 0.62
        assert_eq!(kvarg.describe(Amount::Servings(2.0)), "2 jars (500 g)");
        assert_eq!(kvarg.nutrition(Amount::Grams(100.0)).protein, Some(10.0));
        // chili: 500 g beef (1250) + 300 g rice (390) = 1640 kcal, 4 servings, cooked 1000 g
        let chili = l.find("chili").unwrap();
        assert_eq!(chili.kcal(Amount::Servings(1.0)), 410);
        assert_eq!(chili.kcal(Amount::Grams(300.0)), 492);
        assert_eq!(chili.describe(Amount::Servings(0.5)), "½ serving (125 g)");
        assert!(l.find("minced beef").is_some());
        assert_eq!(l.search("bee").len(), 1);
    }
}
