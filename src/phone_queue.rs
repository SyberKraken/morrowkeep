//! The phone's steps, run on the PC in the order they were made. The phone sends what it did
//! while the PC was unreachable (`POST /api/sync/push`); they're kept in `phone_queue.json`, so
//! nothing is lost if Morrowkeep closes, and a thread works through them one at a time: a step
//! starts only when the previous one is done. Each step runs as of the day and time it was made
//! on the phone. Claude, label, recording and reflection steps are carried out here (Claude's
//! answers are applied straight away); before each, the parts it may change are backed up so
//! the step can be undone from the PC's "From your phone" panel.

use crate::backup;
use crate::store::{read_json, write_json, Data, Paths};
use crate::worker::UiHandle;
use chrono::{DateTime, Local, NaiveDate, NaiveTime};
use morrowkeep_core::steps::Step;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Waiting,
    Running,
    Applied,
    Failed,
    Skipped,
}

impl Status {
    pub fn finished(self) -> bool {
        matches!(self, Status::Applied | Status::Failed | Status::Skipped)
    }
}

/// A step from the phone, and what became of it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Queued {
    /// The step's id (made on the phone).
    pub id: String,
    pub device: String,
    /// Order on that phone.
    pub seq: u64,
    /// When and on which day it was made (the step runs as of then).
    pub at: DateTime<Local>,
    pub date: NaiveDate,
    #[serde(default)]
    pub time: Option<NaiveTime>,
    pub step: Step,
    #[serde(default)]
    pub status: Status,
    /// What happened, or why not.
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub done_at: Option<DateTime<Local>>,
    /// Backups taken before the step ran (part, file), for Undo.
    #[serde(default)]
    pub undo: Vec<(String, PathBuf)>,
}

#[derive(Default, Serialize, Deserialize)]
struct QueueFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    steps: Vec<Queued>,
    /// When phones were last told "Morrowkeep is on".
    #[serde(default)]
    pc_on_sent: Option<DateTime<Local>>,
}

/// Tells phones the PC is on (once Tailscale has had a moment), at most every 6 hours. The phone
/// only shows it if it has steps waiting, and then sends them right away (sw.js).
pub fn announce_pc_on(shared: Arc<Mutex<Data>>) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(20));
        let (paths, on) = {
            let d = shared.lock().unwrap();
            (d.paths.clone(), d.settings.phone_enabled && crate::phone::has_push(&d))
        };
        if !on {
            return;
        }
        let due = with_queue(&paths, |q| {
            let due = q.pc_on_sent.is_none_or(|t| Local::now() - t > chrono::Duration::hours(6));
            if due {
                q.pc_on_sent = Some(Local::now());
                save(&paths, q);
            }
            due
        });
        if due {
            let payload = serde_json::json!({"type": "pc_on", "title": "Morrowkeep is on", "body": "Tap to send what you did on the phone", "tag": "pc-on", "go": "outbox"});
            crate::phone::push_all(&shared, &payload);
        }
    });
}

/// Finished steps kept for the panel and the phone's results.
const KEEP_FINISHED: usize = 300;

static QUEUE: Mutex<Option<QueueFile>> = Mutex::new(None);
/// Wakes the runner when steps arrive.
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

fn queue_path(p: &Paths) -> PathBuf {
    p.root.join("phone_queue.json")
}

/// Where uploaded photos and recordings wait for their step.
pub fn upload_path(p: &Paths, step_id: &str) -> PathBuf {
    let safe: String = step_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    p.root.join("phone-uploads").join(safe)
}

fn with_queue<T>(paths: &Paths, f: impl FnOnce(&mut QueueFile) -> T) -> T {
    let mut q = QUEUE.lock().unwrap();
    if q.is_none() {
        let mut file: QueueFile = read_json(&queue_path(paths)).ok().flatten().unwrap_or_default();
        // a step that was running when Morrowkeep closed runs again (steps are safe to repeat)
        for s in &mut file.steps {
            if s.status == Status::Running {
                s.status = Status::Waiting;
            }
        }
        *q = Some(file);
    }
    f(q.as_mut().unwrap())
}

fn save(paths: &Paths, q: &mut QueueFile) {
    let finished = q.steps.iter().filter(|s| s.status.finished()).count();
    if finished > KEEP_FINISHED {
        let mut drop = finished - KEEP_FINISHED;
        q.steps.retain(|s| {
            if drop > 0 && s.status.finished() {
                drop -= 1;
                false
            } else {
                true
            }
        });
    }
    q.version = 1;
    let _ = write_json(&queue_path(paths), q);
}

fn wake() {
    let (lock, cv) = &WAKE;
    *lock.lock().unwrap() = true;
    cv.notify_all();
}

/// Adds steps from a phone (those it hasn't sent before); returns the ids now held.
pub fn push(paths: &Paths, steps: Vec<Queued>) -> Vec<String> {
    let ids: Vec<String> = steps.iter().map(|s| s.id.clone()).collect();
    with_queue(paths, |q| {
        for s in steps {
            if !q.steps.iter().any(|x| x.id == s.id) {
                q.steps.push(s);
            }
        }
        save(paths, q);
    });
    wake();
    ids
}

