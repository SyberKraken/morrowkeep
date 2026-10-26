// Food tab, Library mode: browse, edit and create ingredients and meals, like the PC's Foods tab.
// (Log mode, in app.js, is for logging what you ate.)
"use strict";

S.foodMode = localStorage.getItem("mk_foodmode") || "log";
S.lib = { data: null, q: "", filter: "all", edit: null, confirmDelete: false, addQ: "" };

async function libLoad() {
  try {
    S.lib.data = await api("/api/foods");
  } catch (e) {
    if (e.server) toast(e.message, 4000);
  }
  if (S.tab === "food") rerenderIfIdle();
}
ONOPEN.food = () => { if (S.foodMode === "library") libLoad(); };

function foodModeHtml() {
  const b = (m, label) => `<button class="grow ${S.foodMode === m ? "primary" : ""}" data-act="foodmode" data-m="${m}">${label}</button>`;
  return `<div class="row" style="margin-bottom:12px">${b("log", "Log food")}${b("library", "Library")}</div>`;
}

const r1 = (x) => Math.round(x * 10) / 10;
const numOr = (v) => { const s = String(v ?? "").trim().replace(",", "."); return s === "" ? null : parseFloat(s); };

// ---------------------------------------------------------------- list

function renderLibrary(st) {
  const L = S.lib, d = L.data;
  if (L.edit) return headerHtml() + (L.edit.kind === "meal" ? mealEditorHtml() : foodEditorHtml());
  if (!d) return headerHtml() + foodModeHtml() + `<p class="muted center">Loading your foods…</p>`;
  const q = L.q.trim().toLowerCase();
  const match = (x) => !q || q.split(/\s+/).every((w) => (x.name + " " + (x.aliases || []).join(" ")).toLowerCase().includes(w));
  let rows = [
    ...d.foods.map((f) => ({ ...f, meal: false, kcal_serving: Math.ceil(f.kcal_per_100g * f.serving_g / 100 - 0.001) })),
    ...d.meals.map((m) => ({ ...m, meal: true })),
  ].filter(match).filter((x) => L.filter === "all" || (L.filter === "meals" ? x.meal : L.filter === "fav" ? x.favorite : !x.meal));
  rows.sort((a, b) => (!!b.favorite - !!a.favorite) || a.name.localeCompare(b.name));
  const chip = (f, label) => `<button class="chip" data-act="libfilter" data-f="${f}" style="${L.filter === f ? "border-color:var(--accent)" : ""}">${label}</button>`;
  return headerHtml() + foodModeHtml() + labelCardHtml(st) + `<div class="card">
    <div class="row"><button class="grow" data-act="libnew" data-kind="food">+ Ingredient</button><button class="grow" data-act="libnew" data-kind="meal">+ Meal</button></div>
    <input id="lib_q" placeholder="Search ${d.foods.length} ingredient${d.foods.length === 1 ? "" : "s"} and ${d.meals.length} meal${d.meals.length === 1 ? "" : "s"}…" value="${esc(L.q)}" style="margin-top:10px" autocomplete="off">
    <div class="chips" style="margin-top:8px">${chip("all", "All")}${chip("ingredients", "Ingredients")}${chip("meals", "Meals")}${chip("fav", "★")}</div></div>
    <div class="card list">${rows.map((x) => `<div class="item" data-act="libopen" data-id="${esc(x.id)}" data-kind="${x.meal ? "meal" : "food"}">
      <span style="width:20px">${x.meal ? "🍲" : ""}</span><div class="grow"><div>${esc(x.name)} ${x.favorite ? `<span style="color:var(--warning)">★</span>` : ""}</div>
      <div class="muted">${x.kcal_serving} kcal / ${esc(x.meal ? "serving" : x.serving_name)}${x.meal ? ` · ${x.ingredients.length} ingredients` : ` · ${r1(x.kcal_per_100g)} / 100 g`}</div></div>
      <span class="muted">›</span></div>`).join("") || `<p class="muted">${q ? "Nothing matches." : "Your library is empty."}</p>`}</div>`;
}

// ---------------------------------------------------------------- ingredient

function foodEditorHtml() {
  const E = S.lib.edit;
  const field = (k, label, mode = "decimal") => `<label class="lbl"><span class="muted">${label}</span><input id="lf_${k}" inputmode="${mode}" value="${esc(E[k] ?? "")}"></label>`;
  return `<div class="row" style="margin-bottom:10px"><button class="small" data-act="libback">‹ Library</button><span class="grow"></span>
      <button class="small" data-act="libfav">${E.favorite ? `<span style="color:var(--warning)">★</span>` : "☆"} Favorite</button></div>
    <div class="card"><h3>${E.id ? "Ingredient" : "New ingredient"}</h3>
      <input id="lf_name" value="${esc(E.name)}" placeholder="Name">
      <input id="lf_aliases" value="${esc(E.aliases)}" placeholder="Other names, comma separated" style="margin-top:8px">
      <div class="grid2">${field("kcal_per_100g", "kcal / 100 g")}${field("protein_per_100g", "protein g / 100 g")}
        ${field("carbs_per_100g", "carbs g / 100 g")}${field("fat_per_100g", "fat g / 100 g")}
        ${field("serving_name", "1 serving is a…", "text")}${field("serving_g", "serving g")}</div>
      <textarea id="lf_notes" style="min-height:50px;margin-top:8px" placeholder="Notes (brand, where to buy…)">${esc(E.notes)}</textarea>
      ${libButtonsHtml()}</div>`;
}

