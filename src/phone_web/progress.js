// Progress (under More): calories and weight over time, the Body numbers and habit streaks,
// like the PC's Charts and Body tabs. Tap a bar or a day on a chart to read its value.
"use strict";

S.prog = { data: null, days: +(localStorage.getItem("mk_progdays") || 30), sel: null };

async function progLoad() {
  try {
    S.prog.data = await api("/api/progress?days=" + S.prog.days);
  } catch (e) {
    if (e.server) toast(e.message, 4000);
  }
  if (S.tab === "progress") rerenderIfIdle();
}
ONOPEN.progress = progLoad;

const W = 340, H = 150, PAD = { l: 34, r: 6, t: 8, b: 18 };
const dayMs = 86400000;
const dateOf = (iso) => new Date(iso + "T12:00");
const shortD = (iso) => dateOf(iso).toLocaleDateString(undefined, { day: "numeric", month: "short" });

/// x position of a date in a chart that spans `from`..`to` (ISO dates).
function xScale(from, to) {
  const a = dateOf(from).getTime(), b = dateOf(to).getTime();
  return (iso) => PAD.l + (W - PAD.l - PAD.r) * ((dateOf(iso).getTime() - a) / Math.max(dayMs, b - a));
}

/// A round number at or above `v` for the top of a chart (1, 2, 2.5 or 5 × a power of 10).
function niceMax(v) {
  const p = Math.pow(10, Math.floor(Math.log10(Math.max(1, v))));
  return [1, 2, 2.5, 5, 10].map((k) => k * p).find((x) => x >= v);
}

function yScale(lo, hi) {
  return (v) => PAD.t + (H - PAD.t - PAD.b) * (1 - (v - lo) / Math.max(1e-9, hi - lo));
}

/// Recessive grid: 3 value lines with labels, and first/last date.
function axesSvg(lo, hi, y, fmt, from, to) {
  const lines = [0, 0.5, 1].map((k) => lo + (hi - lo) * k);
  return lines.map((v) => `<line x1="${PAD.l}" x2="${W - PAD.r}" y1="${y(v)}" y2="${y(v)}" stroke="var(--border)" stroke-width="1"/>
      <text x="${PAD.l - 4}" y="${y(v) + 3}" text-anchor="end" font-size="9" fill="var(--muted)">${fmt(v)}</text>`).join("")
    + `<text x="${PAD.l}" y="${H - 4}" font-size="9" fill="var(--muted)">${shortD(from)}</text>
       <text x="${W - PAD.r}" y="${H - 4}" text-anchor="end" font-size="9" fill="var(--muted)">${shortD(to)}</text>`;
}

function caloriesChart(d) {
  const days = d.calories, limit = d.limit || 0;
  const max = niceMax(Math.max(500, limit, ...days.map((x) => x.kcal)) * 1.05);
  const y = yScale(0, max);
  const n = days.length, slot = (W - PAD.l - PAD.r) / n, bw = Math.max(2, slot - 2);
  const sel = S.prog.sel?.chart === "cal" ? S.prog.sel.i : null;
  const bars = days.map((x, i) => {
    const x0 = PAD.l + i * slot + (slot - bw) / 2, top = y(x.kcal);
    const over = limit && x.kcal > limit;
    return (x.kcal ? `<rect x="${x0}" y="${top}" width="${bw}" height="${Math.max(0, y(0) - top)}" rx="${Math.min(2, bw / 2)}"
        fill="var(--calories)" opacity="${sel === null || sel === i ? 1 : 0.45}" ${over ? `stroke="var(--text)" stroke-width="1"` : ""}/>` : "")
      + `<rect x="${PAD.l + i * slot}" y="${PAD.t}" width="${slot}" height="${H - PAD.t - PAD.b}" fill="transparent" data-act="pgsel" data-chart="cal" data-i="${i}"/>`;
  }).join("");
  // the daily limit, as on the PC's chart
  const tgt = limit ? `M${PAD.l} ${y(limit)} L${W - PAD.r} ${y(limit)}` : "";
  const logged = days.filter((x) => x.kcal);
  const avg = logged.length ? Math.round(logged.reduce((a, x) => a + x.kcal, 0) / logged.length) : 0;
  const over = days.filter((x) => limit && x.kcal > limit).length;
  const read = sel !== null ? `${shortD(days[sel].date)}: <b>${days[sel].kcal} kcal</b>${limit ? ` of ${limit}` : ""}`
    : `average ${avg} kcal on ${logged.length} logged day${logged.length === 1 ? "" : "s"}${limit ? ` · ${over} over the limit` : ""}`;
  return `<div class="card"><h3>Calories</h3><div style="min-height:20px">${read}</div>
    <svg viewBox="0 0 ${W} ${H}" style="width:100%;height:auto;display:block">${axesSvg(0, max, y, (v) => Math.round(v), days[0].date, days[n - 1].date)}
      ${bars}<path d="${tgt}" fill="none" stroke="var(--text)" stroke-width="1.5" stroke-dasharray="4 3" opacity=".7" pointer-events="none"/></svg>
    <div class="muted"><span style="color:var(--calories)">▮</span> kcal per day${limit ? ` · ┄ daily limit ${limit} kcal (${d.limit_from_plan ? "from your Body plan" : "fallback limit"}) · outlined = over` : ""}</div></div>`;
}

