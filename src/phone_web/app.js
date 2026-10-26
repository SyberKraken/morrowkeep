// Morrowkeep on the phone: talks to the Morrowkeep app on your PC (through Tailscale).
// Entries made while the PC can't be reached wait in a queue on the phone and are sent later,
// each with the day and time it was made.
"use strict";

const S = {
  token: localStorage.getItem("mk_token") || "",
  state: null,
  tab: localStorage.getItem("mk_tab") || "today",
  quickJournal: "",
  entry: { text: "", mode: localStorage.getItem("mk_mode") || "local" },
  journal: { text: "", mood: null, day: "today", date: "" },
  food: { q: "", sel: null, amount: 1, grams: false, free: "", kcal: "", what: "" },
  ask: { text: "", job: null, food: false, status: null, reply: null, started: 0, error: "" },
  // a nutrition label photo: null, "reading", "review" or "error"
  label: { stage: null, job: null, food: null, note: "", error: "" },
  weight: "",
  rec: null,
  pendingAudio: null,
  busy: false,
};

// Screens and button actions; plan.js and the other screen files add to these.
const SCREENS = {};
const ACTIONS = {};
// input handlers by element id prefix: INPUTS["lb_"] = (el) => …
const INPUTS = {};

// ---------------------------------------------------------------- helpers

const $ = (sel) => document.querySelector(sel);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const pad = (n) => String(n).padStart(2, "0");
const localDate = (d = new Date()) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const localTime = (d = new Date()) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
const yesterday = () => { const d = new Date(); d.setDate(d.getDate() - 1); return localDate(d); };
const mmss = (s) => `${Math.floor(s / 60)}:${pad(Math.round(s % 60))}`;
const MOODS = ["rough", "meh", "okay", "good", "great"];

function toast(msg, ms = 2600) {
  const t = $("#toast");
  t.textContent = msg;
  t.style.display = "block";
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.style.display = "none"), ms);
}

function moodColor(m) {
  const t = S.state?.theme || {};
  return [t.danger, mix(t.danger, t.warning), t.warning, mix(t.warning, t.success), t.success][m - 1] || t.muted;
}
function mix(a, b) {
  if (!a || !b) return a || b;
  const p = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [x, y] = [p(a), p(b)];
  return "#" + x.map((v, i) => pad(Math.round(v * 0.4 + y[i] * 0.6).toString(16))).join("");
}

// ---------------------------------------------------------------- talking to the PC

async function api(path, opts = {}) {
  const headers = Object.assign({ Authorization: "Bearer " + S.token }, opts.headers || {});
  const res = await fetch(path, Object.assign({}, opts, { headers }));
  let body = {};
  try { body = await res.json(); } catch (_) {}
  if (res.status === 401) {
    localStorage.removeItem("mk_token");
    S.token = "";
    render();
    throw new Error(body.error || "Not paired");
  }
  if (!res.ok) {
    const e = new Error(body.error || `Your PC answered ${res.status}`);
    e.server = true;
    throw e;
  }
  return body;
}

const post = (path, body) => api(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });

// Queue for entries made while the PC can't be reached.
const queue = () => JSON.parse(localStorage.getItem("mk_queue") || "[]");
const saveQueue = (q) => localStorage.setItem("mk_queue", JSON.stringify(q));

/// Sends a change; if the PC can't be reached it's kept and sent later.
async function send(path, body, okMsg) {
  body = Object.assign({ date: localDate(), time: localTime() }, body);
  try {
    const r = await post(path, body);
    toast(okMsg || r.message || "Saved");
    await refresh();
    return true;
  } catch (e) {
    if (e.server) { toast(e.message, 4000); return false; }
    const q = queue();
    q.push({ path, body });
    saveQueue(q);
    toast("Your PC isn't reachable. Saved on the phone, it'll be sent later.", 4000);
    render();
    return true;
  }
}

async function flushQueue() {
  const q = queue();
  if (!q.length || !S.token) return;
  const left = [];
  for (const job of q) {
    try { await post(job.path, job.body); }
    catch (e) { if (!e.server) left.push(job); }
  }
  saveQueue(left);
  if (left.length < q.length) { toast(`Sent ${q.length - left.length} saved entr${q.length - left.length === 1 ? "y" : "ies"}`); await refresh(); }
}

async function refresh() {
  try {
    S.state = await api("/api/state");
    applyTheme(S.state.theme);
    S.offline = false;
  } catch (e) {
    if (!S.token) return;
    S.offline = true;
  }
  render();
}

function applyTheme(t) {
  if (!t) return;
  const r = document.documentElement.style;
  const map = { page: "--page", card: "--card", border: "--border", input: "--input", text: "--text", muted: "--muted", fill: "--fill", selection: "--selection",
    accent: "--accent", on_color: "--on-color", success: "--success", danger: "--danger", warning: "--warning", brand: "--brand", calories: "--calories",
    weight: "--weight", journal: "--journal", goal: "--goal", claude: "--claude", workout: "--workout", rest: "--rest", trend: "--trend" };
  for (const [k, v] of Object.entries(map)) if (t[k]) r.setProperty(v, t[k]);
  document.querySelector('meta[name="theme-color"]').setAttribute("content", t.page);
  localStorage.setItem("mk_theme", JSON.stringify(t));
}

// ---------------------------------------------------------------- pairing

function deviceName() {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return "iPhone";
  if (/iPad/.test(ua)) return "iPad";
  if (/Android/.test(ua)) return "Android phone";
  return "Phone";
}

async function pair(code) {
  try {
    const r = await fetch("/api/pair", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ code, name: deviceName() }) });
    const body = await r.json();
    if (!r.ok) throw new Error(body.error || "Pairing failed");
    S.token = body.token;
    localStorage.setItem("mk_token", S.token);
    shareToken();
    history.replaceState(null, "", "/");
    toast("Paired. Add Morrowkeep to your home screen to open it like an app.", 5000);
    await refresh();
  } catch (e) {
    S.pairError = e.message;
    render();
  }
}

