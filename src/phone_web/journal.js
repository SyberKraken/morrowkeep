// Journal browsing on the phone, like the PC's Journal tab: every day (newest first, with
// "Show older days"), search, Pinned / Voice filters and your tags, On this day, editing,
// pinning and deleting entries, and Claude's "Reflect on my week".
"use strict";

S.jr = { data: null, q: "", pinned: false, voice: false, open: null, edit: null, confirmDelete: false, reflect: null };

function jrQuery(before) {
  const J = S.jr, p = new URLSearchParams();
  if (J.q.trim()) p.set("q", J.q.trim());
  if (J.pinned) p.set("pinned", "1");
  if (J.voice) p.set("voice", "1");
  if (before) p.set("before", before);
  return "/api/journal?" + p;
}

async function jrLoad() {
  try {
    S.jr.data = await api(jrQuery());
  } catch (e) {
    if (e.server) toast(e.message, 4000);
  }
  if (S.tab === "journal") rerenderIfIdle();
}
ONOPEN.journal = jrLoad;

// keep the list current after anything that changes the journal (saving, recordings, …)
const refreshBeforeJournal = refresh;
refresh = async function () {
  await refreshBeforeJournal();
  if (S.tab === "journal" && !S.jr.open) jrLoad();
};

const jrDayLabel = (iso) => iso === localDate() ? "Today" : iso === yesterday() ? "Yesterday"
  : new Date(iso + "T12:00").toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "short", year: iso.slice(0, 4) === localDate().slice(0, 4) ? undefined : "numeric" });

function jrEntryHtml(e) {
  const J = S.jr;
  const meta = `${esc(e.time || "–")}${e.mood ? ` · <span style="color:${moodColor(e.mood)}">${esc(e.mood_label)}</span>` : ""}${e.audio_secs ? ` · 🎤 ${mmss(e.audio_secs)}` : ""}${e.pinned ? " · 📌" : ""}${e.kind === "reflection" ? ` · <span style="color:var(--claude)">reflection</span>` : ""}`;
  const body = `${e.text ? `<div>${esc(e.text)}</div>` : ""}${e.transcribing ? `<div class="muted"><i>Transcribing on your PC…</i></div>` : e.transcript ? `<div style="font-style:italic">${esc(e.transcript)}</div>` : ""}`;
  if (J.open !== e.id) return `<div class="entry" data-act="jropen" data-id="${esc(e.id)}"><div class="meta">${meta}</div>${body}</div>`;
  const E = J.edit;
  return `<div class="entry"><div class="meta">${meta}</div><div class="sheet">
    <textarea id="jre_text" placeholder="Text">${esc(E.text)}</textarea>
    ${e.audio_secs ? `<textarea id="jre_transcript" placeholder="Transcript">${esc(E.transcript)}</textarea>` : ""}
    ${moodsHtml(E.mood, "jremood")}
    <div class="row"><button class="primary grow" data-act="jrsave">Save</button><button data-act="jrpin">${E.pinned ? "📌 Unpin" : "📌 Pin"}</button>
      ${J.confirmDelete ? `<button class="rec" data-act="jrdelete">Yes, delete</button>` : `<button data-act="jraskdelete">🗑</button>`}</div>
    <button class="small" data-act="jrclose">Close</button></div></div>`;
}

function jrDaysHtml(days) {
  return days.map((d) => `<div class="daylabel">${esc(jrDayLabel(d.date))}</div><div class="card">${d.entries.map(jrEntryHtml).join("")}</div>`).join("");
}