function weightChart(d) {
  const pts = d.weights, tr = d.trend, g = d.goal;
  if (!pts.length) return `<div class="card"><h3>Weight</h3><p class="muted">No weigh-ins in this period.</p></div>`;
  const from = d.from, to = d.today;
  // the goal's ideal path, from when it was set to the target date, clipped to the period
  let path = null;
  if (g && g.start_kg > g.target_kg) {
    const a = dateOf(g.set_on).getTime(), b = dateOf(g.target_date).getTime();
    const at = (iso) => g.start_kg + (g.target_kg - g.start_kg) * Math.min(1, Math.max(0, (dateOf(iso).getTime() - a) / Math.max(dayMs, b - a)));
    const s = g.set_on > from ? g.set_on : from;
    path = [[s, at(s)], [to, at(to)]];
  }
  const vals = [...pts.map((p) => p.kg), ...tr.map((p) => p.kg), ...(path ? path.map((p) => p[1]) : [])];
  // whole-kg range with an even span, so all three grid lines land on whole kilograms
  const lo = Math.floor(Math.min(...vals) - 0.5);
  let hi = Math.ceil(Math.max(...vals) + 0.5);
  if ((hi - lo) % 2) hi += 1;
  const x = xScale(from, to), y = yScale(lo, hi);
  const sel = S.prog.sel?.chart === "kg" ? S.prog.sel.i : null;
  const line = (list) => list.map((p, i) => `${i ? "L" : "M"}${x(p.date).toFixed(1)} ${y(p.kg).toFixed(1)}`).join(" ");
  const dots = pts.map((p, i) => `<circle cx="${x(p.date)}" cy="${y(p.kg)}" r="${sel === i ? 5 : 3.5}" fill="var(--weight)" stroke="var(--card)" stroke-width="1.5"/>
      <circle cx="${x(p.date)}" cy="${y(p.kg)}" r="14" fill="transparent" data-act="pgsel" data-chart="kg" data-i="${i}"/>`).join("");
  const last = pts[pts.length - 1], trendNow = tr.length ? tr[tr.length - 1].kg : null;
  const read = sel !== null ? `${shortD(pts[sel].date)}: <b>${pts[sel].kg.toFixed(1)} kg</b>`
    : `latest <b>${last.kg.toFixed(1)} kg</b> (${shortD(last.date)})${trendNow ? ` · 7-day average ${trendNow.toFixed(1)}` : ""}${d.pace_kg_week != null ? ` · ${d.pace_kg_week > 0 ? "+" : ""}${d.pace_kg_week.toFixed(2)} kg/week` : ""}`;
  return `<div class="card"><h3>Weight</h3><div style="min-height:20px">${read}</div>
    <svg viewBox="0 0 ${W} ${H}" style="width:100%;height:auto;display:block">${axesSvg(lo, hi, y, (v) => v.toFixed(0), from, to)}
      ${path ? `<path d="M${x(path[0][0])} ${y(path[0][1])} L${x(path[1][0])} ${y(path[1][1])}" stroke="var(--goal)" stroke-width="1.5" stroke-dasharray="5 4" fill="none"/>` : ""}
      ${tr.length > 1 ? `<path d="${line(tr)}" stroke="var(--trend)" stroke-width="2" fill="none"/>` : ""}${dots}</svg>
    <div class="muted"><span style="color:var(--weight)">●</span> weigh-ins · <span style="color:var(--trend)">━</span> 7-day average${path ? ` · <span style="color:var(--goal)">┄</span> goal path` : ""}</div>
    ${g && g.status ? `<div style="margin-top:6px">${esc(g.status)} · target ${g.target_kg.toFixed(1)} kg by ${shortD(g.target_date)}</div>` : ""}</div>`;
}