function renderPair() {
  return `<header><span class="brand">Morrowkeep</span></header>
  <div class="card">
    <h3>Pair this phone</h3>
    <p class="muted">On your PC, open Morrowkeep → Settings → Phone → <b>Pair a phone</b>, then scan the QR code, or type the code here.</p>
    <div class="row"><input class="grow" id="pairCode" placeholder="Pairing code" autocapitalize="characters" autocomplete="off"><button class="primary" data-act="pair">Pair</button></div>
    ${S.pairError ? `<div class="error">${esc(S.pairError)}</div>` : ""}
  </div>`;
}

// ---------------------------------------------------------------- screens

function headerHtml() {
  const q = queue().length;
  const on = S.state?.notifications;
  return `<header><span class="brand">Morrowkeep</span><span class="muted">${esc(S.state?.date_label || "")}
    ${q ? `<span id="queue"> · ${q} waiting</span>` : ""}${S.offline ? ` · <span style="color:var(--warning)">PC offline</span>` : ""}
    <button class="small" data-act="bell" aria-label="Notifications" style="margin-left:6px;${on ? "" : "opacity:.7"}">${on ? "🔔" : "🔕"}</button></span></header>
    ${S.bellOpen ? notifPanel() : ""}`;
}

// ---------------------------------------------------------------- notifications

const pushSupported = () => "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;
const isIOS = () => /iPhone|iPad|iPod/.test(navigator.userAgent);
const standalone = () => window.matchMedia("(display-mode: standalone)").matches || navigator.standalone;

function notifPanel() {
  const on = S.state?.notifications;
  let body;
  if (!pushSupported()) {
    body = isIOS() && !standalone()
      ? `<p>On iPhone, notifications work when Morrowkeep is opened from its Home Screen icon (iOS 16.4 or newer). Add it with Share → Add to Home Screen, then open it from there.</p>`
      : `<p>This browser can't show notifications from web apps.</p>`;
  } else if (Notification.permission === "denied") {
    body = `<p>Notifications are blocked for Morrowkeep. Allow them in your phone's settings (or the browser's site settings), then come back here.</p>`;
  } else if (on) {
    body = `<p>Notifications are on. Reminders come here when you're away from your PC (or as set in Settings › Reminders on the PC).</p>
      <div class="row"><button class="grow" data-act="pushtest">Send test</button><button class="grow" data-act="pushoff">Turn off</button></div>`;
  } else {
    body = `<p>Get Morrowkeep's reminders on this phone: calories, weight, journal, tasks left today, and tasks starting soon.</p>
      <button class="primary block" data-act="pushon">Turn on notifications</button>`;
  }
  return `<div class="card"><h3>Notifications</h3>${body}</div>`;
}

function b64ToBytes(s) {
  const pad = "=".repeat((4 - (s.length % 4)) % 4);
  const raw = atob((s + pad).replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

async function pushOn() {
  try {
    const perm = await Notification.requestPermission();
    if (perm !== "granted") { toast("Notifications weren't allowed", 4000); return rerender(); }
    const reg = await navigator.serviceWorker.ready;
    const { key } = await api("/api/push/key");
    let sub = await reg.pushManager.getSubscription();
    if (sub) await sub.unsubscribe();
    sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64ToBytes(key) });
    await post("/api/push/subscribe", sub.toJSON());
    toast("Notifications on");
    await refresh();
  } catch (e) {
    toast("Couldn't turn on notifications: " + e.message, 5000);
  }
  rerender();
}

async function pushOff() {
  try {
    const reg = await navigator.serviceWorker.ready;
    const sub = await reg.pushManager.getSubscription();
    if (sub) await sub.unsubscribe();
    await post("/api/push/unsubscribe", {});
    toast("Notifications off");
    await refresh();
  } catch (e) { toast(e.message, 4000); }
  rerender();
}

/// The service worker needs the key too (to snooze from a notification while the app is closed).
function shareToken() {
  if (!S.token || !("caches" in window)) return;
  caches.open("mk-auth").then((c) => c.put("/__token", new Response(S.token))).catch(() => {});
}

function starsHtml(st) {
  const t = st.theme;
  const star = (on, color) => `<span class="star" style="border-color:${color};background:${on ? color : "transparent"}"></span>`;
  const tasks = st.tasks;
  const done = tasks.filter((x) => x.done).length + (st.calories.total > 0) + (st.weight.today != null) + (st.journal.today > 0);
  const all = tasks.length + 3;
  return `<div class="stars">${star(st.calories.total > 0, t.calories)}${star(st.weight.today != null, t.weight)}${star(st.journal.today > 0, t.journal)}
    ${tasks.map((x) => star(x.done, x.color)).join("")}<span class="muted">&nbsp;${done} of ${all}</span></div>`;
}

function caloriesCard(st, withEntries) {
  const c = st.calories;
  const target = c.recommended ?? c.limit;
  let line = "";
  let frac = 0;
  if (target) {
    const left = target - c.total;
    line = left >= 0 ? `${left} left today` : `${-left} over today`;
    frac = Math.min(1, c.total / Math.max(1, target));
  }
  const over = target && c.total > target;
  const color = over ? "var(--danger)" : "var(--calories)";
  return `<div class="card"><h3>Calories</h3>
    <div class="row"><span class="big" style="color:${color}">${c.total}</span><span class="muted">kcal${line ? " · " + line : ""}</span></div>
    ${target ? `<div class="bar"><div style="width:${frac * 100}%;background:${color}"></div></div>` : ""}
    ${withEntries ? entriesList(c.entries) : ""}
    ${withEntries ? "" : `<button class="block" data-act="tab" data-tab="food" style="margin-top:6px">+ Log food</button>`}</div>`;
}

