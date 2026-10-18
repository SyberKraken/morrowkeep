# Workouts tab: plan and progress

Home workouts with dumbbells, bodyweight and a flat (non-reclining) bench. Desktop only for now.
Tick the boxes as steps are finished, so work can resume from here.

## Decisions (from the user)

- **Weights**: kg per dumbbell. Two 15 kg dumbbells = 15; one 15 kg dumbbell = 15.
- **Timing**: every step can have a time. A set with a time counts down (and moves on by
  itself); only a set with reps and **no** time waits for the **Done set** button.
  Breaks and rests are always timed.
- **Beeps**: between every step (when moving on), plus the last 3 seconds of a countdown.
- **Muscle map**: small front/back outline with main muscles strong and helping muscles light,
  as in the mockup. The stored data must be plain, readable text (muscle names, equipment,
  steps) so exports and Claude/other tools know what each exercise does.
- **Desktop only** in this round; the phone player comes later.

## Data (all readable JSON in the data folder)

- `exercises.json`: `{version, exercises: [Exercise]}`. Exercise: `id`, `name`, `aliases`,
  `equipment` (["dumbbells"|"dumbbell"|"bench"|"bodyweight"|"mat"]), `muscles_main`,
  `muscles_helping` (names from a fixed list: chest, upper back, lats, lower back, shoulders,
  rear delts, biceps, triceps, forearms, core, obliques, glutes, quads, hamstrings, calves,
  hip flexors, full body), `level` (beginner/intermediate/advanced), `kind`
  ("reps"|"time"|"reps_per_side"), `steps` [text], `form` [text], `mistakes` [text],
  `easier`, `harder`, `notes`, `builtin` (bool), `edited` (bool).
  Built-ins are added/updated on start unless the user edited them.
- `workouts.json`: `{version, workouts: [Workout]}`. Workout: `id`, `name`, `notes`,
  `items`: [Item]. Item = `{type: "exercise", exercise: id, sets, reps?, seconds?, kg?,
  rest_s}` or `{type: "break", seconds, label?}`. Duration estimate: timed sets use seconds;
  rep sets estimate 3 s/rep.
- `workout_log.json`: `{version, sessions: [Session]}`. Session: `id`, `date`, `start`,
  `duration_s`, `workout` (name + id), `sets`: [{exercise, set, reps?, seconds?, kg?}],
  `completed` (bool).
- Daily backups include all three. Theme colors: `data.workout`, `data.rest`.
- Claude context: the exercise library (names, equipment, muscles) so it can build workouts.

## Steps

- [x] **1. Exercise library** (done 2026-10-18: 47 built-ins in src/workouts_builtin.json, src/workouts.rs, src/ui/workouts.rs): `src/workouts.rs` (model, ~45 built-ins, load/save, filters,
  tests), Workouts tab with Exercises / Workouts / History switch, list with search +
  equipment and muscle filters, info page (steps, form, mistakes, equipment, muscles,
  level, easier/harder, muscle map), editor and "+ New exercise".
- [x] **2. Workouts** (done 2026-10-18: Item/Workout model + 5 starters in src/workouts_starter.json, builder in src/ui/workouts.rs; Start button is a stub until step 3): builder (exercise items with sets/reps/seconds/kg/rest, break items,
  reorder up/down or drag, remove), duration + equipment summary, starter workouts
  (full body, upper, lower, core, 10-min quick).
- [ ] **3. Player**: full-window view, big circle timer, timed steps auto-advance, rep-only
  sets wait for "Done set", rests between sets, beeps (moving on + last 3 s), pause /
  back / skip, adjust reps/kg of the set just done, next-up preview, summary at the end.
- [ ] **4. History and progress**: save sessions, history list, per-exercise last/best on the
  info page, progress chart, link a workout to a task (tick it off when finished).
- [ ] **5. Claude**: "Fill in with Claude" for a new exercise; "Build with Claude" for a
  workout (uses only library exercises, reviewed before saving); library in Claude context;
  import format docs.
- [ ] 6. Later (not now): phone player.
