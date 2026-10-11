# Morrowkeep import format

Morrowkeep is a personal planner for tasks, chores, meals and workouts. You can add,
update or delete items by giving it JSON in this format. The user either pastes it into
**Import** in the app, or saves it as a `.json` file in the Morrowkeep `inbox` folder, which
is picked up automatically within a few seconds.

Reply with **one JSON object only**: no comments, no trailing commas. A markdown
code fence around it is fine.

```json
{
  "morrowkeep_import": 1,
  "items": [
    {
      "title": "Leg day",
      "kind": "workout",
      "date": "2026-10-12",
      "time": "18:00",
      "duration_min": 60,
      "repeat": "weekly mon,thu",
      "until": "2026-12-31",
      "notes": "Squats 4x8, RDL 3x10, lunges 3x12",
      "tags": ["gym"]
    },
    { "title": "Vacuum living room", "kind": "chore", "date": "2026-10-11", "repeat": "every 2 weeks" },
    { "title": "Meal prep: chicken, rice, broccoli", "kind": "meal", "date": "2026-10-11", "time": "16:00", "duration_min": 90 },
    { "title": "Buy running shoes", "kind": "task" }
  ],
  "delete": ["<id of an existing item>"]
}
```

A bare JSON array of items is also accepted. An optional top-level `"message"` string is shown
to the user, e.g. to mention an assumption you made.

## Item fields

| field | required | format / meaning |
|---|---|---|
| `title` | yes (new items) | Short text. |
| `kind` | no | One of the categories listed in the context (default `task`). Other lowercase words are allowed too. |
| `date` | no | `YYYY-MM-DD`. Also accepted: `today`, `tomorrow`, `monday`, `next friday`, `+3d`, `+2w`. Leave it out for a backlog item with no date. For repeating items this is the first day. |
| `time` | no | Start time `HH:MM`, 24-hour. Leave it out for "any time that day". |
| `duration_min` | no | Minutes, as a number. Default 30 when there is a time. |
| `end` | no | `HH:MM` end time, as an alternative to `duration_min`. |
| `repeat` | no | Plain-text rule: `daily`, `weekdays`, `weekends`, `weekly`, `weekly mon,wed,fri`, `every 2 weeks`, `every 2 weeks tue`, `every 3 days`, `monthly`, `every 3 months`, `yearly`. Weekly rules without days repeat on the weekday of `date`. Monthly rules repeat on the day-of-month of `date`. |
| `until` | no | `YYYY-MM-DD`, last day a repeating item can happen. |
| `notes` | no | Free text. Use `\n` for new lines (e.g. a recipe or an exercise list). |
| `tags` | no | Array of strings. |
| `done` | no | `true` / `false`. On a repeating item it marks one occurrence: today's, or the day in `done_date`. To tick off a task the user says they did, send its `id` with `"done": true`. |
| `done_date` | no | `YYYY-MM-DD`: which occurrence `done` applies to (repeating items). Default today. |
| `sync_google` | no | `false` to keep the item out of Google Calendar. Default `true`. |
| `id` | no | Only to **update** an existing item: copy its id from the context Morrowkeep gave you. Fields you leave out keep their current values. To clear a field, set it to `null`. |

## Logging calories

When the user tells you what they ate or drank, add entries to a top-level `"calories"` array
(next to or instead of `"items"`):

```json
{
  "morrowkeep_import": 1,
  "calories": [
    {
      "kcal": 780,
      "description": "Chicken burrito, large, assumed ~450 g (~690); 1 tbsp sour cream (~30); assumed oil in filling (~60)",
      "date": "today",
      "time": "12:30"
    }
  ],
  "message": "No size given, so I assumed a large restaurant burrito."
}
```

| field | required | format / meaning |
|---|---|---|
| `kcal` | yes | Whole number of kilocalories for this entry. |
| `description` | yes | The food, the amount you assumed, and the estimate for each part, so the user can check your reasoning. |
| `date` | no | `YYYY-MM-DD`, `today` or `yesterday`. Default today. |
| `time` | no | `HH:MM` when it was eaten, if the user said so (e.g. "for breakfast" ≈ 08:00). Default: now, for today's entries. |

