// Train: your workouts, how to do each exercise, and the player: a big countdown ring, beeps
// between steps, timed steps moving on by themselves and reps-only sets waiting for "Done set",
// like the PC's player. The finished session goes to the PC (or waits in the queue).
"use strict";

S.train = { data: null, workout: null, open: null, session: null, play: null, summary: null };

async function trainLoad() {
  try {
    S.train.data = await api("/api/workouts");
  } catch (e) {
    if (e.server) toast(e.message, 4000);
  }
  if (S.tab === "train") rerenderIfIdle();
}
ONOPEN.train = () => { if (!S.train.play) trainLoad(); };

const fmtDur = (s) => s >= 3600 ? `${Math.floor(s / 3600)} h ${Math.round((s % 3600) / 60)} min` : s >= 60 ? `${Math.round(s / 60)} min` : `${s} s`;
const mmssT = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
const SECS_PER_REP = 3;

// ---------------------------------------------------------------- list and detail

function renderTrain(st) {
  const T = S.train;
  if (T.play) return renderPlayer();
  if (T.summary) return renderSummary();
  if (T.workout) return headerHtml() + workoutDetailHtml();
  const d = T.data;
  if (!d) return headerHtml() + `<p class="muted center">Loading your workouts…</p>`;
  const list = d.workouts.map((w) => `<div class="item" data-act="trainopen" data-id="${esc(w.id)}"><div class="grow"><b>${esc(w.name)}</b>
      <div class="muted">≈ ${fmtDur(w.seconds)} · ${w.exercises} exercise${w.exercises === 1 ? "" : "s"}${w.equipment.length ? " · " + esc(w.equipment.join(", ")) : ""}</div>
      ${w.task ? `<div class="muted">ticks off: ${esc(w.task)}</div>` : ""}</div><span class="muted">›</span></div>`).join("");
  const hist = d.history.map((s) => {
    const open = T.session === s.id;
    return `<div class="item" data-act="trainsession" data-id="${esc(s.id)}" style="flex-wrap:wrap"><div class="grow"><b>${esc(s.workout)}</b>
      <div class="muted">${esc(shortDay(s.date))} ${esc(s.start)} · ${fmtDur(s.duration_s)} · ${s.sets.length} set${s.sets.length === 1 ? "" : "s"}${s.volume ? ` · ${s.volume} kg lifted` : ""}${s.completed ? "" : " · ended early"}</div></div>
      ${open ? `<div style="width:100%;margin-top:6px">${setsListHtml(s.sets)}</div>` : ""}</div>`;
  }).join("");
  return headerHtml() + `<div class="card list"><h3>Workouts</h3>${list || `<p class="muted">No workouts yet. Build them on the PC (Workouts tab), or ask Claude.</p>`}</div>
    <div class="card list"><h3>History</h3>${hist || `<p class="muted">Workouts you do show up here.</p>`}</div>`;
}
SCREENS.train = renderTrain;

const shortDay = (iso) => new Date(iso + "T12:00").toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });

function setsListHtml(sets) {
  const groups = [];
  for (const x of sets) {
    const g = groups[groups.length - 1];
    if (g && g.exercise === x.exercise) g.sets.push(x); else groups.push({ exercise: x.exercise, name: x.name, sets: [x] });
  }
  return groups.map((g) => `<div style="margin:4px 0"><b>${esc(g.name)}</b><div class="muted">${g.sets.map((x) =>
    [x.reps ? `${x.reps} reps` : mmssT(x.seconds), x.kg ? `${x.kg} kg` : ""].filter(Boolean).join(" @ ")).join(" · ")}</div></div>`).join("");
}

