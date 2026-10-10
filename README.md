# Morrowkeep

A small, always-on Windows planner for tasks, chores, meals and workouts.

- **Local-first.** Everything is stored as readable JSON in `Documents\Morrowkeep`. You can edit
  `items.json` by hand; the app notices and reloads.
- **Habits page.**
  - Today's anytime tasks form a board of large tiles that scale with the window, under a
    constellation that lights a star per finished task. Click a tile and it flips, fills with
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
- **Monthly calorie budget.** Set a daily calorie limit in Settings. The month view shows the
  month's total against limit × days in the month. Today doesn't show the raw limit. It shows a
  *recommended* amount: what's left of the month's budget spread over the remaining days, so a big
  day lowers the following days. A past day with nothing logged counts as exactly your daily limit
  ("no food entered", not "ate nothing").
- **Weight.** Enter today's weight on the Today page (one value per day, stored in
  `weight.json`). It shows the change since your last weigh-in.
- **Streaks** (top of the Charts tab). One row per recurring task, with consecutive completions
  drawn as a streak in a style per kind:
  - workout: a ribbon that thickens
  - meal: comets, with the live streak glowing
  - chore: hopping arcs
  - habit: a vine that sprouts leaves at milestones
  - other: a constellation with a 4-point star every 7th day

  Missed days are hollow rings, and today pulses until done. One-off tasks appear as small stars
  on the day they were finished.
- **Calendar as a star map.** Each month cell is that day's constellation: done tasks lit and
  joined, missed ones as grey rings, upcoming ones as faint rings, and a gold star when everything
  was done. Only tasks with a time are listed as text. The day panel shows the day's anytime
  tasks as a vertical constellation (click a star to tick it off).
- **Charts tab.** A weight line chart and a calories-per-day bar chart (with your daily limit as a
  dashed line), over Week / Month / 3 months / 6 months / Year. Hover for exact values.
- **Reminders.** Windows notifications, also while Morrowkeep is in the tray: unfinished "anytime"
  items for today, no calories logged after 18:00, and no weight logged after 10:00. At most one
  every 60 minutes, with quiet hours (22:00–08:00).
  - Calorie and weight reminders have **input boxes**, so you can log right from the
    notification. Type a number for kcal, or a food description to have Morrowkeep open and
    estimate it with Claude.
  - Buttons: **Log**, **Open Morrowkeep** and **Snooze 2h**. Configure everything under
    Settings > Reminders.
  - Morrowkeep keeps its Start menu shortcut tagged with its notification ID. Windows needs that to
    show the notifications in the notification panel.
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
| `settings.json` | Settings. Editable in the app. |
| `inbox/` | Drop import `.json` files here. They are moved to `imported/`, or to `failed/` with an `.errors.txt`. |
| `backups/` | Daily copies of `items.json`, `calories.json` and `weight.json`, keeping 14 days of each. |
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
session. Set the mode Morrowkeep starts in under **Settings > Input mode**.
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
mode it starts in is set under Settings > Input mode. In **Local** mode (free, offline) it
understands:

| you type | does |
|---|---|
| `450 kcal pasta`, `+450`, `kcal 300 snack` | logs calories |
| `82.4 kg`, `weight 82,4`, `weighed 82.4` | logs today's weight |
| `done vacuum`, `did push-ups` | ticks off the matching task for today |
| `ate a chicken burrito`, `I had a banana` | estimates the calories with Claude, then shows them for review |
| anything else | adds a task (see below) |

The preview line under the bar always shows what Enter will do. In **Claude** mode, everything
goes to Claude, which can plan, log food, record weight and tick off tasks.

### Tasks

| you type / say | becomes |
|---|---|
| `today`, `tomorrow`, `friday`, `next monday`, `oct 14`, `in 3 days`, `2026-11-02` | date |
| `6pm`, `at 18:00`, `at 7` (means 19:00), `noon`, `from 2pm to 3:30pm` | time (+ duration) |
| `for 45 minutes`, `1h30m`, `for an hour` | duration |
| `every day`, `weekdays`, `every mon and thu`, `every 2 weeks`, `monthly` | repeat |
| `until dec 31` | end of repeat |
| `#workout`, `#chore`, `#meal`, or a leading `Workout:` | kind |
| `someday` / `backlog` | no date |

Anything else becomes the title. Without a date, the item goes on today (or on the selected day
in Calendar view).

## Shortcuts

- Just type: goes to the entry bar (`Ctrl+K` also focuses it, `Esc` clears it)
- `Enter`: add
- `Ctrl+Enter`: save in the editor
- `Esc`: close a dialog

## Start with Windows

Turn on **Settings > Window > Start with Windows**. It adds `morrowkeep.exe --hidden` to your user's
Run key, so Morrowkeep starts in the tray.

## License

[MIT](LICENSE).

Third-party code in `vendor/whisper-rs-sys`: the `whisper-rs-sys` crate is released under the
[Unlicense](https://unlicense.org/). The whisper.cpp sources inside it are MIT-licensed by the ggml
authors (see `vendor/whisper-rs-sys/whisper.cpp/LICENSE`).
