// Plan: overdue and undated tasks, then the next two weeks. Tap a task to tick it off for that
// day, or to edit it (title, date, time, category, notes) or delete it.
"use strict";

S.plan = { data: null, open: null, edit: null, confirmDelete: false, loading: false };

async function planLoad() {
  S.plan.loading = true;
  try {
    S.plan.data = await api("/api/agenda?days=14");
  } catch (e) {
    if (e.server) toast(e.message, 4000);
  }
  S.plan.loading = false;
  if (S.tab === "plan") rerenderIfIdle();
}
ONOPEN.plan = planLoad;

const shortDate = (iso) => new Date(iso + "T12:00").toLocaleDateString(undefined, { day: "numeric", month: "short" });

function planItemHtml(x, overdue) {
  const P = S.plan, key = `${x.id}|${x.day}`, open = P.open === key;
  const row = `<div class="task" data-act="planopen" data-key="${esc(key)}">
    <span class="dot" data-act="plantoggle" data-id="${esc(x.id)}" data-day="${esc(x.day)}" style="border-color:${x.color};background:${x.done ? x.color : "transparent"}">${x.done ? "✓" : ""}</span>
    <span style="flex:1;${x.done ? "color:var(--muted);text-decoration:line-through" : ""}">${esc(x.icon)} ${esc(x.title)}</span>
    <span class="muted">${x.repeat ? "↻ " : ""}${overdue && x.date ? esc(shortDate(x.date)) + " " : ""}${esc(x.time || "")}</span></div>`;
  return row + (open ? planSheetHtml(x) : "");
}

function planSheetHtml(x) {
  const P = S.plan, E = P.edit;
  const cats = S.state.categories || [];
  return `<div class="sheet">
    <input id="pe_title" value="${esc(E.title)}" placeholder="Title">
    <div class="row"><input id="pe_date" type="date" value="${esc(E.date || "")}" ${E.date ? "" : "disabled"} class="grow">
      <label class="row muted" style="gap:4px"><input type="checkbox" id="pe_nodate" ${E.date ? "" : "checked"} style="width:auto">no date</label></div>
    <div class="row"><input id="pe_time" type="time" value="${esc(E.time || "")}" class="grow"><button class="small" data-act="planclear">anytime</button></div>
    <select id="pe_kind">${cats.map((c) => `<option value="${esc(c.name)}" ${c.name === E.kind ? "selected" : ""}>${esc(c.icon)} ${esc(c.name)}</option>`).join("")}
      ${cats.some((c) => c.name === E.kind) ? "" : `<option value="${esc(E.kind)}" selected>${esc(E.kind)}</option>`}</select>
    <textarea id="pe_notes" style="min-height:50px" placeholder="Notes">${esc(E.notes)}</textarea>
    ${x.repeat ? `<p class="muted" style="margin:0">↻ ${esc(x.repeat)}. Changing the date moves the whole series; change repeats on the PC.</p>` : ""}
    <div class="row"><button class="primary grow" data-act="plansave" data-id="${esc(x.id)}">Save</button>
      ${P.confirmDelete ? `<button class="rec" data-act="plandelete" data-id="${esc(x.id)}">Yes, delete</button><button data-act="plannodelete">No</button>`
        : `<button data-act="planaskdelete">🗑</button>`}</div></div>`;
}

function renderPlan(st) {
  const P = S.plan, d = P.data;
  let body;
  if (!d) body = `<p class="muted center">${P.loading ? "Loading your plan…" : "Couldn't load your plan."}</p>`;
  else {
    const section = (title, items, style = "", overdue = false) => items.length
      ? `<div class="daylabel" style="${style}">${title}</div><div class="card">${items.map((x) => planItemHtml(x, overdue)).join("")}</div>` : "";
    body = section("OVERDUE", d.overdue, "color:var(--warning)", true)
      + d.days.map((day) => `<div class="daylabel">${esc(day.label)}</div><div class="card">${day.items.length ? day.items.map((x) => planItemHtml(x)).join("")
        : `<p class="muted" style="margin:4px 0">Nothing planned.</p>`}</div>`).join("")
      + section("BACKLOG (no date)", d.backlog);
  }
  return headerHtml() + entryCardHtml(st) + body;
}
SCREENS.plan = renderPlan;

function planFind(key) {
  const d = S.plan.data;
  if (!d) return null;
  const all = [...d.overdue, ...d.backlog, ...d.days.flatMap((x) => x.items)];
  return all.find((x) => `${x.id}|${x.day}` === key);
}

async function planChanged(msg) {
  if (msg) toast(msg);
  S.plan.open = null;
  S.plan.confirmDelete = false;
  await Promise.all([planLoad(), refresh()]);
  rerender();
}

Object.assign(ACTIONS, {
  planopen(el) {
    const P = S.plan, key = el.dataset.key;
    if (P.open === key) { P.open = null; return rerender(); }
    const x = planFind(key);
    if (!x) return;
    P.open = key;
    P.confirmDelete = false;
    P.edit = { title: x.title, date: x.date, time: x.time, kind: x.kind, notes: x.notes || "", lastDate: x.date || x.day };
    rerender();
  },
  async plantoggle(el) {
    const x = planFind(`${el.dataset.id}|${el.dataset.day}`);
    if (x) { x.done = !x.done; rerender(); }
    try {
      const r = await post("/api/task/toggle", { id: el.dataset.id, date: el.dataset.day });
      toast(r.message);
    } catch (e) {
      toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
    }
    planLoad();
    refresh();
  },
  planclear() { S.plan.edit.time = null; rerender(); },
  planaskdelete() { S.plan.confirmDelete = true; rerender(); },
  plannodelete() { S.plan.confirmDelete = false; rerender(); },
  async plansave(el) {
    const E = S.plan.edit;
    if (!E.title.trim()) return toast("The title can't be empty");
    try {
      const r = await post("/api/task/update", { id: el.dataset.id, title: E.title, date: E.date || null, time: E.time || null, kind: E.kind, notes: E.notes });
      await planChanged(r.message);
    } catch (e) {
      toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
    }
  },
  async plandelete(el) {
    try {
      const r = await post("/api/task/delete", { id: el.dataset.id });
      await planChanged(r.message);
    } catch (e) {
      toast(e.server ? e.message : "Your PC isn't reachable right now.", 4000);
    }
  },
});

INPUTS.pe_ = (t) => {
  const E = S.plan.edit;
  if (!E) return;
  const k = t.id.slice(3);
  if (k === "nodate") {
    E.date = t.checked ? null : E.lastDate;
    return rerender();
  }
  E[k] = t.value;
  if (k === "date" && t.value) E.lastDate = t.value;
};