function howToHtml(e) {
  if (!e) return "";
  const list = (title, xs, numbered) => xs && xs.length ? `<h3 style="margin-top:8px">${title}</h3>${xs.map((x, i) => `<div>${numbered ? `${i + 1}. ` : "• "}${esc(x)}</div>`).join("")}` : "";
  return `<div class="muted">Works: <b style="color:var(--workout)">${esc(e.muscles_main.join(", "))}</b>${e.muscles_helping.length ? ` · helping: ${esc(e.muscles_helping.join(", "))}` : ""}</div>
    ${list("HOW TO DO IT", e.steps, true)}${list("GOOD FORM", e.form)}${list("COMMON MISTAKES", e.mistakes)}
    ${e.easier || e.harder ? `<h3 style="margin-top:8px">EASIER · HARDER</h3><div>${esc(e.easier || "–")} · ${esc(e.harder || "–")}</div>` : ""}`;
}

function workoutDetailHtml() {
  const T = S.train, w = T.workout;
  const rows = w.items.map((s, i) => {
    if (s.type === "break") return `<div class="item" style="color:var(--rest)">Break ${s.seconds >= 60 ? mmssT(s.seconds) : s.seconds + " s"}${s.label ? ` · ${esc(s.label)}` : ""}</div>`;
    const what = [s.reps ? `${s.reps} reps${s.per_side ? "/side" : ""}` : "", s.seconds ? `${s.seconds} s` : "", s.kg ? `${s.kg} kg` : "", s.rest_s ? `rest ${s.rest_s} s` : ""].filter(Boolean).join(" · ");
    const open = T.open === i;
    return `<div class="item" data-act="trainhow" data-i="${i}" style="flex-wrap:wrap"><div class="grow"><b>${esc(s.name)}</b><div class="muted">${s.sets} × ${what}</div></div>
      <span class="muted">${open ? "▾" : "ⓘ"}</span>${open ? `<div style="width:100%;margin-top:6px">${howToHtml(w.exercises[s.exercise])}</div>` : ""}</div>`;
  }).join("");
  return `<div class="row" style="margin-bottom:10px"><button class="small" data-act="trainback">‹ Workouts</button></div>
    <div class="card"><h3>≈ ${fmtDur(w.seconds)}</h3><div class="big" style="font-size:24px">${esc(w.name)}</div>
      ${w.notes ? `<p class="muted">${esc(w.notes)}</p>` : ""}
      <button class="primary block" data-act="trainstart" style="margin-top:8px;background:var(--workout);border-color:var(--workout)">▶ Start workout</button></div>
    <div class="card list">${rows}</div>`;
}

// ---------------------------------------------------------------- the player

let audio = null;
function beep(freq, ms) {
  try {
    audio = audio || new (window.AudioContext || window.webkitAudioContext)();
    const o = audio.createOscillator(), g = audio.createGain();
    o.frequency.value = freq;
    o.type = "sine";
    g.gain.setValueAtTime(0.25, audio.currentTime);
    g.gain.exponentialRampToValueAtTime(0.001, audio.currentTime + ms / 1000);
    o.connect(g).connect(audio.destination);
    o.start();
    o.stop(audio.currentTime + ms / 1000);
  } catch (_) {}
  if (navigator.vibrate) navigator.vibrate(freq > 1000 ? 180 : 60);
}

let wakeLock = null;
async function keepAwake(on) {
  try {
    if (on && "wakeLock" in navigator && !wakeLock) {
      wakeLock = await navigator.wakeLock.request("screen");
      wakeLock.addEventListener("release", () => (wakeLock = null));
    } else if (!on && wakeLock) {
      await wakeLock.release();
      wakeLock = null;
    }
  } catch (_) {}
}
document.addEventListener("visibilitychange", () => { if (!document.hidden && S.train.play) keepAwake(true); });

function startPlayer() {
  const w = S.train.workout, now = Date.now(), d = new Date();
  S.train.play = { w, idx: 0, stepStart: now, pausedAt: null, pausedFor: 0, done: [], reps: null, kg: null, lastTick: null,
    started: now, date: localDate(d), start: localTime(d), confirmEnd: false, howto: localStorage.getItem("mk_howto") !== "0" };
  enterStep();
  beep(880, 180);
  keepAwake(true);
  document.body.classList.add("playing");
  window.scrollTo(0, 0);
  rerender();
  clearInterval(startPlayer.timer);
  startPlayer.timer = setInterval(tick, 200);
}