function bodyCard(d) {
  const b = d.body;
  if (!b.complete) return `<div class="card"><h3>Body</h3><p>Daily limit: <b>${b.daily || "none"}</b>${b.daily ? " kcal (the fallback limit)" : ""}</p>
    <p class="muted">For your own numbers, add ${esc(b.missing.join(", "))} on the PC's Body tab.</p></div>`;
  const row = (k, v) => `<div class="row" style="padding:4px 0;border-top:1px solid var(--border)"><span class="grow muted">${k}</span><span>${v}</span></div>`;
  return `<div class="card"><h3>Body · ${b.weight_kg} kg (7-day average)</h3>
    <div class="big" style="font-size:26px">${b.daily} <span class="muted" style="font-size:14px">kcal a day ${b.losing ? "to reach your goal" : "to maintain"}</span></div>
    ${row("Resting burn (BMR)", `${b.bmr} kcal <span class="muted">${esc(b.formula)}</span>`)}
    ${row("Daily burn", `${b.tdee} kcal`)}
    ${b.losing ? row("Deficit", `${b.deficit} kcal <span class="muted">(${b.kg_per_week} kg/wk)</span>`) + row("Days left", b.days_left) : ""}
    ${row("BMI", `${b.bmi} <span class="muted">(healthy ${b.healthy[0]}–${b.healthy[1]} kg)</span>`)}
    ${b.too_fast ? `<div style="color:var(--warning);margin-top:6px">⚠ The goal date is faster than recommended; the safe date is ${shortD(b.safe_date)}.</div>` : ""}
    ${b.floored ? `<div style="color:var(--warning);margin-top:6px">⚠ Held at the minimum safe intake.</div>` : ""}</div>`;
}

function habitsCard(d) {
  if (!d.habits.length) return "";
  const cell = (c, color) => `<span style="display:inline-block;width:9px;height:9px;margin-right:2px;border-radius:50%;${c === 2 ? `background:${color}` : c === 1 ? `border:1.5px solid ${color};opacity:.6` : "background:var(--fill);opacity:.35"}"></span>`;
  return `<div class="card"><h3>Habits · last 3 weeks</h3>${d.habits.map((h) => `<div style="padding:7px 0;border-top:1px solid var(--border)">
      <div class="row"><span class="grow">${esc(h.icon)} ${esc(h.title)}</span><span class="muted">${h.streak ? `🔥 ${h.streak} · ` : ""}${h.done}/${h.due} done</span></div>
      <div style="margin-top:4px;white-space:nowrap;overflow:hidden">${h.cells.map((c) => cell(c, h.color)).join("")}</div></div>`).join("")}
    <div class="muted" style="margin-top:6px">● done · ○ missed · journal: 🔥 ${d.journal_streak} day${d.journal_streak === 1 ? "" : "s"} in a row</div></div>`;
}

function renderProgress(st) {
  const P = S.prog, d = P.data;
  const pick = [14, 30, 90].map((n) => `<button class="small grow ${P.days === n ? "primary" : ""}" data-act="pgdays" data-n="${n}">${n} days</button>`).join("");
  const top = `<div class="row" style="margin-bottom:10px"><button class="small" data-act="tab" data-tab="more">‹ More</button><span class="grow"></span></div>
    <div class="row" style="margin-bottom:12px">${pick}</div>`;
  if (!d) return headerHtml() + top + `<p class="muted center">Loading…</p>`;
  return headerHtml() + top + caloriesChart(d) + weightChart(d) + bodyCard(d) + habitsCard(d);
}
SCREENS.progress = renderProgress;

Object.assign(ACTIONS, {
  pgdays(el) { S.prog.days = +el.dataset.n; S.prog.sel = null; localStorage.setItem("mk_progdays", S.prog.days); progLoad(); rerender(); },
  pgsel(el) {
    const s = { chart: el.dataset.chart, i: +el.dataset.i };
    S.prog.sel = S.prog.sel && S.prog.sel.chart === s.chart && S.prog.sel.i === s.i ? null : s;
    rerender();
  },
});