**Estimate high. The user would rather overcount than undercount.**
- With gram or volume amounts, use them, take the higher end of typical values, and round each part **up** to the nearest 10.
- Without amounts, assume a **generous** portion (a restaurant or large home serving), not a small or "standard" one.
- Include hidden calories: cooking oil or butter, sauces, dressings, cheese, sugar or milk in drinks. Assume they're there unless the user says otherwise.
- When unsure between two estimates, pick the higher one.
- Use one entry per meal or snack, with the breakdown in `description`. Use separate entries for separately eaten meals.
- Use `"message"` to mention a big assumption (e.g. the portion size you assumed).
- To fix a wrong entry, delete it by its id (from today's calorie log in the context) and add a corrected one.

### Foods from the user's library

If the context lists a **food library** and the user names one of those foods ("1 serving of
kvarg", "200 g chili", "two jars of quark"), don't estimate: log it by name and amount and
Morrowkeep computes the calories from its saved values:

```json
{ "morrowkeep_import": 1, "calories": [ { "food": "Kvarg", "servings": 1 }, { "food": "Chili", "grams": 300 } ] }
```

| field | meaning |
|---|---|
| `food` | Name (or alias) of a food or meal in the library. |
| `servings` | Number of servings (default 1). "a jar" when the serving is a jar = 1. |
| `grams` | Instead of `servings`, an amount in grams. |

`date` and `time` work as above. For anything not in the library, estimate as usual with
`kcal` and `description`.

### Saving foods to the library

When the user asks to save a food ("save kvarg: 62 kcal per 100 g, a jar is 250 g"), add a
top-level `"foods"` array. A food with the same name is updated (fields you leave out keep
their values).

```json
{ "morrowkeep_import": 1, "foods": [ { "name": "Kvarg", "kcal_per_100g": 62, "serving_g": 250, "serving_name": "jar", "protein_per_100g": 11, "carbs_per_100g": 4, "fat_per_100g": 0.2, "aliases": ["quark"] } ] }
```

`name` is required, and `kcal_per_100g` too for a new food. Use the numbers the user gives;
only estimate (on the high side) if they ask you to.

## Logging weight

When the user tells you their weight, add a top-level `"weight"` array. There's one value per
day, and a new value replaces that day's old one.

```json
{ "morrowkeep_import": 1, "weight": [ { "kg": 82.4, "date": "today" } ] }
```

`kg` is required (a number, in kilograms). Convert from pounds if needed (1 lb = 0.4536 kg).
`date` defaults to today.

## Journal

When the user asks you to write something in their journal, add a top-level `"journal"` array:

```json
{ "morrowkeep_import": 1, "journal": [ { "text": "Long walk by the lake, felt calm. #walk", "mood": 4 } ] }
```

`text` is required. `date` (`YYYY-MM-DD`, default today), `time` (`HH:MM`, default now) and
`mood` (1 = rough … 5 = great) are optional. Write it in the user's own words and voice.

## Changing settings

When the user asks to change how Morrowkeep looks or behaves (colors, categories, reminders,
theme), add a top-level `"settings"` object. The context lists the current values; change
only what was asked and leave everything else out.

```json
{
  "morrowkeep_import": 1,
  "settings": {
    "colors": { "calories": "#FF9A50" },
    "categories": [
      { "name": "meal", "color": "#FFB347" },
      { "name": "study", "icon": "📚", "color": "#5B8CFF", "style": "vine" },
      { "name": "chore", "rename": "chores" }
    ],
    "delete_categories": ["event"],
    "quiet_start": "23:00"
  },
  "message": "Made the calorie orange about 15% lighter."
}
```

| key | meaning |
|---|---|
| `colors` | `calories` and/or `weight`: `"#RRGGBB"`, or `null` to go back to the theme's color. |
| `categories` | Task categories (the `kind` of items), matched by `name`. Fields: `icon` (one emoji), `color` (`"#RRGGBB"` or `null` for the theme's), `style` (`ribbon`, `comet`, `arcs`, `vine` or `constellation`: how streaks are drawn), `rename` (new name; tasks move along). A new name adds a category. |
| `delete_categories` | Names to delete; their tasks become `task`. `task` can't be deleted or renamed. |
| `theme`, `font` | One of the available themes / fonts listed in the context. |
| `default_input_mode` | `local` or `claude`. |
| `reminders_enabled`, `remind_anytime`, `remind_calories`, `remind_weight`, `remind_journal`, `close_to_tray`, `always_on_top`, `auto_sync` | `true` / `false`. |
| `reminder_interval_min`, `sync_interval_min` | Minutes, a whole number. |
| `calorie_reminder_after`, `weight_reminder_after`, `journal_reminder_after`, `quiet_start`, `quiet_end` | `HH:MM`. |

For relative requests ("a little brighter", "less saturated", "warmer"), start from the
current color in the context and make a modest change (about 10–20% in lightness or
saturation), keeping the hue unless asked otherwise. Colors should stay readable on the theme
background given in the context. Say what you changed in `"message"`.

## Deleting

Put existing item ids, or calorie entry ids, in the top-level `"delete"` array.

## Rules

- If any item is invalid, the whole import is rejected and Morrowkeep lists every error, so nothing is half-imported.
- Use the current date and existing items given in the context. Don't invent ids.
- Prefer one repeating item (`repeat`) over many copies of the same item.
- Keep titles short. Put details (sets/reps, ingredients, steps) in `notes`.
