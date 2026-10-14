# Morrowkeep

A small, always-on Windows planner for tasks, chores, meals and workouts.

- **Local-first.** Everything is stored as readable JSON in `Documents\Morrowkeep`. You can edit
  `items.json` by hand; the app notices and reloads.
- **Habits page.**
  - All of today's tasks form a board of large tiles that scale with the window: scheduled
    ones first (their time shows on the tile), then the anytime ones. Above it, a
    constellation lights a star per finished task. Click a tile and it flips, fills with
    color and later moves to a "done" tray.
  - A right-hand panel holds the calorie and weight entry, today's date, and the backlog
    (undated items and overdue calendar entries, each with **Do today**).
  - Timed items live in the Calendar.
- **Calendar view.** Month grid and week timeline, with a side panel for the selected day.
  Double-click a day or a time slot to create an item there.
- **Quick add.** Type (or say) things like `leg day tomorrow 6pm for 1h every mon and thu #workout`.
  A live preview shows how it was understood. Parsing is done offline and is free.
- **Voice.** Uses local Whisper (whisper.cpp). Audio never leaves the PC.
- **Import from AI tools.** Paste JSON from ChatGPT, Claude, Gemini and so on, or drop `.json` files into the
  `inbox` folder. **Import → "Copy AI instructions + my current plan"** gives the AI the format
  and your current items. See [IMPORT_FORMAT.md](IMPORT_FORMAT.md).
- **Google Calendar mirror.** Dated items are pushed into a dedicated "Morrowkeep" calendar, and
  events from your own calendars are shown read-only.
- **Calorie tracker.** Today shows a big daily total with **+ Add**, quick **+100/+250/+500**
  buttons, and **Describe what you ate** for Claude. Claude deliberately estimates high and
  explains its estimate before you apply it. The calendar shows each day's total. Entries live in
  `calories.json`.
- **Body tab: profile and goal weight.** Enter sex, birth date, height, activity level and
  (optionally) body fat %. Morrowkeep works out:
  - your resting burn (Mifflin-St Jeor, or Katch-McArdle when body fat is known)
  - your daily burn (resting burn × activity factor), BMI and the healthy weight range for your
    height

  Set a **goal weight and date** and it works out the daily deficit (≈7700 kcal per kg) and the
  calories to eat. To help pick the date it offers **ASAP** (the largest recommended deficit:
  the smallest of 1000 kcal/day, 1% of body weight a week, and what keeps you above 1500 kcal,
  or 1200 for women), **Easy** (a 300 kcal/day deficit) and a slider between them that shows
  the weekly loss and calories for each date. A date faster than ASAP gets a warning. A target at
  or above your weight means maintenance for that weight. Calculations use your 7-day average
  weight, so daily water swings don't move the plan. Once you have 2+ weeks of calories and
  weigh-ins, **measured burn** shows what your own logs say you burn.
- **Daily calorie limit.** With a complete profile, the limit comes from the Body plan and
  updates as your weight changes. Otherwise a fallback limit (also on the Body tab) is used.
- **Monthly calorie budget.** The month view shows the
  month's total against limit × days in the month. Today doesn't show the raw limit. It shows a
  *recommended* amount: what's left of the month's budget spread over the remaining days, so a big
  day lowers the following days. A past day with nothing logged counts as exactly your daily limit
  ("no food entered", not "ate nothing").
- **Weight.** Enter today's weight on the Habits page (one value per day, stored in
  `weight.json`). It shows the change since your last weigh-in, and how far you are from your
  goal.
- **Streaks** (top of the Charts tab). One row per recurring task, with consecutive completions
  drawn as a streak in the style of the task's category (set in Settings > Categories):
  - ribbon: thickens as the streak grows (default for workout)
  - comet: the live streak glows, older days fade (meal)
  - arcs: hops from day to day (chore)
  - vine: sprouts leaves at milestones (habit)
  - constellation: a 4-point star every 7th day (everything else)

  Missed days are hollow rings, and today pulses until done. One-off tasks appear as small stars
  on the day they were finished.
- **Calendar as a star map.** Each month cell is that day's constellation: done tasks lit and
  joined, missed ones as grey rings, upcoming ones as faint rings, and a gold star when everything
  was done. Only tasks with a time are listed as text. The day panel shows the day's anytime
  tasks as a vertical constellation (click a star to tick it off).
- **Charts tab.** A weight chart and a calories-per-day bar chart (with your daily limit as a
  dashed line), over Week / Month / 3 months / 6 months / Year. Hover for exact values. The
  weight chart shows weigh-ins, the 7-day average and, with a goal, the target line, a star on
  the target date, the planned path, and where your recent pace leads (dotted). When the
  target date is within one more period, the charts extend to it.