function entriesList(entries) {
  if (!entries.length) return `<p class="muted">Nothing logged today yet.</p>`;
  return `<div class="list">${entries.map((e) => `<div class="item"><span class="muted" style="width:44px">${esc(e.time || "")}</span>
    <b style="color:var(--calories);width:70px">${e.kcal} kcal</b><span class="grow" style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(e.description || "")}</span>
    <button class="small" data-act="delcal" data-id="${esc(e.id)}" aria-label="Remove">✕</button></div>`).join("")}</div>`;
}

function weightCard(st) {
  const w = st.weight;
  const value = S.weight || (w.today != null ? w.today.toFixed(1) : "");
  const hint = w.last ? `last ${w.last.kg.toFixed(1)} kg on ${esc(w.last.date)}` : "kg";
  return `<div class="card"><h3>Weight${w.today != null ? ` · <span style="color:var(--weight)">${w.today.toFixed(1)} kg today</span>` : ""}</h3>
    <div class="row"><input class="grow" id="weightIn" inputmode="decimal" placeholder="${esc(hint)}" value="${esc(value)}"><span class="muted">kg</span>
    <button class="primary" data-act="weight">Save</button></div>
    ${w.goal ? `<div class="muted" style="color:var(--goal);margin-top:6px">${esc(w.goal)}</div>` : ""}</div>`;
}

function recButton(target) {
  if (S.rec && S.rec.target === target) {
    const secs = (Date.now() - S.rec.started) / 1000;
    return `<button class="rec grow" data-act="recstop">■ Stop ${mmss(secs)}</button>`;
  }
  return `<button class="grow" data-act="recstart" data-target="${target}" ${S.rec ? "disabled" : ""}>🎤 Record</button>`;
}

function quickJournalCard(st) {
  const n = st.journal.today;
  return `<div class="card"><h3>Journal · ${n ? `${n} entr${n === 1 ? "y" : "ies"} today` : "nothing yet today"}</h3>
    <textarea id="quickJournal" style="min-height:60px" placeholder="A line about today… (#tags work)">${esc(S.quickJournal)}</textarea>
    <div class="row" style="margin-top:8px">${recButton("quick")}<button class="primary grow" data-act="quickjournal">Save</button></div>
    ${pendingAudioHtml()}</div>`;
}

function tasksCard(st) {
  if (!st.tasks.length) return `<div class="card"><h3>Today's tasks</h3><p class="muted">Nothing planned today.</p></div>`;
  return `<div class="card"><h3>Today's tasks</h3>${st.tasks.map((x) => `<div class="task" data-act="toggle" data-id="${esc(x.id)}">
    <span class="dot" style="border-color:${x.color};background:${x.done ? x.color : "transparent"}">${x.done ? "✓" : ""}</span>
    <span style="flex:1;${x.done ? "color:var(--muted);text-decoration:line-through" : ""}">${esc(x.title)}</span>
    <span class="muted">${x.repeat ? "↻ " : ""}${esc(x.time || "")}</span></div>`).join("")}</div>`;
}

// The entry bar, like the PC's: Local mode understands tasks, "450 kcal pasta", "82.4 kg",
// "done vacuum", "1 serving kvarg", "journal: …", "goal 76 kg by feb 1"; Claude mode asks Claude.
function entryCardHtml(st) {
  const E = S.entry, claude = E.mode === "claude";
  const hint = claude ? "Ask Claude: \"plan 3 gym sessions this week\", \"I had a burrito\"…"
    : "gym tomorrow 6pm #workout · 450 kcal pasta · 82.4 kg · done vacuum";
  return `<div class="card"><div class="row">
      <button class="mode ${claude ? "claude" : ""}" data-act="entrymode" ${st.claude ? "" : "disabled"}>${claude ? "✦ Claude" : "Local"}</button>
      <input class="grow" id="entryText" placeholder="${esc(hint)}" value="${esc(E.text)}" enterkeyhint="send" autocomplete="off">
      <button class="primary" data-act="entryadd" ${claude && S.ask.job ? "disabled" : ""}>${claude ? "Ask" : "Add"}</button></div></div>
    ${askPreviewHtml()}`;
}

async function entrySubmit() {
  const E = S.entry, text = E.text.trim();
  if (!text) return toast("Type something first");
  if (E.mode === "claude") return startAsk(text, false).then(() => { E.text = ""; rerender(); });
  try {
    const r = await post("/api/entry", { text, date: localDate() });
    E.text = "";
    if (r.claude) return startAsk(r.claude, true);
    toast(r.message || "Saved");
    await refresh();
    if (S.tab === "plan" && typeof planLoad === "function") planLoad();
  } catch (e) {
    if (e.server) return toast(e.message, 4000);
    // the PC can't be reached: keep it and send it later
    const q = queue();
    q.push({ path: "/api/entry", body: { text, date: localDate(), time: localTime() } });
    saveQueue(q);
    E.text = "";
    toast("Your PC isn't reachable. Saved on the phone, it'll be sent later.", 4000);
    rerender();
  }
}

function renderToday(st) {
  return headerHtml() + entryCardHtml(st) + starsHtml(st) + caloriesCard(st, false) + weightCard(st) + quickJournalCard(st) + tasksCard(st);
}

// More: everything that doesn't need a tab of its own
function renderMore(st) {
  const row = (tab, ic, title, sub) => `<div class="item" data-act="tab" data-tab="${tab}"><span style="font-size:20px;width:28px">${ic}</span>
    <div class="grow"><b>${title}</b><div class="muted">${sub}</div></div><span class="muted">›</span></div>`;
  return headerHtml() + `<div class="card list menu">
    ${row("progress", "📈", "Progress", "Calories and weight charts, body numbers, habit streaks")}
    ${row("ask", "✦", "Ask Claude", "Plan, log or change anything in words")}
    </div>
    ${notifPanel()}`;
}