function enterStep(at) {
  const P = S.train.play, s = P.w.steps[P.idx];
  P.stepStart = at || Date.now();
  P.pausedFor = 0;
  P.pausedAt = null;
  P.lastTick = null;
  if (s && s.type === "set") { P.reps = s.reps; P.kg = s.kg; }
}

function elapsed() {
  const P = S.train.play;
  return ((P.pausedAt || Date.now()) - P.stepStart - P.pausedFor) / 1000;
}

const stepDuration = (s) => s.type === "rest" ? s.seconds : s.seconds;

/// Records the current set (if any) and moves on; `at` carries over time a timed step overran.
function nextStep(at) {
  const P = S.train.play, s = P.w.steps[P.idx];
  if (s && s.type === "set") {
    const secs = at ? stepDuration(s) : Math.round(elapsed());
    P.done.push({ exercise: s.exercise, name: s.name, set: s.set, reps: P.reps ?? null, seconds: secs, kg: P.kg ?? null });
  }
  P.idx += 1;
  if (P.idx >= P.w.steps.length) return finishPlayer(true);
  beep(1320, 220);
  enterStep(at);
  rerender();
}

function tick() {
  const P = S.train.play;
  if (!P || P.pausedAt) return;
  let s = P.w.steps[P.idx], d = s && stepDuration(s);
  // timed steps move on by themselves (several at once if the phone was asleep)
  let guard = 0;
  while (d && elapsed() >= d && guard++ < 50) {
    const end = P.stepStart + P.pausedFor + d * 1000;
    nextStep(end);
    if (!S.train.play) return;
    s = P.w.steps[P.idx];
    d = stepDuration(s);
  }
  if (d) {
    const left = Math.ceil(d - elapsed());
    if (left >= 1 && left <= 3 && P.lastTick !== left) { P.lastTick = left; beep(880, 120); }
  }
  paintPlayer();
}

function totalLeft() {
  const P = S.train.play;
  let t = 0;
  P.w.steps.forEach((s, i) => {
    if (i < P.idx) return;
    const d = stepDuration(s) || (s.reps || 10) * SECS_PER_REP * (s.per_side ? 2 : 1);
    t += i === P.idx && stepDuration(s) ? Math.max(0, d - elapsed()) : i === P.idx ? 0 : d;
  });
  return t;
}

const RING = 2 * Math.PI * 100;

/// Updates the numbers and the ring without rebuilding the page.
function paintPlayer() {
  const P = S.train.play, s = P.w.steps[P.idx], d = stepDuration(s);
  const big = $("#pl_big"), ring = $("#pl_ring"), total = $("#pl_total"), small = $("#pl_small");
  if (!big) return;
  if (d) {
    const left = Math.max(0, d - elapsed());
    big.textContent = left >= 60 ? mmssT(Math.ceil(left)) : Math.ceil(left);
    ring.style.strokeDashoffset = RING * (1 - left / d);
  } else {
    big.textContent = P.reps ?? "–";
    small.textContent = `reps${s.per_side ? " per side" : ""}${P.kg ? ` · ${P.kg} kg` : ""} · ${mmssT(elapsed())}`;
    ring.style.strokeDashoffset = 0;
  }
  total.textContent = mmssT(totalLeft());
}