- **Reminders.** Windows notifications, also while Morrowkeep is in the tray: unfinished "anytime"
  items for today, no calories logged after 18:00, no weight logged after 10:00, and nothing in
  the journal after 21:00. At most one
  every 60 minutes, with quiet hours (22:00–08:00).
  - Calorie, weight and journal reminders have **input boxes**, so you can log right from the
    notification. Type a number for kcal, or a food description to have Morrowkeep open and
    estimate it with Claude.
  - Buttons: **Log**, **Open Morrowkeep** and **Snooze 2h**. Configure everything under
    Settings > Reminders.
  - Morrowkeep keeps its Start menu shortcut tagged with its notification ID. Windows needs that to
    show the notifications in the notification panel.
- **Foods tab: your food library.** Save ingredients with kcal per 100 g, a default serving
  ("1 jar = 250 g") and optional protein, carbs and fat, plus aliases ("quark" for kvarg).
  Build **meals** from them: ingredient weights, how many servings it makes, and optionally the
  cooked weight, so "300 g chili" is exact. Per-serving and per-100 g numbers update as you edit.
  **✨ Estimate with Claude** fills in a new ingredient from its name. Then log by amount anywhere:
  - on the food's page (servings or grams);
  - in the entry bar: `1 serving of kvarg`, `2 jars of kvarg`, `200 g chili`, `had half a portion
    of chili`, `kvarg x2`;
  - in Claude mode ("add 1 serving of kvarg to my calorie count"): Claude refers to the
    library and Morrowkeep computes the calories, or saves new foods for you;
  - with the **Foods** picker on the Habits calorie card (favorites and recent foods, one click
    per serving).

  Entries from the library keep their grams and macros, and the calorie card shows today's
  protein, carbs and fat.