Object.assign(SCREENS, { today: renderToday, food: renderFood, journal: renderJournal, ask: renderAsk, more: renderMore });

function foodKcal(f, amount, grams) {
  const g = grams ? amount : amount * f.serving_g;
  return Math.ceil(f.kcal_100g * g / 100 - 0.001);
}

// ---------------------------------------------------------------- nutrition label photos

const LABEL_FIELDS = [
  ["name", "Name", "text"], ["kcal_per_100g", "kcal / 100 g", "num"], ["protein_per_100g", "protein g", "num"],
  ["carbs_per_100g", "carbs g", "num"], ["fat_per_100g", "fat g", "num"], ["serving_name", "1 serving is a…", "text"], ["serving_g", "serving g", "num"],
];

function labelCardHtml(st) {
  const L = S.label;
  if (!st.claude) return "";
  if (L.stage === "reading") return `<div class="card"><h3>📷 Nutrition label</h3><p class="muted">Claude is reading the label on your PC…</p></div>`;
  if (L.stage === "error") return `<div class="card"><h3>📷 Nutrition label</h3><div class="error">${esc(L.error)}</div>
    <div class="row" style="margin-top:8px"><button class="grow" data-act="scanlabel">Take another photo</button><button data-act="labeldiscard">Close</button></div></div>`;
  if (L.stage === "review") {
    const f = L.food;
    const field = ([k, label, kind]) => `<label class="lbl"><span class="muted">${label}</span><input id="lb_${k}" ${kind === "num" ? 'inputmode="decimal"' : ""} value="${esc(f[k] ?? "")}"></label>`;
    return `<div class="card"><h3>📷 Check the numbers</h3>
      ${L.note ? `<p class="muted" style="color:var(--claude)">Claude: ${esc(L.note)}</p>` : ""}
      ${field(LABEL_FIELDS[0])}
      <div class="grid2">${LABEL_FIELDS.slice(1).map(field).join("")}</div>
      <div class="row" style="margin-top:10px"><button class="primary grow" data-act="labelsave">Save to my foods</button><button data-act="labelsavelog">Save + log 1</button></div>
      <div class="row" style="margin-top:6px"><button class="small" data-act="labeldiscard">Discard</button></div></div>`;
  }
  return `<div class="card"><div class="row"><div class="grow"><b>📷 Scan a nutrition label</b><div class="muted">Swedish or English. Claude reads it and you check it before it's saved.</div></div>
    <button data-act="scanlabel">Scan</button></div></div>`;
}

/// Shrinks a photo to at most 1568 px (what Claude reads anyway) as JPEG, upright.
async function labelJpeg(file) {
  const img = await createImageBitmap(file, { imageOrientation: "from-image" });
  const k = Math.min(1, 1568 / Math.max(img.width, img.height));
  const c = document.createElement("canvas");
  c.width = Math.round(img.width * k);
  c.height = Math.round(img.height * k);
  c.getContext("2d").drawImage(img, 0, 0, c.width, c.height);
  return new Promise((ok) => c.toBlob(ok, "image/jpeg", 0.85));
}

async function uploadLabel(file) {
  const L = S.label;
  Object.assign(L, { stage: "reading", job: null, food: null, note: "", error: "" });
  rerender();
  try {
    const jpeg = await labelJpeg(file);
    const r = await api("/api/food/label", { method: "POST", headers: { "Content-Type": "image/jpeg" }, body: jpeg });
    L.job = r.job;
    pollLabel(r.job);
  } catch (e) {
    Object.assign(L, { stage: "error", error: e.server ? e.message : "Your PC isn't reachable right now. Try again when it's on." });
    rerender();
  }
}

async function pollLabel(job) {
  const L = S.label;
  if (L.job !== job || L.stage !== "reading") return;
  try {
    const r = await api("/api/food/label?job=" + encodeURIComponent(job));
    if (L.job !== job) return;
    if (!r.done) return void setTimeout(() => pollLabel(job), 1500);
    if (r.error) Object.assign(L, { stage: "error", error: r.error });
    else Object.assign(L, { stage: "review", food: r.food, note: r.note || "" });
  } catch (e) {
    Object.assign(L, { stage: "error", error: e.message });
  }
  rerender();
}

async function saveLabel(andLog) {
  const L = S.label, f = L.food;
  const body = { job: L.job };
  for (const [k, , kind] of LABEL_FIELDS) {
    const v = String(f[k] ?? "").trim();
    body[k] = kind === "num" ? (v === "" ? null : parseFloat(v.replace(",", "."))) : v;
  }
  try {
    const r = await post("/api/food/new", body);
    Object.assign(L, { stage: null, job: null, food: null, note: "" });
    if (S.lib.data) libLoad();
    if (andLog) await send("/api/food", { id: r.id, servings: 1 });
    else { toast(r.message); await refresh(); }
    Object.assign(S.food, { sel: r.id, amount: 1, grams: false });
  } catch (e) {
    toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
  }
  rerender();
}