function libButtonsHtml() {
  const L = S.lib;
  return `<div class="row" style="margin-top:10px"><button class="primary grow" data-act="libsave">Save</button>
    ${!L.edit.id ? "" : L.confirmDelete ? `<button class="rec" data-act="libdelete">Yes, delete</button><button data-act="libnodelete">No</button>`
      : `<button data-act="libaskdelete">🗑</button>`}</div>`;
}

// ---------------------------------------------------------------- meal

function mealTotals(E) {
  const foods = S.lib.data.foods;
  let kcal = 0, p = 0, c = 0, f = 0, g = 0;
  for (const x of E.ingredients) {
    const food = foods.find((y) => y.id === x.food);
    const grams = numOr(x.grams) || 0;
    if (!food) continue;
    kcal += food.kcal_per_100g * grams / 100;
    p += (food.protein_per_100g || 0) * grams / 100;
    c += (food.carbs_per_100g || 0) * grams / 100;
    f += (food.fat_per_100g || 0) * grams / 100;
    g += grams;
  }
  const servings = numOr(E.servings) || 1, cooked = numOr(E.cooked_g);
  const weight = cooked || g;
  return `${Math.round(kcal)} kcal in all · <b>${Math.ceil(kcal / servings - 0.001)} kcal</b> per serving (${Math.round(weight / servings)} g)
    <div class="muted">per serving: protein ${r1(p / servings)} g · carbs ${r1(c / servings)} g · fat ${r1(f / servings)} g${weight ? ` · ${Math.round(kcal / weight * 100)} kcal / 100 g` : ""}</div>`;
}

function mealEditorHtml() {
  const L = S.lib, E = L.edit, foods = L.data.foods;
  const q = L.addQ.trim().toLowerCase();
  const found = q ? foods.filter((f) => q.split(/\s+/).every((w) => f.name.toLowerCase().includes(w))).slice(0, 8) : [];
  return `<div class="row" style="margin-bottom:10px"><button class="small" data-act="libback">‹ Library</button><span class="grow"></span>
      <button class="small" data-act="libfav">${E.favorite ? `<span style="color:var(--warning)">★</span>` : "☆"} Favorite</button></div>
    <div class="card"><h3>${E.id ? "Meal" : "New meal"}</h3>
      <input id="lm_name" value="${esc(E.name)}" placeholder="Name">
      <div class="grid2"><label class="lbl"><span class="muted">servings it makes</span><input id="lm_servings" inputmode="decimal" value="${esc(E.servings)}"></label>
        <label class="lbl"><span class="muted">cooked weight g (optional)</span><input id="lm_cooked_g" inputmode="decimal" value="${esc(E.cooked_g ?? "")}"></label></div>
      <h3 style="margin-top:12px">Ingredients</h3>
      ${E.ingredients.map((x, i) => {
        const food = foods.find((y) => y.id === x.food);
        return `<div class="row" style="margin:6px 0"><span class="grow">${esc(food ? food.name : "(deleted)")}</span>
          <input id="lmg_${i}" inputmode="decimal" value="${esc(x.grams)}" style="width:80px"><span class="muted">g</span>
          <button class="small" data-act="lmremove" data-i="${i}">✖</button></div>`;
      }).join("") || `<p class="muted">No ingredients yet. Add them from your library below.</p>`}
      <input id="lm_add" placeholder="Add an ingredient: search your library…" value="${esc(L.addQ)}" autocomplete="off" style="margin-top:6px">
      <div class="chips" style="margin-top:6px">${found.map((f) => `<button class="chip" data-act="lmadd" data-id="${esc(f.id)}">+ ${esc(f.name)}</button>`).join("")}</div>
      <div id="lm_totals" style="margin-top:10px">${mealTotals(E)}</div>
      <textarea id="lm_notes" style="min-height:50px;margin-top:8px" placeholder="Notes">${esc(E.notes)}</textarea>
      ${libButtonsHtml()}</div>`;
}

// ---------------------------------------------------------------- actions

