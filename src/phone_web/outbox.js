// The outbox: everything the phone changes is a step, kept in order (in IndexedDB) until the PC
// has carried it out. The phone applies waiting steps to its own copy at once, with the same
// rules as the PC (core.wasm), so counters go up and a task made offline can be ticked off
// offline. When the PC can be reached the steps are sent in order (photos and recordings first),
// and the PC runs them one by one; their results come back here.
"use strict";

const OB = { steps: [], seq: 0, history: [], sending: false, polling: false };

const PENDING_TEXT = "Waiting for your PC";

async function outboxInit() {
  try {
    OB.steps = (await IDB.get("outbox")) || [];
    OB.history = (await IDB.get("outbox_history")) || [];
  } catch (e) { console.warn("outbox", e); }
  OB.seq = OB.steps.reduce((m, s) => Math.max(m, s.seq), Number(localStorage.getItem("mk_seq") || 0));
}

async function outboxSave() {
  localStorage.setItem("mk_seq", String(OB.seq));
  await IDB.set("outbox", OB.steps);
  await IDB.set("outbox_history", OB.history.slice(-60));
}

// waiting, sent and done steps the PC's copy doesn't include yet
replicaSteps = () => OB.steps.filter((s) => s.state !== "failed" && !RP.finished.has(s.id)).map((s) => s.step);

const hhmmss = (t) => (t && t.length === 5 ? t + ":00" : t);
const newId = () => (globalThis.crypto && crypto.randomUUID ? crypto.randomUUID() : MK.call({ fn: "new_id" }));

/// Builds a step with Morrowkeep's rules (new ids, exact formats).
function makeStep(kind, args) {
  const r = MK.call(Object.assign({ fn: "make_step", kind }, args));
  if (r && r.error && !r.type) throw Object.assign(new Error(r.error), { server: true });
  return r;
}

/// Queues a step: applied to the phone's copy at once, then sent when the PC can be reached.
/// Returns the step's local result ({ok}|{pending}), or throws with the reason it can't be done.
async function queueStep(step, opts = {}) {
  const now = new Date();
  const rec = { id: newId(), seq: ++OB.seq, at: now.toISOString(), date: opts.date || logDate(), time: opts.time !== undefined ? opts.time : logTime(),
    step, state: "waiting", blob: !!opts.blob, label: opts.label || "" };
  OB.steps.push(rec);
  const results = replicaProject();
  const mine = results[results.length - 1] || {};
  if (mine.error) {
    // the PC would refuse it too: don't queue it
    OB.steps.pop();
    OB.seq--;
    replicaProject();
    throw Object.assign(new Error(mine.error), { server: true });
  }
  if (opts.blob) await IDB.set(rec.id, opts.blob, "blobs");
  await outboxSave();
  drawState();
  outboxFlush();
  return mine;
}

/// Queues a step and shows what happened; false if it couldn't be done.
async function doStep(step, opts = {}) {
  try {
    const r = await queueStep(step, opts);
    toast(r.pending || (S.offline ? `${r.ok} · saved on the phone, sent when your PC is on` : r.ok));
    if (ONOPEN[S.tab]) ONOPEN[S.tab]();
    rerender();
    return true;
  } catch (e) {
    toast(e.message, 4000);
    return false;
  }
}

/// Sends waiting steps (in order) and follows the ones the PC is working on.
async function outboxFlush() {
  if (OB.sending || !S.token) return;
  OB.sending = true;
  try {
    const waiting = OB.steps.filter((s) => s.state === "waiting");
    // photos and recordings go first, so they're there when their step runs
    for (const s of waiting.filter((x) => x.blob && !x.blobSent)) {
      const b = await IDB.get(s.id, "blobs");
      if (b) await api("/api/sync/blob?id=" + encodeURIComponent(s.id), { method: "POST", headers: { "Content-Type": b.type || "application/octet-stream" }, body: b, timeout: 180000 });
      s.blobSent = true;
      await outboxSave();
    }
    if (waiting.length) {
      const r = await post("/api/sync/push", { steps: waiting.map(({ id, seq, at, date, time, step }) => ({ id, seq, at, date, time, step })) });
      for (const s of waiting) if (r.accepted.includes(s.id)) s.state = "sent";
      for (const x of r.rejected || []) finish(OB.steps.find((s) => s.id === x.id), "failed", x.error);
      await outboxSave();
      S.offline = false;
    }
  } catch (e) {
    if (!e.server) S.offline = true;
  } finally {
    OB.sending = false;
  }
  if (OB.steps.some((s) => s.state === "sent")) outboxPoll();
  rerenderIfIdle();
}

function finish(rec, state, message) {
  if (!rec || rec.state === "done" || rec.state === "failed") return;
  rec.state = state;
  rec.message = message;
  OB.history.push({ id: rec.id, at: rec.at, label: rec.label || describeStep(rec.step), state, message });
  if (rec.blob) IDB.del(rec.id, "blobs").catch(() => {});
}

/// Asks the PC how the sent steps are doing, until they're all done.
async function outboxPoll() {
  if (OB.polling) return;
  OB.polling = true;
  try {
    while (OB.steps.some((s) => s.state === "sent")) {
      const sent = OB.steps.filter((s) => s.state === "sent");
      let st;
      try { st = await api("/api/sync/status?ids=" + sent.map((s) => s.id).join(",")); }
      catch (e) { break; }
      let finished = 0;
      for (const x of st.steps) {
        if (!["applied", "failed", "skipped"].includes(x.status)) continue;
        finish(sent.find((s) => s.id === x.id), x.status === "applied" ? "done" : "failed", x.message);
        finished++;
        if (x.status !== "applied") toast("Your PC couldn't do it: " + x.message, 5000);
      }
      OB.working = sent.length - finished;
      if (finished) {
        await outboxSave();
        // the PC's new copy includes them
        try { await replicaSync(true); } catch (_) {}
        outboxPurge();
        drawState();
        if (ONOPEN[S.tab]) ONOPEN[S.tab]();
        rerenderIfIdle();
      }
      if (OB.steps.some((s) => s.state === "sent")) await new Promise((r) => setTimeout(r, 2000));
    }
  } finally {
    OB.polling = false;
    OB.working = 0;
    rerenderIfIdle();
  }
}