function renderPlayer() {
  const P = S.train.play, s = P.w.steps[P.idx], rest = s.type === "rest", d = stepDuration(s);
  const color = rest ? "var(--rest)" : "var(--workout)";
  const title = rest ? esc(s.label || "Rest") : esc(s.name);
  const nextSet = P.w.steps.slice(P.idx + 1).find((x) => x.type === "set");
  const sub = rest ? (nextSet ? `next: ${esc(nextSet.name)} (set ${nextSet.set})` : "last one coming up") : `set ${s.set} of ${s.sets}`;
  const small = d ? (s.type === "set" ? [s.reps ? `${s.reps} reps${s.per_side ? " per side" : ""}` : "", s.kg ? `${s.kg} kg` : ""].filter(Boolean).join(" · ") || "keep going" : "rest") : "";
  const doneLabel = rest ? "Skip rest ⏭" : d ? "✔ Done" : "✔ Done set";
  const howEx = s.type === "set" ? s.exercise : nextSet && nextSet.exercise;
  const then = P.w.steps.slice(P.idx + 1, P.idx + 4).map((x) => x.type === "rest" ? `Rest ${x.seconds >= 60 ? mmssT(x.seconds) : x.seconds + " s"}` : `${x.name} ${x.set}/${x.sets}`);
  return `<div class="row" style="padding:6px 2px 4px"><span class="muted grow">${esc(P.w.name)} · ${P.idx + 1}/${P.w.steps.length} · <span id="pl_total"></span> left</span>
      ${P.confirmEnd ? `<button class="small rec" data-act="plend">End + save</button><button class="small" data-act="plkeep">Keep going</button>` : `<button class="small" data-act="plaskend">End</button>`}</div>
    <div class="center" style="margin-top:6px"><div style="font-size:24px;font-weight:650;color:${rest ? color : "var(--text)"}">${title}</div><div class="muted" style="font-size:15px">${sub}</div></div>
    <div style="position:relative;width:min(78vw,300px);aspect-ratio:1;margin:12px auto">
      <svg viewBox="0 0 240 240" style="width:100%;height:100%;transform:rotate(-90deg)">
        <circle cx="120" cy="120" r="100" fill="none" stroke="var(--fill)" stroke-width="16"/>
        <circle id="pl_ring" cx="120" cy="120" r="100" fill="none" stroke="${color}" stroke-width="16" stroke-linecap="round"
          stroke-dasharray="${RING}" style="opacity:${P.pausedAt ? 0.45 : 1}"/></svg>
      <div style="position:absolute;inset:0;display:grid;place-content:center;text-align:center">
        ${P.pausedAt ? `<div style="color:var(--warning);font-size:14px">paused</div>` : ""}
        <div id="pl_big" style="font-size:64px;font-weight:700;line-height:1"></div><div id="pl_small" class="muted" style="font-size:14px">${esc(small)}</div></div></div>
    <div class="grid2" style="grid-template-columns:1fr 1fr 1fr 1fr;gap:6px">
      <button data-act="plback">⏮</button><button data-act="plpause">${P.pausedAt ? "▶" : "⏸"}</button>
      <button class="primary" data-act="plnext" style="grid-column:span 2;background:${color};border-color:${color}">${doneLabel}</button></div>
    <div class="row" style="margin-top:6px"><button class="small grow" data-act="plskip">Skip without counting ⏭</button></div>
    ${s.type === "set" ? `<div class="card" style="margin-top:10px"><h3>This set (what you actually did)</h3><div class="row">
      <button class="small" data-act="plreps" data-d="-1">−</button><span class="grow center"><b>${P.reps ?? "–"}</b> reps</span><button class="small" data-act="plreps" data-d="1">+</button>
      ${P.kg != null || s.kg != null ? `<button class="small" data-act="plkg" data-d="-0.5">−</button><span class="grow center"><b>${P.kg ?? "–"}</b> kg</span><button class="small" data-act="plkg" data-d="0.5">+</button>` : ""}</div></div>` : ""}
    ${then.length ? `<p class="muted center">Then: ${esc(then.join("  ›  "))}</p>` : ""}
    ${howEx ? `<div class="card"><div class="row" data-act="plhowto"><h3 class="grow" style="margin:0">${rest ? "UP NEXT · " : ""}HOW TO: ${esc(P.w.exercises[howEx]?.name || "")}</h3><span class="muted">${P.howto ? "▾" : "▸"}</span></div>
      ${P.howto ? `<div style="margin-top:6px">${howToHtml(P.w.exercises[howEx])}</div>` : ""}</div>` : ""}`;
}

