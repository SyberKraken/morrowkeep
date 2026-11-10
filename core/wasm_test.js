// Runs the phone's offline scenario against the real WebAssembly build:
//   cargo build -p morrowkeep-core --target wasm32-unknown-unknown --release && node core/wasm_test.js
const fs = require("fs"), path = require("path"), assert = require("assert");
const MK = require("../src/phone_web/core.js");
(async () => {
  await MK.load(fs.readFileSync(path.join(__dirname, "../target/wasm32-unknown-unknown/release/morrowkeep_core.wasm")));
  const today = "2026-11-10";
  let snap = { weights: [{ date: "2026-11-09", kg: 82.0 }], foods: { foods: [{ id: "oats", name: "Oats", kcal_per_100g: 370, serving_g: 40, serving_name: "bowl" }], meals: [] } };
  const queue = [];
  for (const text of ["call mum tomorrow 7pm", "200 kcal", "1 bowl oats", "82,1 kg"]) {
    const r = MK.call({ fn: "plan_entry", text, day: today, today, snapshot: snap });
    assert.ok(r.steps, JSON.stringify(r));
    queue.push(...r.steps);
  }
  assert.deepStrictEqual(MK.call({ fn: "plan_entry", text: "ate a burrito", day: today, today, snapshot: snap }), { claude: "a burrito" });
  const task = queue[0].item;
  assert.match(task.id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.strictEqual(task.date, "2026-11-11");
  assert.strictEqual(task.created.slice(0, 10), new Date().toISOString().slice(0, 10).slice(0, 4) + task.created.slice(4, 10));
  queue.push({ type: "task_toggle", id: task.id, day: "2026-11-11" });
  queue.push({ type: "label", food_id: "L1" }, MK.call({ fn: "food_log", food: "L1", servings: 1, date: today, time: null }));
  const p = MK.call({ fn: "project", snapshot: snap, steps: queue, today });
  const res = p.results.map((r) => Object.keys(r)[0]);
  assert.deepStrictEqual(res, ["ok", "ok", "ok", "ok", "ok", "pending", "ok"], JSON.stringify(p.results));
  snap = p.snapshot;
  assert.strictEqual(snap.calories.reduce((a, c) => a + c.kcal, 0), 200 + 148);
  assert.strictEqual(snap.items[0].done, true);
  assert.ok(Math.abs(snap.weights[snap.weights.length - 1].kg - 82.1) < 1e-4); // f32, shown to one decimal
  const lib = MK.call({ fn: "library", foods: snap.foods });
  assert.ok(lib.some((f) => f.waiting) && lib.some((f) => f.name === "Oats" && f.kcal_serving === 148));
  // two new ids in a row are different
  const a = MK.call({ fn: "food_log", food: "oats", date: today, time: null }), b = MK.call({ fn: "food_log", food: "oats", date: today, time: null });
  assert.notStrictEqual(a.id, b.id);
  console.log("wasm scenario ok:", p.results.map((r) => r.ok || r.pending).join(" | "));
})().catch((e) => { console.error(e); process.exit(1); });