/// Everything under the composer on the Journal tab.
function journalBrowseHtml(st, fallback) {
  const J = S.jr, d = J.data;
  const filtered = J.q.trim() || J.pinned || J.voice;
  const chip = (on, act, label, extra = "") => `<button class="chip" data-act="${act}" ${extra} style="${on ? "border-color:var(--accent)" : ""}">${label}</button>`;
  const R = J.reflect;
  const reflect = st.claude ? `<div class="card"><div class="row"><div class="grow"><b>✨ Reflect on my week</b>
      <div class="muted">Claude reads your last 7 days and adds a short reflection. The only time journal text goes to Claude.</div></div>
      <button data-act="jrreflect" ${R && !R.done ? "disabled" : ""}>${R && !R.done ? "Thinking…" : "Reflect"}</button></div>
      ${R && R.error ? `<div class="error">${esc(R.error)}</div>` : ""}</div>` : "";
  const search = `<div class="card"><input id="jr_q" placeholder="Search ${d ? d.total : ""} entries…" value="${esc(J.q)}" autocomplete="off">
    <div class="chips" style="margin-top:8px">${chip(J.pinned, "jrpinned", "📌 Pinned")}${chip(J.voice, "jrvoice", "🎤 Voice")}
      ${(d ? d.tags : []).map((t) => chip(J.q.trim() === "#" + t, "jrtag", "#" + esc(t), `data-t="${esc(t)}"`)).join("")}</div></div>`;
  if (!d) return reflect + search + (fallback || "");
  const onThisDay = !filtered && d.on_this_day.length ? `<div class="card"><h3>On this day</h3>${d.on_this_day.map((x) =>
    `<div class="muted" style="margin-top:6px">${esc(x.label)} · ${esc(jrDayLabel(x.date))}</div>${x.entries.map(jrEntryHtml).join("")}`).join("")}</div>` : "";
  const list = d.days.length ? jrDaysHtml(d.days) : `<p class="muted center">${filtered ? "Nothing matches." : "No journal entries yet."}</p>`;
  return reflect + search + onThisDay + list + (d.more ? `<button class="block" data-act="jrmore" style="margin-bottom:12px">Show older days</button>` : "");
}

async function jrPost(path, body) {
  try {
    const r = await post(path, body);
    toast(r.message || "Saved");
    Object.assign(S.jr, { open: null, edit: null, confirmDelete: false });
    await jrLoad();
  } catch (e) {
    toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
  }
  rerender();
}

function jrFind(id) {
  const d = S.jr.data;
  return d && [...d.days, ...d.on_this_day].flatMap((x) => x.entries).find((e) => e.id === id);
}

async function jrPollReflect(job) {
  try {
    const r = await api("/api/journal/reflect?job=" + encodeURIComponent(job));
    if (!r.done) return void setTimeout(() => jrPollReflect(job), 2000);
    S.jr.reflect = { done: true, error: r.error || "" };
    if (!r.error) { toast("Claude's reflection is in today's journal"); S.jr.reflect = null; await jrLoad(); }
  } catch (e) {
    S.jr.reflect = { done: true, error: e.message };
  }
  rerender();
}

let jrSearchTimer = null;
INPUTS.jr_q = (t) => {
  S.jr.q = t.value;
  clearTimeout(jrSearchTimer);
  jrSearchTimer = setTimeout(async () => { await jrLoad(); keepTyping($("#jr_q") || t, "#jr_q"); }, 300);
};
INPUTS.jre_ = (t) => { S.jr.edit[t.id.slice(4)] = t.value; };

Object.assign(ACTIONS, {
  jropen(el) {
    const e = jrFind(el.dataset.id);
    if (!e) return;
    Object.assign(S.jr, { open: e.id, confirmDelete: false, edit: { text: e.text, transcript: e.transcript || "", mood: e.mood, pinned: e.pinned } });
    rerender();
  },
  jrclose() { Object.assign(S.jr, { open: null, edit: null }); rerender(); },
  jremood(el) { const m = +el.dataset.m; S.jr.edit.mood = S.jr.edit.mood === m ? null : m; rerender(); },
  jrpin() { S.jr.edit.pinned = !S.jr.edit.pinned; rerender(); },
  jrsave() {
    const E = S.jr.edit, e = jrFind(S.jr.open);
    const body = { id: S.jr.open, text: E.text, mood: E.mood || 0, pinned: E.pinned };
    if (e && e.audio_secs) body.transcript = E.transcript;
    return jrPost("/api/journal/update", body);
  },
  jraskdelete() { S.jr.confirmDelete = true; rerender(); },
  jrdelete() { return jrPost("/api/journal/delete", { id: S.jr.open }); },
  jrpinned() { S.jr.pinned = !S.jr.pinned; jrLoad(); },
  jrvoice() { S.jr.voice = !S.jr.voice; jrLoad(); },
  jrtag(el) { const t = "#" + el.dataset.t; S.jr.q = S.jr.q.trim() === t ? "" : t; jrLoad(); },
  async jrmore() {
    const d = S.jr.data, last = d.days[d.days.length - 1].date;
    try {
      const r = await api(jrQuery(last));
      d.days.push(...r.days);
      d.more = r.more;
    } catch (e) {
      toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
    }
    rerender();
  },
  async jrreflect() {
    S.jr.reflect = { done: false };
    rerender();
    try {
      const r = await post("/api/journal/reflect", {});
      jrPollReflect(r.job);
    } catch (e) {
      S.jr.reflect = { done: true, error: e.server ? e.message : "Your PC isn't reachable right now." };
      rerender();
    }
  },
});