/// The steps with these ids (for the phone's results).
pub fn status(paths: &Paths, ids: &[String]) -> Vec<Queued> {
    with_queue(paths, |q| q.steps.iter().filter(|s| ids.contains(&s.id)).cloned().collect())
}

/// Ids of a phone's steps that are done with (applied, failed or skipped): the phone stops
/// replaying those on top of the PC's copy.
pub fn finished_ids(paths: &Paths, device: &str) -> Vec<String> {
    with_queue(paths, |q| q.steps.iter().filter(|s| s.device == device && s.status.finished()).map(|s| s.id.clone()).collect())
}

/// Everything in the queue, oldest first (for the PC's panel).
pub fn all(paths: &Paths) -> Vec<Queued> {
    with_queue(paths, |q| q.steps.clone())
}

/// Skips a waiting step, or runs a failed one again.
pub fn set_waiting_or_skipped(paths: &Paths, id: &str, skip: bool) {
    with_queue(paths, |q| {
        if let Some(s) = q.steps.iter_mut().find(|s| s.id == id) {
            if skip && s.status == Status::Waiting {
                s.status = Status::Skipped;
                s.message = "Skipped on the PC".into();
                s.done_at = Some(Local::now());
            } else if !skip && matches!(s.status, Status::Failed | Status::Skipped) {
                s.status = Status::Waiting;
                s.message.clear();
            }
        }
        save(paths, q);
    });
    wake();
}

/// Undoes a step that ran: restores the parts backed up before it (later changes to those parts
/// are undone too).
pub fn undo(d: &mut Data, id: &str) -> Result<String, String> {
    let paths = d.paths.clone();
    let parts = with_queue(&paths, |q| q.steps.iter().find(|s| s.id == id).map(|s| s.undo.clone())).ok_or("That step is gone")?;
    if parts.is_empty() {
        return Err("Nothing was backed up for this step".into());
    }
    if let Some((_, f)) = parts.iter().find(|(_, f)| !f.exists()) {
        return Err(format!("the backup {} is gone (the last 20 are kept)", f.display()));
    }
    for (part, file) in &parts {
        backup::restore(&paths, part, file)?;
        d.reload_after_restore(part);
    }
    with_queue(&paths, |q| {
        if let Some(s) = q.steps.iter_mut().find(|s| s.id == id) {
            s.undo.clear();
            s.message = format!("Undone · {}", s.message);
        }
        save(&paths, q);
    });
    Ok("Undone".into())
}

/// Starts the thread that works through the queue (once).
pub fn start(shared: Arc<Mutex<Data>>, ui: UiHandle) {
    std::thread::Builder::new()
        .name("phone queue".into())
        .spawn(move || loop {
            let paths = shared.lock().unwrap().paths.clone();
            // the next waiting step, in the order they arrived
            let next = with_queue(&paths, |q| {
                let i = q.steps.iter().position(|s| s.status == Status::Waiting)?;
                q.steps[i].status = Status::Running;
                save(&paths, q);
                Some(q.steps[i].clone())
            });
            match next {
                Some(item) => {
                    crate::phone::repaint(&ui);
                    let (status, message, undo) = run(&shared, &ui, &item);
                    with_queue(&paths, |q| {
                        if let Some(s) = q.steps.iter_mut().find(|s| s.id == item.id) {
                            s.status = status;
                            s.message = message.clone();
                            s.undo = undo;
                            s.done_at = Some(Local::now());
                        }
                        save(&paths, q);
                    });
                    let mut d = shared.lock().unwrap();
                    d.status = format!("📱 {message}");
                    drop(d);
                    crate::phone::repaint(&ui);
                }
                None => {
                    // nothing to do: sleep until steps arrive (no polling)
                    let (lock, cv) = &WAKE;
                    let mut woken = lock.lock().unwrap();
                    while !*woken {
                        woken = cv.wait(woken).unwrap();
                    }
                    *woken = false;
                }
            }
        })
        .expect("start the phone queue");
}

/// Backs up the parts a Claude/label/reflection step may change.
fn backup_parts(paths: &Paths, parts: &[&str]) -> Vec<(String, PathBuf)> {
    parts.iter().filter_map(|p| backup::snapshot(paths, p, "phone-step").ok().map(|f| (p.to_string(), f))).collect()
}