function renderFood(st) {
  if (S.foodMode === "library") return renderLibrary(st);
  const F = S.food;
  const foods = st.foods;
  const q = F.q.trim().toLowerCase();
  const byId = (id) => foods.find((f) => f.id === id);
  let list;
  if (q) list = foods.filter((f) => q.split(/\s+/).every((w) => f.name.toLowerCase().includes(w))).slice(0, 12);
  else {
    const ids = [...foods.filter((f) => f.favorite).map((f) => f.id), ...st.recent_foods];
    list = [...new Set(ids)].map(byId).filter(Boolean).slice(0, 10);
  }
  const sel = F.sel && byId(F.sel);
  let selHtml = "";
  if (sel) {
    const unit = F.grams ? "g" : (F.amount === 1 ? sel.serving_name : sel.serving_name + "s");
    const g = F.grams ? F.amount : Math.round(F.amount * sel.serving_g);
    selHtml = `<div class="card"><div class="row"><b class="grow">${esc(sel.name)}</b><span class="muted">${sel.kcal_serving} kcal / ${esc(sel.serving_name)}</span></div>
      <div class="row" style="margin-top:10px"><button data-act="amt" data-d="-1">−</button>
      <div class="grow center"><b>${F.grams ? F.amount : F.amount}</b> ${esc(unit)}${F.grams ? "" : ` <span class="muted">(${g} g)</span>`}</div>
      <button data-act="amt" data-d="1">+</button></div>
      <div class="row" style="margin-top:8px"><button class="small" data-act="unit">${F.grams ? "Use servings" : "Use grams"}</button>
      <button class="primary grow" data-act="logfood">Log ${foodKcal(sel, F.amount, F.grams)} kcal</button></div></div>`;
  }
  const askBox = S.ask.food && (S.ask.job || S.ask.reply || S.ask.error) ? askPreviewHtml() : "";
  return headerHtml() + foodModeHtml() + labelCardHtml(st) + `
    <div class="card"><h3>Your foods</h3>
      <input id="foodQ" placeholder="Search your foods…" value="${esc(F.q)}" autocomplete="off">
      <div class="chips" style="margin-top:10px">${list.map((f) => `<button class="chip" data-act="selfood" data-id="${esc(f.id)}" style="${F.sel === f.id ? "border-color:var(--accent)" : ""}">${esc(f.name)} · ${f.kcal_serving}</button>`).join("") ||
        `<span class="muted">${foods.length ? "Nothing matches." : "Your food library is empty. Add foods on your PC (Foods tab)."}</span>`}</div></div>
    ${selHtml}
    <div class="card"><h3>Not in your foods?</h3>
      ${st.claude ? `<div class="row"><input class="grow" id="freeFood" placeholder="ate a chicken burrito" value="${esc(F.free)}"><button data-act="estimate" ${S.ask.job ? "disabled" : ""}>✦ Estimate</button></div>
      <p class="muted" style="margin:6px 0 10px">Claude estimates it on your PC; you check it before it's added.</p>` : ""}
      <div class="row"><input id="kcalIn" inputmode="numeric" placeholder="kcal" style="width:90px" value="${esc(F.kcal)}"><input class="grow" id="kcalWhat" placeholder="what was it? (optional)" value="${esc(F.what)}">
      <button class="primary" data-act="addkcal">Add</button></div></div>
    ${askBox}
    ${caloriesCard(st, true)}`;
}

function moodsHtml(sel, act) {
  return `<div class="moods"><span class="muted">Mood</span>${[1, 2, 3, 4, 5].map((m) => `<button class="mood" data-act="${act}" data-m="${m}" aria-label="${MOODS[m - 1]}"
    style="border-color:${moodColor(m)};background:${sel === m ? moodColor(m) : "transparent"}"></button>`).join("")}<span class="muted">${sel ? MOODS[sel - 1] : ""}</span></div>`;
}

function pendingAudioHtml() {
  if (!S.pendingAudio) return "";
  return `<div class="error">A recording couldn't be sent. <button class="small" data-act="retryaudio">Try again</button> <button class="small" data-act="dropaudio">Discard</button></div>`;
}

function journalDay() {
  const J = S.journal;
  return J.day === "today" ? localDate() : J.day === "yesterday" ? yesterday() : J.date || localDate();
}

function renderJournal(st) {
  const J = S.journal;
  const groups = {};
  for (const e of st.journal.recent) (groups[e.date] = groups[e.date] || []).push(e);
  const label = (d) => d === localDate() ? "Today" : d === yesterday() ? "Yesterday" : d;
  const entries = Object.keys(groups).sort().reverse().map((d) => `<h3 style="margin:14px 2px 4px;font-size:13px;color:var(--muted)">${esc(label(d))}</h3>
    <div class="card">${groups[d].map((e) => `<div class="entry"><div class="meta">${esc(e.time || "–")}${e.mood ? ` · <span style="color:${moodColor(e.mood)}">${esc(e.mood_label)}</span>` : ""}
      ${e.audio_secs ? ` · 🎤 ${mmss(e.audio_secs)}` : ""}${e.kind === "reflection" ? ` · <span style="color:var(--claude)">reflection</span>` : ""}</div>
      ${e.text ? `<div>${esc(e.text)}</div>` : ""}
      ${e.transcribing ? `<div class="muted"><i>Transcribing on your PC…</i></div>` : e.transcript ? `<div style="font-style:italic">${esc(e.transcript)}</div>` : ""}</div>`).join("")}</div>`).join("");
  return headerHtml() + `<div class="card">
      <div class="row"><h3 class="grow" style="margin:0">Journal</h3><span class="muted">🔥 ${st.journal.streak} day${st.journal.streak === 1 ? "" : "s"} in a row</span></div>
      <p class="muted" style="font-style:italic;margin:6px 0">${esc(st.journal.prompt)}</p>
      <textarea id="journalText" placeholder="Write about your day… #tags work">${esc(J.text)}</textarea>
      <div style="margin:10px 0">${moodsHtml(J.mood, "jmood")}</div>
      <div class="row"><span class="muted">Day</span><select id="jday" class="grow">
        <option value="today" ${J.day === "today" ? "selected" : ""}>Today</option>
        <option value="yesterday" ${J.day === "yesterday" ? "selected" : ""}>Yesterday</option>
        <option value="pick" ${J.day === "pick" ? "selected" : ""}>Pick a date…</option></select>
        ${J.day === "pick" ? `<input id="jdate" type="date" max="${localDate()}" value="${esc(J.date || localDate())}" style="width:auto">` : ""}</div>
      <div class="row" style="margin-top:10px">${recButton("journal")}<button class="primary grow" data-act="savejournal">Save</button></div>
      ${pendingAudioHtml()}</div>${entries || `<p class="muted center">No entries in the last few days.</p>`}`;
}