/// Drops steps the PC's copy already includes (or that failed and the PC has finished with).
function outboxPurge() {
  const before = OB.steps.length;
  OB.steps = OB.steps.filter((s) => !(RP.finished.has(s.id) && (s.state === "done" || s.state === "failed")));
  if (OB.steps.length !== before) { outboxSave(); replicaProject(); }
}

/// Steps the phone made that the PC hasn't carried out yet.
const outboxWaiting = () => OB.steps.filter((s) => s.state === "waiting" || s.state === "sent");

/// Ids of things changed by steps the PC doesn't have yet (shown with ⏳).
function pendingIds() {
  const ids = new Set();
  for (const s of OB.steps) {
    if (s.state === "failed" || RP.finished.has(s.id)) continue;
    const st = s.step;
    const id = st.id || (st.item && st.item.id) || (st.entry && st.entry.id) || (st.food && st.food.id) || (st.meal && st.meal.id) || (st.session && st.session.id) || st.food_id;
    if (id) ids.add(id);
  }
  return ids;
}
const pend = (id) => (pendingIds().has(id) ? ` <span title="${PENDING_TEXT}">⏳</span>` : "");

/// What a step does, in a few words (for the waiting list).
function describeStep(st) {
  switch (st.type) {
    case "task_add": return `Add task "${st.item.title}"`;
    case "task_update": return `Edit task "${st.title}"`;
    case "task_toggle": case "task_done": return "Tick off a task";
    case "task_delete": return "Delete a task";
    case "calories_add": return `+${st.entry.kcal} kcal${st.entry.description ? " · " + st.entry.description : ""}`;
    case "food_log": return "Log a food";
    case "calories_delete": return "Remove a calorie entry";
    case "weight_set": return st.kg ? `Weight ${Number(st.kg).toFixed(1)} kg` : "Remove a weight";
    case "goal_set": return `Goal ${st.kg} kg`;
    case "journal_add": return `Journal: "${st.entry.text.slice(0, 40)}"`;
    case "journal_audio": return "Voice journal entry";
    case "journal_update": return "Edit a journal entry";
    case "journal_delete": return "Delete a journal entry";
    case "food_save": return `Save food "${st.food.name}"`;
    case "meal_save": return `Save meal "${st.meal.name}"`;
    case "food_delete": return "Delete a food";
    case "workout_session": return `Workout: ${st.session.workout}`;
    case "claude": return `✦ Claude: "${st.text}"`;
    case "label": return "📷 Read a nutrition label";
    case "reflect": return "✦ Reflect on my week";
    default: return st.type;
  }
}

/// Removes a step that hasn't been sent yet (and later steps about the same new thing).
async function outboxRemove(id) {
  const rec = OB.steps.find((s) => s.id === id && s.state === "waiting");
  if (!rec) return toast("That's already with your PC");
  const made = rec.step.item ? rec.step.item.id : rec.step.entry ? rec.step.entry.id : null;
  OB.steps = OB.steps.filter((s) => s !== rec && !(made && s.state === "waiting" && s.step.id === made));
  if (rec.blob) IDB.del(rec.id, "blobs").catch(() => {});
  await outboxSave();
  replicaProject();
  drawState();
  if (ONOPEN[S.tab]) ONOPEN[S.tab]();
  rerender();
}

// ---------------------------------------------------------------- More › Waiting for PC

function renderOutbox(st) {
  const waiting = outboxWaiting();
  const row = (s) => `<div class="item"><div class="grow"><div>${esc(s.label || describeStep(s.step))}</div>
    <div class="muted">${esc(new Date(s.at).toLocaleString(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit" }))} · ${s.state === "sent" ? "with your PC" : "on the phone"}</div></div>
    ${s.state === "waiting" ? `<button class="small" data-act="obremove" data-id="${esc(s.id)}">Remove</button>` : ""}</div>`;
  const old = waiting.length && Date.now() - new Date(waiting[0].at).getTime() > 7 * 86400000;
  const hist = OB.history.slice().reverse().slice(0, 30).map((h) => `<div class="item"><span>${h.state === "done" ? "✓" : "✗"}</span><div class="grow">
    <div>${esc(h.label)}</div><div class="muted" style="${h.state === "done" ? "" : "color:var(--danger)"}">${esc(h.message || "")}</div></div></div>`).join("");
  return headerHtml() + `<div class="row" style="margin-bottom:10px"><button class="small" data-act="tab" data-tab="more">‹ More</button><span class="grow"></span>
      ${waiting.length && !S.offline ? `<button class="small" data-act="obsend">Send now</button>` : ""}</div>
    <div class="card list"><h3>Waiting for your PC (${waiting.length})</h3>
      ${old ? `<div class="error">Some of these have waited over a week. Open the app while your PC is on so they're not lost.</div>` : ""}
      ${waiting.map(row).join("") || `<p class="muted">Nothing waiting. What you do while your PC is off waits here and is sent, in order, when it's on.</p>`}</div>
    ${hist ? `<div class="card list"><h3>Done by your PC</h3>${hist}</div>` : ""}`;
}
SCREENS.outbox = renderOutbox;

Object.assign(ACTIONS, {
  obremove(el) { return outboxRemove(el.dataset.id); },
  obsend() { outboxFlush(); },
});