function libOpen(kind, item) {
  const L = S.lib;
  L.confirmDelete = false;
  L.addQ = "";
  if (kind === "meal") {
    const m = item || { name: "", servings: 1, cooked_g: null, ingredients: [], notes: "", favorite: false };
    L.edit = { kind, id: m.id || null, name: m.name, servings: m.servings, cooked_g: m.cooked_g, notes: m.notes || "", favorite: !!m.favorite,
      aliases: (m.aliases || []).join(", "), ingredients: m.ingredients.map((x) => ({ food: x.food, grams: Math.round(x.grams * 10) / 10 })) };
  } else {
    const f = item || { name: "", kcal_per_100g: "", serving_g: 100, serving_name: "serving", notes: "", favorite: false };
    // stored as 32-bit floats: 3.9 arrives as 3.9000000953674316
    const n2 = (x) => x == null || x === "" ? x : Math.round(x * 100) / 100;
    L.edit = { kind, id: f.id || null, name: f.name, aliases: (f.aliases || []).join(", "), kcal_per_100g: n2(f.kcal_per_100g), serving_g: n2(f.serving_g),
      serving_name: f.serving_name, protein_per_100g: n2(f.protein_per_100g), carbs_per_100g: n2(f.carbs_per_100g), fat_per_100g: n2(f.fat_per_100g),
      notes: f.notes || "", favorite: !!f.favorite };
  }
  window.scrollTo(0, 0);
  rerender();
}

async function libSave() {
  const E = S.lib.edit;
  if (!E.name.trim()) return toast("Give it a name");
  const item = { id: E.id, name: E.name, favorite: E.favorite, notes: E.notes, aliases: E.aliases.split(",").map((s) => s.trim()).filter(Boolean) };
  if (E.kind === "meal") {
    Object.assign(item, { servings: numOr(E.servings), cooked_g: numOr(E.cooked_g),
      ingredients: E.ingredients.map((x) => ({ food: x.food, grams: numOr(x.grams) })).filter((x) => x.grams > 0) });
  } else {
    for (const k of ["kcal_per_100g", "serving_g", "protein_per_100g", "carbs_per_100g", "fat_per_100g"]) item[k] = numOr(E[k]);
    item.serving_name = E.serving_name;
  }
  try {
    const r = await post("/api/foods/save", { kind: E.kind, item });
    toast(r.message);
    S.lib.edit = null;
    await Promise.all([libLoad(), refresh()]);
  } catch (e) {
    toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
  }
  rerender();
}

Object.assign(ACTIONS, {
  foodmode(el) {
    S.foodMode = el.dataset.m;
    localStorage.setItem("mk_foodmode", S.foodMode);
    S.lib.edit = null;
    if (S.foodMode === "library") libLoad();
    rerender();
  },
  libfilter(el) { S.lib.filter = el.dataset.f; rerender(); },
  libnew(el) { libOpen(el.dataset.kind, null); },
  libopen(el) {
    const d = S.lib.data, kind = el.dataset.kind;
    const item = (kind === "meal" ? d.meals : d.foods).find((x) => x.id === el.dataset.id);
    if (item) libOpen(kind, item);
  },
  libback() { S.lib.edit = null; rerender(); },
  libfav() { S.lib.edit.favorite = !S.lib.edit.favorite; rerender(); },
  libsave: libSave,
  libaskdelete() { S.lib.confirmDelete = true; rerender(); },
  libnodelete() { S.lib.confirmDelete = false; rerender(); },
  async libdelete() {
    try {
      const r = await post("/api/foods/delete", { id: S.lib.edit.id });
      toast(r.message);
      S.lib.edit = null;
      await Promise.all([libLoad(), refresh()]);
    } catch (e) {
      toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
    }
    rerender();
  },
  lmadd(el) {
    const f = S.lib.data.foods.find((x) => x.id === el.dataset.id);
    if (!f) return;
    S.lib.edit.ingredients.push({ food: f.id, grams: f.serving_g });
    S.lib.addQ = "";
    rerender();
  },
  lmremove(el) { S.lib.edit.ingredients.splice(+el.dataset.i, 1); rerender(); },
});

// typing: keep the cursor where it is; meal totals update in place
INPUTS.lib_q = (t) => { S.lib.q = t.value; keepTyping(t, "#lib_q"); };
INPUTS.lf_ = (t) => { S.lib.edit[t.id.slice(3)] = t.value; };
INPUTS.lm_add = (t) => { S.lib.addQ = t.value; keepTyping(t, "#lm_add"); };
INPUTS.lmg_ = (t) => { S.lib.edit.ingredients[+t.id.slice(4)].grams = t.value; $("#lm_totals").innerHTML = mealTotals(S.lib.edit); };
INPUTS.lm_ = (t) => { S.lib.edit[t.id.slice(3)] = t.value; if (t.id !== "lm_name" && t.id !== "lm_notes") $("#lm_totals").innerHTML = mealTotals(S.lib.edit); };

/// Re-renders a list as you type in its search box, keeping the focus and cursor.
function keepTyping(t, sel) {
  const pos = t.selectionStart;
  rerender();
  const n = $(sel);
  if (n) { n.focus(); n.setSelectionRange(pos, pos); }
}