- **Journal tab.** Write entries or record voice notes:
  - **Voice entries:** the recording is kept as a WAV file (`journal-audio\`). Whisper transcribes
    it on this PC in the background, and the transcript shows under it, editable. **▶ Play** opens
    the recording in your audio player.
  - **Each entry** can have a mood (rough to great) and #tags. It can be pinned, edited or
    deleted; a deleted recording is moved to `journal-audio\deleted\`, not erased.
  - **Browsing:** days are listed newest first, with "Show older days" to page back. A mini
    calendar marks the days you wrote and jumps to any of them.
  - **Search** covers every entry and transcript, with Pinned/Voice filters and your most-used
    tags as one-click searches.
  - **Writing prompts**, a "days in a row" streak, word counts, and **On this day** (what you
    wrote a month, 3 months and a year ago).
  - **✨ Reflect on my week:** sends the last 7 days to Claude and adds its short reflection as an
    entry. This is the only time journal text is sent to Claude.
  - **Export to Markdown** writes the whole journal to `journal-export.md`.
  - On the **Habits page**, a quick journal card (type or 🎤) adds an entry, and any entry today
    lights the *journal* star in the constellation. In the entry bar, `journal: …` or
    `dear diary …` saves an entry too. Typing anywhere on the Journal page starts an entry.
- **Settings tab.** A full page with sections on the left (General, Appearance, Categories,
  Input & voice, Reminders, Google Calendar). Changes are a draft until you press Save (or
  Ctrl+S); Revert throws them away, and the Settings button shows a dot while something is
  unsaved.
- **Categories.** Your task kinds, edited under Settings > Categories: name, icon, color (or the
  theme's) and streak style, with a live preview of the style. Renaming one moves its tasks
  along; deleting one turns its tasks into plain `task`s. Add your own (e.g. `study`) and use it
  with `#study`; kinds your tasks already use show up as one-click "add" buttons.
- **Look and feel.** A night-sky theme with the Inter font. Themes, fonts and your own calorie
  and weight colors are under Settings > Appearance (see [Themes](#themes)).
- **Small moments of delight.** A finished tile flips and throws a burst of sparkles, its star
  in the constellation ignites, totals count to their new value, and messages appear as short
  toasts. All of it can be slowed down, sped up or turned off in the theme (`motion`).
- **Lightweight.** A single 9 MB native exe with no browser engine. With the window open it uses
  about 80 MB of RAM and 0% CPU when idle. Closing the window tears down the window and GPU
  context, leaving only the tray and a background worker at about 5–12 MB. Measured on Windows 11
  with an AMD GPU.

## Build and run

```
cargo build --release
target\release\morrowkeep.exe
```

Building without voice support doesn't need CMake:

```
cargo build --release --no-default-features
```

The voice feature builds whisper.cpp with CMake and MSVC. `.cargo/config.toml` points at the
CMake bundled with the Visual Studio 2022 Build Tools. Adjust it if yours is elsewhere.
`vendor/whisper-rs-sys` is a patched copy of that crate whose bindings work on Windows without
LLVM (see the comment in `Cargo.toml`).

## Data folder

| file | what |
|---|---|
| `items.json` | All your items. This is the master copy. |
| `calories.json` | Calorie log, one entry per food or drink. Daily totals are summed from it. |
| `weight.json` | Weight log, one entry per day. |
| `journal.json` | Journal entries (text, mood, transcripts). |
| `foods.json` | Food library: ingredients and meals. |
| `journal-audio/` | Journal voice recordings (WAV). |
| `settings.json` | Settings, including your categories and color choices. Editable on the Settings tab. |
| `body.json` | Body profile, goal weight (with earlier goals) and the fallback calorie limit. Editable on the Body tab. |
| `inbox/` | Drop import `.json` files here. They are moved to `imported/`, or to `failed/` with an `.errors.txt`. |
| `backups/` | Daily copies of `items.json`, `calories.json`, `weight.json`, `journal.json` and `foods.json`, keeping 14 days of each. |
| `models/` | Whisper voice models. |
| `google_token.json` | Google sign-in token. Delete it to disconnect. |
| `google_events_cache.json`, `sync_state.json` | Sync bookkeeping. |
| `IMPORT_FORMAT.md` | The import spec, so you can point an AI tool at it. |

Set the `MORROWKEEP_DIR` environment variable to keep the data somewhere else, for example in a
OneDrive folder.

## Connecting Google Calendar (one-time, free)

1. Open <https://console.cloud.google.com/> and create a project.
2. **APIs & Services > Library**, then enable **Google Calendar API**.
3. **APIs & Services > OAuth consent screen**. Choose **External**, fill in an app name and your
   email, and add your own Google account under **Test users**.
4. **Credentials > Create credentials > OAuth client ID**, with application type **Desktop app**.
5. In Morrowkeep, open **Settings**, paste the client ID and secret, then click **Connect Google account**
   and approve in the browser.

Morrowkeep creates a calendar called "Morrowkeep" and syncs every 15 minutes, or on demand. Edits made
to Morrowkeep events in Google get overwritten on the next sync, because your local files win. Items
deleted in Morrowkeep are deleted from Google.

## Claude input mode (uses your Claude plan, not API credits)

Press **Tab** in the entry bar (or click its **Local/Claude** chip) to switch for the current
session. Set the mode Morrowkeep starts in under **Settings > Input & voice**.
In Claude mode, what you type or dictate goes to Claude Code (`claude -p`), signed in with your
own Claude account. The usage counts toward your Pro limits, the same pool as claude.ai. Claude
gets the import format, today's date and your upcoming items, and replies with import JSON.
Morrowkeep shows the proposed changes and nothing changes until you click **Apply**.

Requests are kept small: Claude Code's default system prompt is replaced with a short one, and
tools, MCP servers, your Claude Code settings and CLAUDE.md files are all switched off. Model
choice: Haiku (lightest), Sonnet (default) or Opus.

One-time setup: Morrowkeep finds `claude.exe` on PATH, at `~\.localin`, or bundled with the
Claude desktop app. Sign it in once: run it in a terminal, type `/login`, choose your Claude
account, then use **Test connection** in Settings. This is meant for your own personal use of
your own subscription.

## The entry bar

Start typing anywhere in the window and it goes into the entry bar, unless another text box is
active. **Tab** switches between Local and Claude while you type, and Esc clears the bar. The
mode it starts in is set under Settings > Input & voice. In **Local** mode (free, offline) it
understands:

| you type | does |
|---|---|
| `450 kcal pasta`, `+450`, `kcal 300 snack` | logs calories |
| `82.4 kg`, `weight 82,4`, `weighed 82.4` | logs today's weight |
| `journal: long walk, felt good`, `dear diary …` | adds a journal entry |
| `1 serving of kvarg`, `200 g chili`, `had 2 jars of kvarg` | logs a food from your library (exact kcal) |
| `goal 76 kg by feb 1`, `target weight 75.5` | sets your goal weight (without a date: 0.5 kg a week) |
| `done vacuum`, `did push-ups` | ticks off the matching task for today |
| `ate a chicken burrito`, `I had a banana` | estimates the calories with Claude, then shows them for review |
| anything else | adds a task (see below) |

The preview line under the bar always shows what Enter will do. In **Claude** mode, everything
goes to Claude, which can plan, log food, record weight, tick off tasks and change settings
("make the calorie color a little brighter", "add a study category with a vine streak", "no
reminders before 9"). Settings changes show up in the preview, with color swatches, before you
apply them.

Anything can be logged for another day: "make a journal entry for yesterday", "I ate a burrito
last night", "I did the vacuuming yesterday". Just after midnight Claude is told that the day
that just ended is probably the one you mean, so "tonight I had pasta" at 00:30 lands on
yesterday. To fill in gaps ("I drank 2L water on monday and tuesday, weighed 82.3 on
tuesday"), Claude sees the last 14 days (calories, weight, journal) and each habit's recent
check-offs, logs each thing on its own day, and the preview shows exactly which days get
ticked off.

### Tasks

| you type / say | becomes |
|---|---|
| `today`, `tomorrow`, `friday`, `next monday`, `oct 14`, `in 3 days`, `2026-11-02` | date |
| `6pm`, `at 18:00`, `at 7` (means 19:00), `noon`, `from 2pm to 3:30pm` | time (+ duration) |
| `for 45 minutes`, `1h30m`, `for an hour` | duration |
| `every day`, `weekdays`, `every mon and thu`, `every 2 weeks`, `monthly` | repeat |
| `until dec 31` | end of repeat |
| `#workout`, `#chore`, `#meal`, any `#category`, or a leading `Workout:` | category (kind) |
| `someday` / `backlog` | no date |

Anything else becomes the title. Without a date, the item goes on today (or on the selected day
in Calendar view).

## Themes

Every color comes from a theme: a JSON file in `Documents\Morrowkeep	hemes\`. Twelve built-in
themes are created there on start (missing ones only, your edits are kept):

| dark | light |
|---|---|
| Night sky (default), Dracula, Nord, Solarized Dark, Gruvbox Dark, Catppuccin Mocha, Tokyo Night, One Dark, Rosé Pine | Daylight, Solarized Light, Catppuccin Latte |

The ten named after other color schemes are inspired by them: each is generated from that
scheme's palette with the same rules (see `src/ui/palettes.rs`), and every built-in theme is
tested for readable text contrast. To make your own:

1. Copy a theme file (e.g. `night-sky.json` → `mine.json`).
2. Change its `name` and any colors (`"#RRGGBB"` or `"#RRGGBBAA"`).
3. Pick it in Settings > Appearance.

While a theme is active, saving its file applies the changes live. A theme file only needs the
keys it changes; anything missing falls back to Night sky. On start, Morrowkeep adds keys that
newer versions introduced to every theme file (with default values, never touching yours), so the
files always list every option. Sections:

| section | what it colors |
|---|---|
| `dark` | `true` for a dark theme, `false` for a light one |
| `surface` | page, dialogs (`window`), cards and tiles (`card`, `card_border`), text fields (`input`), weekend days in the calendar (`weekend`), `shadow` |
| `text` | `primary`, `muted`, `on_color` (text on filled tiles and chips) |
| `widget` | buttons (`fill`, `fill_hover`, `fill_active`, `border`, `border_hover`), `selection`, `link` |
| `status` | `success` (done, all done), `danger` (over budget, errors), `warning` (cautions such as a too-fast goal), `danger_background` |
| `kinds` | default color per category; a color picked in Settings > Categories wins. Unknown kinds use `other` |
| `data` | `brand` (logo and name), `today` (today's ring in the calendar), `calories`, `weight`, `claude`, `google`, `not_logged`, `gold`, `now_line`, `chart_limit_line`, `goal` (goal line, star, planned path), `trend` (7-day average and projection), `journal` |
| `sky` | the star field: `star` color (alpha = max brightness) and `density` (0 = off) |
| `shape` | `card_radius`, `tile_radius`, `widget_radius` |
| `charts` | `line_fill_alpha` (area under the weight line, 0 = none) and `bar_radius` (rounded bar tops) |
| `motion` | `enabled` (false = no animations) and `speed` (multiplier, e.g. 2.0 = twice as fast) |

## Shortcuts

- Drag a tab in the top bar to move it (Settings > Appearance > Reset tab order undoes it)
- `F11`: full screen on/off (Settings > General > Window also has **Always on top**)
- Just type: goes to the entry bar (`Ctrl+K` also focuses it, `Esc` clears it)
- `Enter`: add
- `Ctrl+Enter`: save in the editor
- `Esc`: close a dialog

## Start with Windows

Turn on **Settings > Window > Start with Windows**. It adds `morrowkeep.exe --hidden` to your user's
Run key, so Morrowkeep starts in the tray.

## License

[MIT](LICENSE).

Bundled fonts in `assets/fonts`: Inter and Nunito, both under the SIL Open Font License (see
`Inter-LICENSE.txt` and `Nunito-OFL.txt`).

Third-party code in `vendor/whisper-rs-sys`: the `whisper-rs-sys` crate is released under the
[Unlicense](https://unlicense.org/). The whisper.cpp sources inside it are MIT-licensed by the ggml
authors (see `vendor/whisper-rs-sys/whisper.cpp/LICENSE`).