function askPreviewHtml() {
  const A = S.ask;
  if (A.job && !A.reply) {
    const secs = Math.round((Date.now() - A.started) / 1000);
    return `<div class="card"><h3 style="color:var(--claude)">Claude</h3><p>Thinking on your PC… ${secs} s</p>
      <p class="muted">"${esc(A.request)}"</p></div>`;
  }
  if (A.error) return `<div class="card"><div class="error">${esc(A.error)}</div><button class="small" data-act="askclear" style="margin-top:8px">OK</button></div>`;
  if (!A.reply) return "";
  const r = A.reply;
  return `<div class="card" style="border-color:var(--claude)"><h3 style="color:var(--claude)">Claude · "${esc(A.request)}"</h3>
    ${r.message ? `<p style="font-style:italic;margin:4px 0">${esc(r.message)}</p>` : ""}
    ${(r.errors || []).map((e) => `<div class="error">${esc(e)}</div>`).join("")}
    ${r.lines && r.lines.length ? `<div class="preview">${esc(r.lines.join("\n"))}</div>` : ""}
    <div class="row">${r.can_apply ? `<button class="primary grow" data-act="askapply">Apply ${r.lines.length} change${r.lines.length === 1 ? "" : "s"}</button>` : ""}
    <button class="grow" data-act="askdiscard">Discard</button></div></div>`;
}