/// Carries out one step; returns its status, message and backups.
fn run(shared: &Arc<Mutex<Data>>, ui: &UiHandle, item: &Queued) -> (Status, String, Vec<(String, PathBuf)>) {
    let at = item.at;
    let day = item.date;
    match &item.step {
        Step::Claude { text, food } => {
            let (cli, prompt, model, dir, paths) = {
                let d = shared.lock().unwrap();
                let Some(cli) = crate::claude::find_cli(&d.settings) else {
                    return (Status::Failed, "Claude Code wasn't found on the PC".into(), vec![]);
                };
                let request = if *food { format!("Log the calories for what I ate/drank (estimate high): {text}") } else { text.clone() };
                (cli, crate::claude::build_prompt_at(&d, &request, at), d.settings.claude_model.clone(), d.paths.root.clone(), d.paths.clone())
            };
            let reply = match crate::claude::ask(&cli, &model, &prompt, &dir) {
                Ok(r) => r,
                Err(e) => return (Status::Failed, e, vec![]),
            };
            let undo = backup_parts(&paths, &["items", "calories", "weight", "foods", "journal", "workouts"]);
            let mut d = shared.lock().unwrap();
            let plan = crate::import::plan(&reply, &d.items, &d.calories, &d.settings, &d.foods, &d.exercises.exercises, day);
            if plan.ops.is_empty() {
                let why = if plan.errors.is_empty() { plan.message.clone().unwrap_or_else(|| "Claude had nothing to change".into()) } else { plan.errors.join("; ") };
                return (if plan.errors.is_empty() { Status::Applied } else { Status::Failed }, format!("\"{text}\": {why}"), vec![]);
            }
            let summary = crate::import::apply(&mut d, plan.ops);
            let note = plan.message.map(|m| format!(" · {m}")).unwrap_or_default();
            (Status::Applied, format!("Claude, \"{text}\": {summary}{note}"), undo)
        }
        Step::Label { food_id } => {
            let (settings, root, paths) = {
                let d = shared.lock().unwrap();
                (d.settings.clone(), d.paths.root.clone(), d.paths.clone())
            };
            let photo = match std::fs::read(upload_path(&paths, &item.id)) {
                Ok(b) => b,
                Err(_) => return (Status::Failed, "The label photo didn't arrive from the phone".into(), vec![]),
            };
            let jpeg = match crate::label::prepare(&photo) {
                Ok(j) => j,
                Err(e) => return (Status::Failed, e, vec![]),
            };
            let label = match crate::label::read(&settings, &root, &jpeg) {
                Ok(l) => l,
                Err(e) => return (Status::Failed, e, vec![]),
            };
            let undo = backup_parts(&paths, &["foods"]);
            let mut food = label.food;
            food.id = food_id.clone();
            let _ = crate::label::save_photo(&root, food_id, &jpeg);
            let _ = std::fs::remove_file(upload_path(&paths, &item.id));
            match shared.lock().unwrap().apply_step_on(&Step::FoodSave { food }, day) {
                Ok(m) => (Status::Applied, format!("Label read · {m}{}", if label.note.is_empty() { String::new() } else { format!(" · {}", label.note) }), undo),
                Err(e) => (Status::Failed, e, undo),
            }
        }
        Step::JournalAudio { entry } => {
            let paths = shared.lock().unwrap().paths.clone();
            let wav = match std::fs::read(upload_path(&paths, &item.id)) {
                Ok(b) => b,
                Err(_) => return (Status::Failed, "The recording didn't arrive from the phone".into(), vec![]),
            };
            if shared.lock().unwrap().journal.iter().any(|e| e.id == entry.id) {
                return (Status::Applied, "Journal entry saved".into(), vec![]);
            }
            match crate::phone::save_recording(shared, ui, entry.clone(), &wav) {
                Ok(m) => {
                    let _ = std::fs::remove_file(upload_path(&paths, &item.id));
                    (Status::Applied, m, vec![])
                }
                Err(e) => (Status::Failed, e, vec![]),
            }
        }
        Step::Reflect { day: week_end } => {
            let (cli, prompt, model, dir, paths) = {
                let d = shared.lock().unwrap();
                let Some(prompt) = crate::journal::reflection_prompt(&d.journal, *week_end) else {
                    return (Status::Failed, "Nothing written in the 7 days before to reflect on".into(), vec![]);
                };
                let Some(cli) = crate::claude::find_cli(&d.settings) else {
                    return (Status::Failed, "Claude Code wasn't found on the PC".into(), vec![]);
                };
                (cli, prompt, d.settings.claude_model.clone(), d.paths.root.clone(), d.paths.clone())
            };
            match crate::claude::ask(&cli, &model, &prompt, &dir) {
                Ok(text) => {
                    let undo = backup_parts(&paths, &["journal"]);
                    let mut e = crate::journal::JournalEntry::new(text.trim());
                    e.kind = "reflection".into();
                    e.date = *week_end;
                    e.time = item.time;
                    let r = shared.lock().unwrap().apply_step_on(&Step::JournalAdd { entry: e }, day);
                    match r {
                        Ok(_) => (Status::Applied, "Claude's reflection added to your journal".into(), undo),
                        Err(e) => (Status::Failed, e, undo),
                    }
                }
                Err(e) => (Status::Failed, e, vec![]),
            }
        }
        // everything else: the same rules as on the phone, as of the day it was made
        step => match shared.lock().unwrap().apply_step_on(step, day) {
            Ok(m) => (Status::Applied, m, vec![]),
            Err(e) => (Status::Failed, e, vec![]),
        },
    }
}