async function finishPlayer(completed) {
  const T = S.train, P = T.play;
  clearInterval(startPlayer.timer);
  keepAwake(false);
  document.body.classList.remove("playing");
  T.play = null;
  const duration_s = Math.round((Date.now() - P.started) / 1000);
  T.summary = { name: P.w.name, completed, duration_s, sets: P.done };
  if (completed) beep(1320, 450);
  rerender();
  if (P.done.length) {
    await send("/api/workout/session", { workout_id: P.w.id, workout: P.w.name, date: P.date, time: P.start, start: P.start, duration_s, sets: P.done, completed });
    trainLoad();
  }
}

function renderSummary() {
  const R = S.train.summary;
  const volume = R.sets.reduce((a, x) => a + (x.reps || 0) * (x.kg || 0), 0);
  return headerHtml() + `<div class="card center"><div style="font-size:40px">${R.completed ? "🎉" : "👍"}</div>
      <div class="big" style="font-size:22px">${R.completed ? "Workout done!" : "Workout ended"}</div>
      <div class="muted">${esc(R.name)} · ${mmssT(R.duration_s)} · ${R.sets.length} set${R.sets.length === 1 ? "" : "s"}${volume ? ` · ${Math.round(volume)} kg lifted` : ""}</div></div>
    ${R.sets.length ? `<div class="card">${setsListHtml(R.sets)}</div>` : `<p class="muted center">No sets were done, so nothing was saved.</p>`}
    <button class="primary block" data-act="traindone">Done</button>`;
}

Object.assign(ACTIONS, {
  async trainopen(el) {
    try {
      S.train.workout = await api("/api/workout?id=" + encodeURIComponent(el.dataset.id));
      S.train.open = null;
      window.scrollTo(0, 0);
    } catch (e) {
      toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
    }
    rerender();
  },
  trainback() { S.train.workout = null; rerender(); },
  trainhow(el) { const i = +el.dataset.i; S.train.open = S.train.open === i ? null : i; rerender(); },
  trainsession(el) { S.train.session = S.train.session === el.dataset.id ? null : el.dataset.id; rerender(); },
  trainstart: startPlayer,
  traindone() { S.train.summary = null; S.train.workout = null; rerender(); },
  plnext() { nextStep(); },
  plback() {
    const P = S.train.play;
    if (P.idx > 0) {
      P.idx -= 1;
      const s = P.w.steps[P.idx];
      // the set being redone isn't counted twice
      if (s.type === "set") {
        const k = P.done.map((x) => x.exercise === s.exercise && x.set === s.set).lastIndexOf(true);
        if (k >= 0) P.done.splice(k, 1);
      }
    }
    enterStep();
    rerender();
  },
  plpause() {
    const P = S.train.play;
    if (P.pausedAt) { P.pausedFor += Date.now() - P.pausedAt; P.pausedAt = null; } else P.pausedAt = Date.now();
    rerender();
  },
  plskip() {
    const P = S.train.play;
    P.idx += 1;
    if (P.idx >= P.w.steps.length) return finishPlayer(true);
    beep(1320, 220);
    enterStep();
    rerender();
  },
  plreps(el) { const P = S.train.play; P.reps = Math.max(0, (P.reps || 0) + +el.dataset.d) || null; rerender(); },
  plkg(el) { const P = S.train.play; P.kg = Math.max(0, Math.round(((P.kg || 0) + +el.dataset.d) * 4) / 4) || null; rerender(); },
  plhowto() { const P = S.train.play; P.howto = !P.howto; localStorage.setItem("mk_howto", P.howto ? "1" : "0"); rerender(); },
  plaskend() { S.train.play.confirmEnd = true; rerender(); },
  plkeep() { S.train.play.confirmEnd = false; rerender(); },
  plend() { finishPlayer(false); },
});

// the ring and numbers are painted right after each rebuild
const renderTrainScreen = SCREENS.train;
SCREENS.train = (st) => { const html = renderTrainScreen(st); if (S.train.play) setTimeout(paintPlayer, 0); return html; };