function renderAsk(st) {
  const A = S.ask;
  return headerHtml() + `<div class="card"><h3>Ask Claude</h3>
    <p class="muted" style="margin:0 0 8px">Anything you'd type in Morrowkeep's entry bar: "I ran Monday and Tuesday", "plan 3 gym sessions this week", "I had a burrito for lunch".</p>
    <textarea id="askText" placeholder="What should Claude do?">${esc(A.text)}</textarea>
    <button class="primary block" data-act="ask" style="margin-top:8px" ${A.job || !st.claude ? "disabled" : ""}>Ask</button>
    ${st.claude ? "" : `<div class="error">Claude Code wasn't found on your PC.</div>`}</div>
    ${!A.food ? askPreviewHtml() : ""}`;
}

// ---------------------------------------------------------------- render

function render() {
  const app = $("#app");
  const nav = $("#nav");
  // don't rebuild under the user's fingers while they're typing
  const active = document.activeElement;
  const typing = active && (active.tagName === "INPUT" || active.tagName === "TEXTAREA");
  if (typing && S.state && S.token && !render.force) return;
  render.force = false;
  if (!S.token) {
    nav.classList.add("hidden");
    app.innerHTML = renderPair();
    return;
  }
  if (!S.state) {
    nav.classList.add("hidden");
    app.innerHTML = headerHtml() + `<div class="card"><p>${S.offline ? "Can't reach Morrowkeep on your PC. Is the PC on, Morrowkeep running, and Tailscale connected on this phone?" : "Loading…"}</p>
      ${S.offline ? `<button class="primary block" data-act="retry">Try again</button>` : ""}</div>`;
    return;
  }
  nav.classList.remove("hidden");
  const st = S.state;
  // sub-pages of More keep the More button lit
  const navTab = SCREENS[S.tab] && $(`nav button[data-tab="${S.tab}"]`) ? S.tab : "more";
  nav.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.tab === navTab));
  app.innerHTML = (SCREENS[S.tab] || renderToday)(st);
}

function rerender() {
  render.force = true;
  render();
}

// ---------------------------------------------------------------- Claude

async function startAsk(text, food) {
  text = text.trim();
  if (!text) return toast("Type something first");
  Object.assign(S.ask, { job: null, food, request: text, reply: null, error: "", started: Date.now() });
  try {
    const r = await post("/api/ask", { text, food });
    S.ask.job = r.job;
    if (food) S.food.free = ""; else S.ask.text = "";
    rerender();
    pollAsk();
  } catch (e) {
    S.ask.error = e.server ? e.message : "Your PC isn't reachable right now.";
    rerender();
  }
}

async function pollAsk() {
  if (!S.ask.job || S.ask.reply) return;
  try {
    const r = await api("/api/ask?job=" + encodeURIComponent(S.ask.job));
    if (r.status === "running") { rerenderIfIdle(); setTimeout(pollAsk, 1500); return; }
    if (r.status === "error") { S.ask.error = r.error; S.ask.job = null; }
    else S.ask.reply = r;
  } catch (e) {
    setTimeout(pollAsk, 4000);
    return;
  }
  rerender();
}

function rerenderIfIdle() {
  const a = document.activeElement;
  if (!(a && (a.tagName === "INPUT" || a.tagName === "TEXTAREA"))) rerender();
}

// ---------------------------------------------------------------- recording

async function startRec(target) {
  if (!navigator.mediaDevices?.getUserMedia) return toast("The microphone needs the https:// address (through Tailscale).", 5000);
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } });
    const ctx = new (window.AudioContext || window.webkitAudioContext)();
    const src = ctx.createMediaStreamSource(stream);
    const proc = ctx.createScriptProcessor(4096, 1, 1);
    const chunks = [];
    proc.onaudioprocess = (e) => chunks.push(new Float32Array(e.inputBuffer.getChannelData(0)));
    src.connect(proc);
    proc.connect(ctx.destination);
    S.rec = { stream, ctx, src, proc, chunks, rate: ctx.sampleRate, started: Date.now(), target };
    S.rec.timer = setInterval(() => {
      if ((Date.now() - S.rec.started) / 1000 >= 300) stopRec();
      else rerenderIfIdle();
    }, 1000);
    rerender();
  } catch (e) {
    toast("Microphone not available: " + e.message, 5000);
  }
}

function toWav(chunks, rate) {
  let n = 0;
  for (const c of chunks) n += c.length;
  const all = new Float32Array(n);
  let o = 0;
  for (const c of chunks) { all.set(c, o); o += c.length; }
  // down to 16 kHz (average each window), as Whisper wants
  const ratio = rate / 16000;
  const out = new Int16Array(Math.floor(n / ratio));
  for (let i = 0; i < out.length; i++) {
    const a = Math.floor(i * ratio), b = Math.min(n, Math.floor((i + 1) * ratio));
    let s = 0;
    for (let j = a; j < b; j++) s += all[j];
    const v = Math.max(-1, Math.min(1, s / Math.max(1, b - a)));
    out[i] = v < 0 ? v * 32768 : v * 32767;
  }
  const buf = new ArrayBuffer(44 + out.length * 2);
  const dv = new DataView(buf);
  const str = (off, s) => [...s].forEach((ch, i) => dv.setUint8(off + i, ch.charCodeAt(0)));
  str(0, "RIFF"); dv.setUint32(4, 36 + out.length * 2, true); str(8, "WAVE"); str(12, "fmt ");
  dv.setUint32(16, 16, true); dv.setUint16(20, 1, true); dv.setUint16(22, 1, true); dv.setUint32(24, 16000, true);
  dv.setUint32(28, 32000, true); dv.setUint16(32, 2, true); dv.setUint16(34, 16, true); str(36, "data"); dv.setUint32(40, out.length * 2, true);
  new Int16Array(buf, 44).set(out);
  return new Blob([buf], { type: "audio/wav" });
}

async function stopRec() {
  const R = S.rec;
  if (!R) return;
  clearInterval(R.timer);
  R.src.disconnect(); R.proc.disconnect();
  R.stream.getTracks().forEach((t) => t.stop());
  R.ctx.close();
  S.rec = null;
  const secs = (Date.now() - R.started) / 1000;
  if (secs < 1) { rerender(); return toast("Too short, nothing saved"); }
  const wav = toWav(R.chunks, R.rate);
  // text and mood typed alongside go with the recording
  const meta = R.target === "quick"
    ? { text: S.quickJournal, mood: "", date: localDate(), time: localTime() }
    : { text: S.journal.text, mood: S.journal.mood || "", date: journalDay(), time: S.journal.day === "today" ? localTime() : "" };
  S.pendingAudio = { wav, meta, target: R.target };
  await uploadAudio();
}

async function uploadAudio() {
  const P = S.pendingAudio;
  if (!P) return;
  const qs = new URLSearchParams(P.meta).toString();
  try {
    const r = await api("/api/journal/audio?" + qs, { method: "POST", headers: { "Content-Type": "audio/wav" }, body: P.wav });
    S.pendingAudio = null;
    if (P.target === "quick") S.quickJournal = ""; else Object.assign(S.journal, { text: "", mood: null });
    toast(r.message || "Recording saved", 3500);
    await refresh();
    rerender();
    // transcription runs on the PC: check back for the transcript
    setTimeout(refresh, 8000);
    setTimeout(refresh, 20000);
  } catch (e) {
    toast(e.server ? e.message : "Your PC isn't reachable. The recording is kept here until you try again.", 5000);
    rerender();
  }
}

// ---------------------------------------------------------------- actions

document.addEventListener("input", (e) => {
  const t = e.target;
  if (t.id === "quickJournal") S.quickJournal = t.value;
  else if (t.id === "journalText") S.journal.text = t.value;
  else if (t.id === "weightIn") S.weight = t.value;
  else if (t.id === "askText") S.ask.text = t.value;
  else if (t.id === "freeFood") S.food.free = t.value;
  else if (t.id === "kcalIn") S.food.kcal = t.value;
  else if (t.id === "kcalWhat") S.food.what = t.value;
  else if (t.id.startsWith("lb_") && S.label.food) S.label.food[t.id.slice(3)] = t.value;
  else if (t.id === "entryText") S.entry.text = t.value;
  else if (t.id === "foodQ") {
    S.food.q = t.value;
    // keep typing in the search box while the list updates
    const pos = t.selectionStart;
    rerender();
    const n = $("#foodQ");
    n.focus();
    n.setSelectionRange(pos, pos);
  } else if (t.id === "jdate") S.journal.date = t.value;
  else { const k = Object.keys(INPUTS).find((p) => t.id.startsWith(p)); if (k) INPUTS[k](t); }
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && e.target.id === "entryText") { e.preventDefault(); entrySubmit(); }
});

document.addEventListener("change", (e) => {
  if (e.target.id === "jday") { S.journal.day = e.target.value; rerender(); }
  if (e.target.id === "labelFile" && e.target.files[0]) { const f = e.target.files[0]; e.target.value = ""; uploadLabel(f); }
});

document.addEventListener("click", async (e) => {
  const el = e.target.closest("[data-act], nav button");
  if (!el) return;
  if (el.closest("nav")) return goTab(el.dataset.tab);
  const act = el.dataset.act;
  const st = S.state;
  switch (act) {
    case "pair": return pair($("#pairCode").value);
    case "retry": return refresh();
    case "tab": return goTab(el.dataset.tab);
    case "entrymode": S.entry.mode = S.entry.mode === "claude" ? "local" : "claude"; localStorage.setItem("mk_mode", S.entry.mode); return rerender();
    case "entryadd": return entrySubmit();
    case "weight": {
      const kg = parseFloat((S.weight || "").replace(",", "."));
      if (!(kg >= 20 && kg <= 400)) return toast("Enter your weight in kg, like 82.4");
      if (await send("/api/weight", { kg }, `Weight ${kg.toFixed(1)} kg saved`)) S.weight = "";
      return rerender();
    }
    case "quickjournal": {
      if (!S.quickJournal.trim()) return toast("Write something first");
      if (await send("/api/journal", { text: S.quickJournal })) S.quickJournal = "";
      return rerender();
    }
    case "savejournal": {
      const J = S.journal;
      if (!J.text.trim()) return toast("Write something first");
      const day = journalDay();
      if (await send("/api/journal", { text: J.text, mood: J.mood, date: day, time: J.day === "today" ? localTime() : null })) Object.assign(J, { text: "", mood: null });
      return rerender();
    }
    case "jmood": { const m = +el.dataset.m; S.journal.mood = S.journal.mood === m ? null : m; return rerender(); }
    case "toggle": {
      const t = st.tasks.find((x) => x.id === el.dataset.id);
      if (t) { t.done = !t.done; rerender(); }
      return send("/api/task/toggle", { id: el.dataset.id }, t && t.done ? `✔ ${t.title}` : "Un-ticked");
    }
    case "delcal": return send("/api/calories/delete", { id: el.dataset.id }, "Removed");
    case "scanlabel": return $("#labelFile").click();
    case "labelsave": return saveLabel(false);
    case "labelsavelog": return saveLabel(true);
    case "labeldiscard": {
      if (S.label.job) post("/api/food/label/discard", { job: S.label.job }).catch(() => {});
      Object.assign(S.label, { stage: null, job: null, food: null, note: "", error: "" });
      return rerender();
    }
    case "selfood": {
      const id = el.dataset.id;
      Object.assign(S.food, S.food.sel === id ? { sel: null } : { sel: id, amount: 1, grams: false });
      return rerender();
    }
    case "amt": {
      const F = S.food, f = st.foods.find((x) => x.id === F.sel);
      const step = F.grams ? (f && f.serving_g >= 100 ? 50 : 10) : 0.5;
      F.amount = Math.max(step, Math.round((F.amount + step * +el.dataset.d) * 100) / 100);
      return rerender();
    }
    case "unit": {
      const F = S.food, f = st.foods.find((x) => x.id === F.sel);
      if (!f) return;
      if (F.grams) Object.assign(F, { grams: false, amount: Math.max(0.5, Math.round(F.amount / f.serving_g * 2) / 2) });
      else Object.assign(F, { grams: true, amount: Math.round(F.amount * f.serving_g) });
      return rerender();
    }
    case "logfood": {
      const F = S.food;
      const body = F.grams ? { id: F.sel, grams: F.amount } : { id: F.sel, servings: F.amount };
      if (await send("/api/food", body)) Object.assign(F, { sel: null, amount: 1, grams: false, q: "" });
      return rerender();
    }
    case "addkcal": {
      const kcal = parseInt(S.food.kcal, 10);
      if (!(kcal > 0)) return toast("Enter the calories as a number");
      if (await send("/api/calories", { kcal, description: S.food.what.trim() })) Object.assign(S.food, { kcal: "", what: "" });
      return rerender();
    }
    case "estimate": return startAsk(S.food.free, true);
    case "ask": return startAsk(S.ask.text, false);
    case "askapply": {
      try {
        const r = await post("/api/ask/apply", { job: S.ask.job });
        toast(r.message || "Applied", 3500);
        Object.assign(S.ask, { job: null, reply: null, food: false });
        await refresh();
      } catch (err) { toast(err.message, 4000); }
      return rerender();
    }
    case "askdiscard":
      if (S.ask.job) post("/api/ask/discard", { job: S.ask.job }).catch(() => {});
      Object.assign(S.ask, { job: null, reply: null, error: "", food: false });
      return rerender();
    case "askclear": Object.assign(S.ask, { error: "", job: null, food: false }); return rerender();
    case "bell": S.bellOpen = !S.bellOpen; return rerender();
    case "pushon": return pushOn();
    case "pushoff": return pushOff();
    case "pushtest":
      try { const r = await post("/api/push/test", {}); toast(r.message || "Sent"); } catch (err) { toast(err.message, 4000); }
      return;
    case "recstart": return startRec(el.dataset.target);
    case "recstop": return stopRec();
    case "retryaudio": return uploadAudio();
    case "dropaudio": S.pendingAudio = null; return rerender();
    default: if (ACTIONS[act]) return ACTIONS[act](el, st);
  }
});

// Screens can load their own data when opened: ONOPEN.plan = () => …
const ONOPEN = {};

function goTab(tab) {
  S.tab = tab;
  localStorage.setItem("mk_tab", tab);
  window.scrollTo(0, 0);
  if (ONOPEN[tab]) ONOPEN[tab]();
  rerender();
}

// ---------------------------------------------------------------- start

// runs once every screen file has loaded
window.addEventListener("DOMContentLoaded", async function start() {
  const saved = localStorage.getItem("mk_theme");
  if (saved) try { applyTheme(JSON.parse(saved)); } catch (_) {}
  if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js").catch(() => {});
  const m = location.hash.match(/pair=([A-Za-z0-9]+)/);
  if (m) return pair(m[1]);
  // opened from a notification: go to the screen it's about
  const go = new URLSearchParams(location.search).get("go");
  if (go && SCREENS[go]) {
    S.tab = go;
    localStorage.setItem("mk_tab", go);
    history.replaceState(null, "", "/");
  }
  if (!SCREENS[S.tab]) S.tab = "today";
  shareToken();
  render();
  if (S.token) { await refresh(); flushQueue(); if (ONOPEN[S.tab]) ONOPEN[S.tab](); }
});

document.addEventListener("visibilitychange", () => { if (!document.hidden && S.token) { refresh(); flushQueue(); } });
window.addEventListener("online", () => flushQueue());
setInterval(() => { if (!document.hidden && S.token) { flushQueue(); refresh(); } }, 30000);
