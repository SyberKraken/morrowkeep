//! Phone access: a small web server inside Morrowkeep that serves a mobile web app (in
//! phone_web/) and a JSON API for it. It listens on 127.0.0.1 only; Tailscale's `serve` puts it
//! on your private tailnet with HTTPS (needed for the microphone and "install as app").
//!
//! A phone is paired once with a one-time code (shown as a QR code in Settings > Phone) and
//! then sends a secret token with every request; only its SHA-256 is stored. Everything goes
//! through the same data and code as the desktop app, so changes show up on both at once.

use crate::store::Data;
use crate::worker::UiHandle;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiny_http::{Header, Method, Request, Response, Server};

const INDEX: &str = include_str!("phone_web/index.html");
const APP_JS: &str = include_str!("phone_web/app.js");
const SW_JS: &str = include_str!("phone_web/sw.js");
const MANIFEST: &str = include_str!("phone_web/manifest.webmanifest");
const ICON: &[u8] = include_bytes!("../assets/morrowkeep.png");
/// The phone app's screens, one file each (they share app.js's helpers).
const SCRIPTS: &[(&str, &str)] = &[("/plan.js", include_str!("phone_web/plan.js")), ("/foods.js", include_str!("phone_web/foods.js")), ("/train.js", include_str!("phone_web/train.js"))];

/// How long a pairing code works.
const PAIR_SECS: u64 = 600;

/// A paired phone. Only a hash of its token is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneDevice {
    pub id: String,
    pub name: String,
    pub token_sha256: String,
    pub added: NaiveDate,
    /// Where this phone receives notifications, once turned on in its app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push: Option<crate::push::Subscription>,
}

struct Running {
    server: Arc<Server>,
    port: u16,
    thread: std::thread::JoinHandle<()>,
}

struct AskJob {
    rx: Option<Receiver<Result<String, String>>>,
    result: Option<Result<String, String>>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static STATUS: Mutex<String> = Mutex::new(String::new());
static PAIR: Mutex<Option<(String, Instant)>> = Mutex::new(None);
static LAST_SEEN: Mutex<Option<HashMap<String, DateTime<Local>>>> = Mutex::new(None);
static JOBS: Mutex<Option<HashMap<String, AskJob>>> = Mutex::new(None);

/// A label photo being read for the phone: Claude's reading (once done) and the photo, kept
/// until the food is saved.
struct LabelJob {
    rx: Option<Receiver<Result<crate::label::LabelFood, String>>>,
    result: Option<Result<crate::label::LabelFood, String>>,
    jpeg: Arc<Vec<u8>>,
}

static LABEL_JOBS: Mutex<Option<HashMap<String, LabelJob>>> = Mutex::new(None);
static TRANSCRIBING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn sha(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn repaint(ui: &UiHandle) {
    if let Some(ctx) = ui.lock().unwrap().as_ref() {
        ctx.request_repaint();
    }
}

// ------------------------------------------------------------------ server lifecycle

/// Starts, restarts or stops the server to match the settings. Called by the background
/// worker every few seconds, so it also runs while Morrowkeep is only in the tray.
pub fn sync(shared: &Arc<Mutex<Data>>, ui: &UiHandle) {
    let (enabled, port) = {
        let d = shared.lock().unwrap();
        (d.settings.phone_enabled, d.settings.phone_port)
    };
    let mut running = RUNNING.lock().unwrap();
    let wrong_port = running.as_ref().is_some_and(|r| r.port != port);
    if (!enabled || wrong_port) && running.is_some() {
        let r = running.take().unwrap();
        r.server.unblock();
        let _ = r.thread.join();
        *STATUS.lock().unwrap() = "Stopped".into();
    }
    if enabled && running.is_none() {
        match Server::http(("127.0.0.1", port)) {
            Ok(server) => {
                let server = Arc::new(server);
                let (s, sh, u) = (server.clone(), shared.clone(), ui.clone());
                let thread = std::thread::spawn(move || {
                    for rq in s.incoming_requests() {
                        handle(rq, &sh, &u);
                    }
                });
                *running = Some(Running { server, port, thread });
                *STATUS.lock().unwrap() = format!("Running on 127.0.0.1:{port}");
            }
            Err(e) => *STATUS.lock().unwrap() = format!("Couldn't start on port {port}: {e}"),
        }
    }
}

pub fn status() -> String {
    STATUS.lock().unwrap().clone()
}

pub fn is_running() -> bool {
    RUNNING.lock().unwrap().is_some()
}

/// A new one-time pairing code (replaces any earlier one).
pub fn new_pair_code() -> String {
    let code: String = uuid::Uuid::new_v4().simple().to_string().chars().take(8).collect::<String>().to_uppercase();
    *PAIR.lock().unwrap() = Some((code.clone(), Instant::now()));
    code
}

/// The current pairing code and its seconds left, if any.
pub fn pair_code() -> Option<(String, u64)> {
    let p = PAIR.lock().unwrap();
    let (code, t) = p.as_ref()?;
    let left = PAIR_SECS.checked_sub(t.elapsed().as_secs())?;
    Some((code.clone(), left))
}

pub fn last_seen(device_id: &str) -> Option<DateTime<Local>> {
    LAST_SEEN.lock().unwrap().as_ref()?.get(device_id).copied()
}

// ------------------------------------------------------------------ Tailscale

pub fn tailscale_exe() -> Option<PathBuf> {
    [r"C:\Program Files\Tailscale\tailscale.exe", r"C:\Program Files (x86)\Tailscale\tailscale.exe"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

fn tailscale(args: &[&str]) -> Result<String, String> {
    let exe = tailscale_exe().ok_or("Tailscale isn't installed")?;
    let out = std::process::Command::new(exe)
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.is_empty() { "Tailscale reported an error".into() } else { text })
    }
}

/// This PC's name on the tailnet ("my-pc.tail1234.ts.net"), if Tailscale is connected.
pub fn tailscale_name() -> Result<String, String> {
    let v: Value = serde_json::from_str(&tailscale(&["status", "--json"])?).map_err(|e| e.to_string())?;
    if v["BackendState"].as_str() != Some("Running") {
        return Err(format!("Tailscale isn't connected ({})", v["BackendState"].as_str().unwrap_or("unknown state")));
    }
    v["Self"]["DNSName"].as_str().map(|s| s.trim_end_matches('.').to_string()).filter(|s| !s.is_empty()).ok_or("MagicDNS is off in Tailscale".into())
}

/// Serves Morrowkeep on the tailnet over HTTPS (port 443 → 127.0.0.1:port).
pub fn tailscale_publish(port: u16) -> Result<String, String> {
    tailscale(&["serve", "--bg", &format!("http://127.0.0.1:{port}")])
}

pub fn tailscale_unpublish() -> Result<String, String> {
    tailscale(&["serve", "--https=443", "off"])
}

// ------------------------------------------------------------------ requests

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn respond(rq: Request, code: u16, content_type: &str, body: Vec<u8>) {
    let resp = Response::from_data(body)
        .with_status_code(code)
        .with_header(header("Content-Type", content_type))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("X-Content-Type-Options", "nosniff"));
    let _ = rq.respond(resp);
}

fn respond_json(rq: Request, code: u16, v: &Value) {
    respond(rq, code, "application/json; charset=utf-8", v.to_string().into_bytes());
}

fn query(url: &str) -> HashMap<String, String> {
    url.split_once('?')
        .map(|(_, q)| q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), url_decode(v))).collect())
        .unwrap_or_default()
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn handle(mut rq: Request, shared: &Arc<Mutex<Data>>, ui: &UiHandle) {
    let url = rq.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();
    let method = rq.method().clone();
    match (&method, path.as_str()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => return respond(rq, 200, "text/html; charset=utf-8", INDEX.as_bytes().to_vec()),
        (Method::Get, "/app.js") => return respond(rq, 200, "text/javascript; charset=utf-8", APP_JS.as_bytes().to_vec()),
        (Method::Get, "/sw.js") => return respond(rq, 200, "text/javascript; charset=utf-8", SW_JS.as_bytes().to_vec()),
        (Method::Get, "/manifest.webmanifest") => return respond(rq, 200, "application/manifest+json", MANIFEST.as_bytes().to_vec()),
        (Method::Get, "/icon.png") => return respond(rq, 200, "image/png", ICON.to_vec()),
        (Method::Get, p) if SCRIPTS.iter().any(|(n, _)| *n == p) => {
            let js = SCRIPTS.iter().find(|(n, _)| *n == p).unwrap().1;
            return respond(rq, 200, "text/javascript; charset=utf-8", js.as_bytes().to_vec());
        }
        _ => {}
    }
    if !path.starts_with("/api/") {
        return respond(rq, 404, "text/plain", b"Not found".to_vec());
    }
    // body (audio uploads are the big ones: 5 min at 16 kHz is about 10 MB)
    let mut body = Vec::new();
    if rq.as_reader().take(30 * 1024 * 1024).read_to_end(&mut body).is_err() {
        return respond_json(rq, 400, &json!({"error": "Couldn't read the request"}));
    }
    let json_body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if path == "/api/pair" && method == Method::Post {
        let (code, v) = pair(shared, &json_body);
        return respond_json(rq, code, &v);
    }
    // everything else needs a paired phone
    let token = rq
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .and_then(|h| h.value.as_str().strip_prefix("Bearer ").map(str::to_string))
        .unwrap_or_default();
    let device = {
        let d = shared.lock().unwrap();
        let hash = sha(&token);
        d.settings.phone_devices.iter().find(|p| !token.is_empty() && p.token_sha256 == hash).map(|p| p.id.clone())
    };
    let Some(device) = device else {
        return respond_json(rq, 401, &json!({"error": "This phone isn't paired (or was removed). Pair it again from Settings > Phone on your PC."}));
    };
    LAST_SEEN.lock().unwrap().get_or_insert_with(HashMap::new).insert(device.clone(), Local::now());
    let q = query(&url);
    let result = api(&method, &path, &q, &json_body, body, shared, ui, &device);
    match result {
        Ok(v) => respond_json(rq, 200, &v),
        Err((code, msg)) => respond_json(rq, code, &json!({"error": msg})),
    }
}

fn pair(shared: &Arc<Mutex<Data>>, body: &Value) -> (u16, Value) {
    let code = body["code"].as_str().unwrap_or("").trim().to_uppercase();
    let ok = pair_code().is_some_and(|(c, _)| !code.is_empty() && c == code);
    if !ok {
        return (403, json!({"error": "That pairing code isn't valid any more. Make a new one in Settings > Phone on your PC."}));
    }
    *PAIR.lock().unwrap() = None;
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let name = body["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Phone").chars().take(40).collect::<String>();
    let mut d = shared.lock().unwrap();
    d.settings.phone_devices.push(PhoneDevice { id: uuid::Uuid::new_v4().to_string(), name: name.clone(), token_sha256: sha(&token), added: Local::now().date_naive(), push: None });
    d.save_settings();
    d.status = format!("Phone paired: {name}");
    (200, json!({"token": token, "name": name}))
}

type ApiResult = Result<Value, (u16, String)>;

fn bad(msg: impl Into<String>) -> (u16, String) {
    (400, msg.into())
}

/// The day an entry belongs to: "date" (YYYY-MM-DD, today, yesterday) or today.
fn day_of(v: &Value, q: &HashMap<String, String>) -> Result<NaiveDate, (u16, String)> {
    let today = Local::now().date_naive();
    match v["date"].as_str().or(q.get("date").map(String::as_str)).filter(|s| !s.is_empty()) {
        None => Ok(today),
        Some(s) => crate::parse::parse_date(s, today).filter(|d| *d <= today).ok_or_else(|| bad(format!("\"{s}\" isn't a date up to today"))),
    }
}

fn time_of(v: &Value, q: &HashMap<String, String>) -> Option<NaiveTime> {
    v["time"].as_str().or(q.get("time").map(String::as_str)).and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())
}

#[allow(clippy::too_many_arguments)]
fn api(method: &Method, path: &str, q: &HashMap<String, String>, v: &Value, body: Vec<u8>, shared: &Arc<Mutex<Data>>, ui: &UiHandle, device: &str) -> ApiResult {
    let today = Local::now().date_naive();
    let done = |d: &mut Data, msg: String| {
        d.status = format!("Phone: {msg}");
        repaint(ui);
        Ok(json!({"ok": true, "message": msg}))
    };
    match (method, path) {
        (Method::Get, "/api/state") => Ok(state(&shared.lock().unwrap(), device)),

        // ---- notifications
        (Method::Get, "/api/push/key") => {
            let mut d = shared.lock().unwrap();
            if d.settings.push_key.is_empty() {
                d.settings.push_key = crate::push::new_key();
                d.save_settings();
            }
            let key = crate::push::public_key(&d.settings.push_key).map_err(|e| (500, e))?;
            Ok(json!({"key": key}))
        }
        (Method::Post, "/api/push/subscribe") => {
            let s = |k: &str| v.pointer(k).and_then(Value::as_str).map(str::to_string).filter(|s| !s.is_empty());
            let (Some(endpoint), Some(p256dh), Some(auth)) = (s("/endpoint"), s("/keys/p256dh"), s("/keys/auth")) else {
                return Err(bad("The phone didn't send a complete subscription"));
            };
            if !endpoint.starts_with("https://") {
                return Err(bad("Unexpected push address"));
            }
            let mut d = shared.lock().unwrap();
            if let Some(dev) = d.settings.phone_devices.iter_mut().find(|p| p.id == device) {
                dev.push = Some(crate::push::Subscription { endpoint, p256dh, auth });
            }
            d.save_settings();
            done(&mut d, "notifications turned on".into())
        }
        (Method::Post, "/api/push/unsubscribe") => {
            let mut d = shared.lock().unwrap();
            if let Some(dev) = d.settings.phone_devices.iter_mut().find(|p| p.id == device) {
                dev.push = None;
            }
            d.save_settings();
            done(&mut d, "notifications turned off".into())
        }
        (Method::Post, "/api/push/test") => {
            send_test(shared, device)?;
            Ok(json!({"ok": true, "message": "Test notification sent"}))
        }
        (Method::Post, "/api/snooze") => {
            let hours = v["hours"].as_f64().unwrap_or(2.0).clamp(0.25, 24.0);
            crate::notify::snooze(hours);
            let mut d = shared.lock().unwrap();
            done(&mut d, format!("reminders snoozed for {hours} h"))
        }

        (Method::Post, "/api/weight") => {
            let kg = v["kg"].as_f64().filter(|k| (20.0..=400.0).contains(k)).ok_or_else(|| bad("Enter a weight between 20 and 400 kg"))? as f32;
            let day = day_of(v, q)?;
            let mut d = shared.lock().unwrap();
            d.set_weight(day, Some(kg));
            done(&mut d, format!("weight {kg:.1} kg"))
        }

        (Method::Post, "/api/calories") => {
            let kcal = v["kcal"].as_f64().filter(|k| (1.0..=20000.0).contains(k)).ok_or_else(|| bad("Enter calories between 1 and 20000"))?.ceil() as u32;
            let day = day_of(v, q)?;
            let what = v["description"].as_str().unwrap_or("").trim().to_string();
            let mut e = crate::model::CalorieEntry::new(day, kcal, what, "phone");
            if let Some(t) = time_of(v, q) {
                e.time = Some(t);
            }
            let mut d = shared.lock().unwrap();
            d.calories.push(e);
            d.save_calories();
            let total = d.calories_on(day);
            done(&mut d, format!("+{kcal} kcal ({total} that day)"))
        }

        (Method::Post, "/api/food") => {
            let id = v["id"].as_str().ok_or_else(|| bad("Missing food"))?;
            let amount = match (v["servings"].as_f64(), v["grams"].as_f64()) {
                (_, Some(g)) if g > 0.0 => crate::foods::Amount::Grams(g as f32),
                (Some(s), _) if s > 0.0 => crate::foods::Amount::Servings(s as f32),
                _ => crate::foods::Amount::Servings(1.0),
            };
            let day = day_of(v, q)?;
            let mut d = shared.lock().unwrap();
            let p = d.foods.by_id(id).ok_or_else(|| bad("That food isn't in your library any more"))?;
            let mut e = Data::food_entry(&p, amount, day, "phone");
            if let Some(t) = time_of(v, q) {
                e.time = Some(t);
            }
            let msg = format!("+{} kcal · {}", e.kcal, e.description);
            d.calories.push(e);
            d.save_calories();
            done(&mut d, msg)
        }

        // a photo of a nutrition label (JPEG from the phone): Claude reads it in the background
        (Method::Post, "/api/food/label") => {
            let jpeg = Arc::new(crate::label::prepare(&body).map_err(bad)?);
            let (settings, root) = {
                let d = shared.lock().unwrap();
                crate::claude::find_cli(&d.settings).ok_or_else(|| bad("Claude Code wasn't found on your PC"))?;
                (d.settings.clone(), d.paths.root.clone())
            };
            let (tx, rx) = mpsc::channel();
            let photo = jpeg.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::label::read(&settings, &root, &photo));
            });
            let id = uuid::Uuid::new_v4().simple().to_string();
            LABEL_JOBS.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), LabelJob { rx: Some(rx), result: None, jpeg });
            Ok(json!({"job": id}))
        }

        (Method::Get, "/api/food/label") => {
            let id = q.get("job").ok_or_else(|| bad("Missing job"))?;
            let mut jobs = LABEL_JOBS.lock().unwrap();
            let job = jobs.get_or_insert_with(HashMap::new).get_mut(id).ok_or_else(|| bad("That photo is gone; take it again"))?;
            if let Some(rx) = &job.rx {
                if let Ok(r) = rx.try_recv() {
                    job.result = Some(r);
                    job.rx = None;
                }
            }
            match &job.result {
                None => Ok(json!({"done": false})),
                Some(Ok(l)) => {
                    let f = &l.food;
                    // one decimal, so 3.9 isn't sent as 3.9000000953674316
                    let r1 = |x: f32| (x as f64 * 10.0).round() / 10.0;
                    Ok(json!({"done": true, "note": l.note, "food": {"name": f.name, "kcal_per_100g": r1(f.kcal_per_100g), "protein_per_100g": f.protein_per_100g.map(r1),
                        "carbs_per_100g": f.carbs_per_100g.map(r1), "fat_per_100g": f.fat_per_100g.map(r1), "serving_g": r1(f.serving_g), "serving_name": f.serving_name}}))
                }
                Some(Err(e)) => Ok(json!({"done": true, "error": e})),
            }
        }

        // save a food (reviewed on the phone) to the library, with its label photo
        (Method::Post, "/api/food/new") => {
            let name = v["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("Give the food a name"))?;
            let num = |k: &str, max: f64| v[k].as_f64().filter(|n| n.is_finite() && *n >= 0.0 && *n <= max).map(|n| n as f32);
            let kcal = num("kcal_per_100g", 900.0).ok_or_else(|| bad("Calories per 100 g must be a number from 0 to 900"))?;
            let food = crate::foods::Food {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.chars().take(60).collect(),
                kcal_per_100g: kcal,
                serving_g: num("serving_g", 5000.0).filter(|g| *g >= 1.0).unwrap_or(100.0),
                serving_name: v["serving_name"].as_str().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "serving".into()),
                protein_per_100g: num("protein_per_100g", 100.0),
                carbs_per_100g: num("carbs_per_100g", 100.0),
                fat_per_100g: num("fat_per_100g", 100.0),
                ..Default::default()
            };
            let job = v["job"].as_str().and_then(|j| LABEL_JOBS.lock().unwrap().get_or_insert_with(HashMap::new).remove(j));
            let mut d = shared.lock().unwrap();
            if let Some(job) = job {
                let _ = crate::label::save_photo(&d.paths.root, &food.id, &job.jpeg);
            }
            let (id, msg) = (food.id.clone(), format!("{} added to your foods", food.name));
            d.foods.foods.push(food);
            d.save_foods();
            d.status = format!("Phone: {msg}");
            repaint(ui);
            Ok(json!({"ok": true, "message": msg, "id": id}))
        }

        (Method::Post, "/api/food/label/discard") => {
            if let Some(j) = v["job"].as_str() {
                LABEL_JOBS.lock().unwrap().get_or_insert_with(HashMap::new).remove(j);
            }
            Ok(json!({"ok": true}))
        }

        // the entry bar's Local mode, same as on the PC
        (Method::Post, "/api/entry") => {
            let text = v["text"].as_str().unwrap_or("");
            let day = day_of(v, q)?;
            let mut d = shared.lock().unwrap();
            match crate::entry::run(&mut d, text, day, today) {
                crate::entry::Outcome::Done(msg) => done(&mut d, msg),
                crate::entry::Outcome::Claude(food) => Ok(json!({"claude": food})),
                crate::entry::Outcome::Error(msg) => Err(bad(msg)),
            }
        }

        // the Plan tab: overdue and undated tasks, then the next `days` days
        (Method::Get, "/api/agenda") => {
            let days = q.get("days").and_then(|n| n.parse::<i64>().ok()).unwrap_or(14).clamp(1, 60);
            let d = shared.lock().unwrap();
            let end = today + Duration::days(days - 1);
            let mut overdue: Vec<&crate::model::Item> = d.items.iter().filter(|it| it.repeat.is_none() && !it.done && it.date.is_some_and(|x| x < today)).collect();
            overdue.sort_by_key(|it| it.date);
            let backlog: Vec<Value> = d.items.iter().filter(|it| it.date.is_none() && !it.done).map(|it| item_json(it, today)).collect();
            let occ = crate::model::occurrences(&d.items, today, end);
            let mut list: Vec<Value> = Vec::new();
            let mut day = today;
            while day <= end {
                let mut its: Vec<&crate::model::Item> = occ.iter().filter(|(x, _)| *x == day).map(|(_, i)| &d.items[*i]).collect();
                its.sort_by_key(|it| (it.time.is_none(), it.time));
                let label = match (day - today).num_days() {
                    0 => "Today".to_string(),
                    1 => "Tomorrow".to_string(),
                    _ => day.format("%A %-d %b").to_string(),
                };
                list.push(json!({"date": day.to_string(), "label": label, "items": its.iter().map(|it| item_json(it, day)).collect::<Vec<_>>()}));
                day += Duration::days(1);
            }
            Ok(json!({"overdue": overdue.iter().map(|it| item_json(it, it.date.unwrap_or(today))).collect::<Vec<_>>(), "backlog": backlog, "days": list}))
        }

        // edit a task from the phone: title, date (null = backlog), time (null = anytime), category, notes
        (Method::Post, "/api/task/update") => {
            let id = v["id"].as_str().ok_or_else(|| bad("Missing task"))?;
            let mut d = shared.lock().unwrap();
            let i = d.item_index(id).ok_or_else(|| bad("That task isn't there any more"))?;
            let it = &mut d.items[i];
            if let Some(t) = v["title"].as_str() {
                let t = t.trim();
                if t.is_empty() {
                    return Err(bad("The title can't be empty"));
                }
                it.title = t.to_string();
            }
            match v.get("date") {
                Some(Value::Null) => it.date = None,
                Some(Value::String(s)) => it.date = Some(NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| bad("Pick a date"))?),
                _ => {}
            }
            match v.get("time") {
                Some(Value::Null) => it.time = None,
                Some(Value::String(s)) if s.is_empty() => it.time = None,
                Some(Value::String(s)) => it.time = Some(NaiveTime::parse_from_str(s, "%H:%M").map_err(|_| bad("Use a time like 18:30"))?),
                _ => {}
            }
            if let Some(k) = v["kind"].as_str().map(str::trim).filter(|k| !k.is_empty()) {
                it.kind = k.to_lowercase();
            }
            if let Some(n) = v["notes"].as_str() {
                it.notes = n.trim().to_string();
            }
            it.touch();
            let title = it.title.clone();
            d.save_items();
            done(&mut d, format!("saved \"{title}\""))
        }

        (Method::Post, "/api/task/delete") => {
            let id = v["id"].as_str().ok_or_else(|| bad("Missing task"))?;
            let mut d = shared.lock().unwrap();
            let i = d.item_index(id).ok_or_else(|| bad("That task isn't there any more"))?;
            let title = d.items[i].title.clone();
            d.delete_item(id);
            d.save_items();
            done(&mut d, format!("deleted \"{title}\""))
        }

        // the food library: every ingredient and meal, with meals' computed numbers
        (Method::Get, "/api/foods") => {
            let d = shared.lock().unwrap();
            let lib = &d.foods;
            let meals: Vec<Value> = lib
                .meals
                .iter()
                .map(|m| {
                    let p = lib.meal_portionable(m);
                    let (_, total_g) = lib.meal_totals(m);
                    let mut v = serde_json::to_value(m).unwrap_or(Value::Null);
                    v["kcal_serving"] = json!(p.kcal(crate::foods::Amount::Servings(1.0)));
                    v["serving_g"] = json!(p.serving_g.round());
                    v["kcal_100g"] = json!(p.per100.kcal.round());
                    v["total_g"] = json!(total_g.round());
                    v
                })
                .collect();
            Ok(json!({"foods": lib.foods, "meals": meals}))
        }

        // add or change an ingredient ("food") or a meal; no id = new
        (Method::Post, "/api/foods/save") => {
            let item = &v["item"];
            let name = item["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("Give it a name"))?.chars().take(60).collect::<String>();
            let num = |k: &str, max: f64| item[k].as_f64().filter(|n| n.is_finite() && *n >= 0.0 && *n <= max).map(|n| n as f32);
            let text = |k: &str| item[k].as_str().unwrap_or("").trim().to_string();
            let aliases: Vec<String> = item["aliases"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
            let id = item["id"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
            let mut d = shared.lock().unwrap();
            let saved_id = if v["kind"].as_str() == Some("meal") {
                let ingredients: Vec<crate::foods::Ingredient> = item["ingredients"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|g| Some(crate::foods::Ingredient { food: g["food"].as_str()?.to_string(), grams: g["grams"].as_f64().filter(|x| *x > 0.0 && *x <= 20000.0)? as f32 }))
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(g) = ingredients.iter().find(|g| d.foods.food(&g.food).is_none()) {
                    return Err(bad(format!("Ingredient {} isn't in your library", g.food)));
                }
                let mut m = id.as_deref().and_then(|i| d.foods.meals.iter().find(|m| m.id == i).cloned()).unwrap_or_else(|| crate::foods::Meal { id: uuid::Uuid::new_v4().to_string(), ..Default::default() });
                m.name = name;
                m.aliases = aliases;
                m.ingredients = ingredients;
                m.servings = num("servings", 100.0).filter(|s| *s > 0.0).unwrap_or(1.0);
                m.cooked_g = num("cooked_g", 50000.0).filter(|g| *g > 0.0);
                m.favorite = item["favorite"].as_bool().unwrap_or(false);
                m.notes = text("notes");
                let id = m.id.clone();
                match d.foods.meals.iter_mut().find(|x| x.id == id) {
                    Some(x) => *x = m,
                    None => d.foods.meals.push(m),
                }
                id
            } else {
                let kcal = num("kcal_per_100g", 900.0).ok_or_else(|| bad("Calories per 100 g must be a number from 0 to 900"))?;
                let mut f = id.as_deref().and_then(|i| d.foods.food(i).cloned()).unwrap_or_else(|| crate::foods::Food { id: uuid::Uuid::new_v4().to_string(), ..Default::default() });
                f.name = name;
                f.aliases = aliases;
                f.kcal_per_100g = kcal;
                f.serving_g = num("serving_g", 5000.0).filter(|g| *g >= 1.0).unwrap_or(100.0);
                f.serving_name = Some(text("serving_name").to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "serving".into());
                f.protein_per_100g = num("protein_per_100g", 100.0);
                f.carbs_per_100g = num("carbs_per_100g", 100.0);
                f.fat_per_100g = num("fat_per_100g", 100.0);
                f.favorite = item["favorite"].as_bool().unwrap_or(false);
                f.notes = text("notes");
                let id = f.id.clone();
                match d.foods.foods.iter_mut().find(|x| x.id == id) {
                    Some(x) => *x = f,
                    None => d.foods.foods.push(f),
                }
                id
            };
            d.save_foods();
            let r = done(&mut d, "saved to your foods".into())?;
            Ok(json!({"ok": true, "message": r["message"], "id": saved_id}))
        }

        (Method::Post, "/api/foods/delete") => {
            let id = v["id"].as_str().ok_or_else(|| bad("Missing food"))?;
            let mut d = shared.lock().unwrap();
            let name = if let Some(i) = d.foods.foods.iter().position(|f| f.id == id) {
                let f = d.foods.foods.remove(i);
                // like on the PC: it's taken out of meals that used it
                for m in &mut d.foods.meals {
                    m.ingredients.retain(|g| g.food != f.id);
                }
                f.name
            } else if let Some(i) = d.foods.meals.iter().position(|m| m.id == id) {
                d.foods.meals.remove(i).name
            } else {
                return Err(bad("That food isn't in your library any more"));
            };
            d.save_foods();
            done(&mut d, format!("deleted {name}"))
        }

        // Train tab: workouts and recent sessions
        (Method::Get, "/api/workouts") => {
            let d = shared.lock().unwrap();
            let lib = &d.exercises.exercises;
            let workouts: Vec<Value> = d
                .workouts
                .workouts
                .iter()
                .filter(|w| !w.items.is_empty())
                .map(|w| {
                    json!({"id": w.id, "name": w.name, "notes": w.notes, "seconds": w.seconds(lib), "exercises": w.exercise_count(),
                           "equipment": w.equipment(lib).iter().map(|q| crate::workouts::equipment_label(q).to_lowercase()).collect::<Vec<_>>(),
                           "task": w.task.as_deref().and_then(|id| d.items.iter().find(|i| i.id == id)).map(|i| i.title.clone())})
                })
                .collect();
            let mut sessions: Vec<&crate::workouts::Session> = d.workout_log.sessions.iter().collect();
            sessions.sort_by(|a, b| (b.date, &b.start).cmp(&(a.date, &a.start)));
            let history: Vec<Value> = sessions
                .iter()
                .take(30)
                .map(|s| {
                    let volume: f32 = s.sets.iter().map(|x| x.reps.unwrap_or(0) as f32 * x.kg.unwrap_or(0.0)).sum();
                    json!({"id": s.id, "date": s.date.to_string(), "start": s.start, "duration_s": s.duration_s, "workout": s.workout,
                           "sets": s.sets, "completed": s.completed, "volume": volume.round()})
                })
                .collect();
            Ok(json!({"workouts": workouts, "history": history}))
        }

        // one workout, ready to play: its steps and how to do each exercise
        (Method::Get, "/api/workout") => {
            let id = q.get("id").ok_or_else(|| bad("Missing workout"))?;
            let d = shared.lock().unwrap();
            let lib = &d.exercises.exercises;
            let w = d.workouts.workouts.iter().find(|w| w.id == *id).ok_or_else(|| bad("That workout isn't there any more"))?;
            let steps: Vec<Value> = w
                .steps(lib)
                .into_iter()
                .map(|s| match s {
                    crate::workouts::Step::Set { exercise, name, set, sets, reps, seconds, kg, per_side } => {
                        json!({"type": "set", "exercise": exercise, "name": name, "set": set, "sets": sets, "reps": reps, "seconds": seconds, "kg": kg, "per_side": per_side})
                    }
                    crate::workouts::Step::Rest { seconds, label } => json!({"type": "rest", "seconds": seconds, "label": label}),
                })
                .collect();
            let mut exercises = serde_json::Map::new();
            for item in &w.items {
                if let crate::workouts::Item::Exercise { exercise, .. } = item {
                    if let Some(e) = lib.iter().find(|e| e.id == *exercise) {
                        exercises.insert(e.id.clone(), json!({"name": e.name, "steps": e.steps, "form": e.form, "mistakes": e.mistakes, "easier": e.easier,
                            "harder": e.harder, "muscles_main": e.muscles_main, "muscles_helping": e.muscles_helping,
                            "equipment": e.equipment.iter().map(|q| crate::workouts::equipment_label(q)).collect::<Vec<_>>()}));
                    }
                }
            }
            let items: Vec<Value> = w
                .items
                .iter()
                .map(|it| match it {
                    crate::workouts::Item::Exercise { exercise, sets, reps, seconds, kg, rest_s } => {
                        let e = lib.iter().find(|e| e.id == *exercise);
                        json!({"type": "exercise", "exercise": exercise, "name": e.map_or(exercise.as_str(), |e| e.name.as_str()), "sets": sets, "reps": reps,
                               "seconds": seconds, "kg": kg, "rest_s": rest_s, "per_side": e.is_some_and(|e| e.kind == "reps_per_side")})
                    }
                    crate::workouts::Item::Break { seconds, label } => json!({"type": "break", "seconds": seconds, "label": label}),
                })
                .collect();
            Ok(json!({"id": w.id, "name": w.name, "notes": w.notes, "task": w.task, "seconds": w.seconds(lib), "items": items, "steps": steps, "exercises": exercises}))
        }

        // a workout done on the phone
        (Method::Post, "/api/workout/session") => {
            let sets: Vec<crate::workouts::SetDone> = serde_json::from_value(v["sets"].clone()).map_err(|e| bad(format!("sets: {e}")))?;
            if sets.is_empty() {
                return Err(bad("No sets were done"));
            }
            let workout_id = v["workout_id"].as_str().unwrap_or("").to_string();
            let day = day_of(v, q)?;
            let mut d = shared.lock().unwrap();
            let (name, task) = match d.workouts.workouts.iter().find(|w| w.id == workout_id) {
                Some(w) => (w.name.clone(), w.task.clone()),
                None => (v["workout"].as_str().unwrap_or("Workout").to_string(), None),
            };
            let session = crate::workouts::Session {
                id: uuid::Uuid::new_v4().to_string(),
                date: day,
                start: v["start"].as_str().filter(|s| NaiveTime::parse_from_str(s, "%H:%M").is_ok()).unwrap_or("00:00").to_string(),
                duration_s: v["duration_s"].as_u64().unwrap_or(0).min(86_400) as u32,
                workout_id,
                workout: name,
                sets,
                completed: v["completed"].as_bool().unwrap_or(true),
            };
            let msg = d.record_workout(session, task.as_deref());
            done(&mut d, msg)
        }

        (Method::Post, "/api/calories/delete") => {
            let id = v["id"].as_str().ok_or_else(|| bad("Missing entry"))?;
            let mut d = shared.lock().unwrap();
            let before = d.calories.len();
            d.calories.retain(|c| c.id != id);
            if d.calories.len() == before {
                return Err(bad("That entry is already gone"));
            }
            d.save_calories();
            done(&mut d, "calorie entry removed".into())
        }

        (Method::Post, "/api/task/toggle") => {
            let id = v["id"].as_str().ok_or_else(|| bad("Missing task"))?;
            // a later day is fine for a one-off task finished early (it counts as done today);
            // a repeating one is ticked on its own day
            let later = v["date"].as_str().and_then(|x| NaiveDate::parse_from_str(x, "%Y-%m-%d").ok()).is_some_and(|x| x > today);
            let day = if later { today } else { day_of(v, q)? };
            let mut d = shared.lock().unwrap();
            let i = d.item_index(id).ok_or_else(|| bad("That task isn't there any more"))?;
            if later && d.items[i].repeat.is_some() {
                return Err(bad("Tick off a repeating task on its day"));
            }
            d.items[i].toggle_done(day);
            let (title, now_done) = (d.items[i].title.clone(), d.items[i].is_done_on(day));
            d.save_items();
            done(&mut d, format!("{} {title}", if now_done { "✔" } else { "un-ticked" }))
        }

        (Method::Post, "/api/journal") => {
            let text = v["text"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return Err(bad("Write something first"));
            }
            let mut e = crate::journal::JournalEntry::new(text);
            e.mood = v["mood"].as_u64().filter(|m| (1..=5).contains(m)).map(|m| m as u8);
            e.date = day_of(v, q)?;
            e.time = time_of(v, q).or(if e.date == today { e.time } else { None });
            let mut d = shared.lock().unwrap();
            d.journal.push(e);
            d.save_journal();
            done(&mut d, "journal entry saved".into())
        }

        (Method::Post, "/api/journal/audio") => journal_audio(q, body, shared, ui),

        (Method::Post, "/api/ask") => {
            let text = v["text"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return Err(bad("Type what you want Claude to do"));
            }
            let food = v["food"].as_bool().unwrap_or(false);
            let (cli, prompt, model, dir) = {
                let d = shared.lock().unwrap();
                let cli = crate::claude::find_cli(&d.settings).ok_or_else(|| bad("Claude Code wasn't found on your PC"))?;
                let request = if food { format!("Log the calories for what I ate/drank (estimate high): {text}") } else { text.clone() };
                (cli, crate::claude::build_prompt(&d, &request), d.settings.claude_model.clone(), d.paths.root.clone())
            };
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::claude::ask(&cli, &model, &prompt, &dir));
            });
            let id = uuid::Uuid::new_v4().simple().to_string();
            JOBS.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), AskJob { rx: Some(rx), result: None });
            Ok(json!({"job": id}))
        }

        (Method::Get, "/api/ask") => {
            let id = q.get("job").ok_or_else(|| bad("Missing job"))?;
            let mut jobs = JOBS.lock().unwrap();
            let job = jobs.get_or_insert_with(HashMap::new).get_mut(id).ok_or_else(|| bad("That request is gone"))?;
            if job.result.is_none() {
                if let Some(rx) = &job.rx {
                    if let Ok(r) = rx.try_recv() {
                        job.result = Some(r);
                        job.rx = None;
                    }
                }
            }
            match &job.result {
                None => Ok(json!({"status": "running"})),
                Some(Err(e)) => Ok(json!({"status": "error", "error": e})),
                Some(Ok(text)) => {
                    let d = shared.lock().unwrap();
                    let plan = crate::import::plan(text, &d.items, &d.calories, &d.settings, &d.foods, &d.exercises.exercises, today);
                    let lines: Vec<String> = plan.ops.iter().map(|o| o.describe()).collect();
                    Ok(json!({"status": "done", "message": plan.message, "lines": lines, "errors": plan.errors, "can_apply": plan.errors.is_empty() && !plan.ops.is_empty()}))
                }
            }
        }

        (Method::Post, "/api/ask/apply") => {
            let id = v["job"].as_str().ok_or_else(|| bad("Missing job"))?;
            let text = {
                let mut jobs = JOBS.lock().unwrap();
                match jobs.get_or_insert_with(HashMap::new).remove(id) {
                    Some(AskJob { result: Some(Ok(t)), .. }) => t,
                    _ => return Err(bad("That reply isn't ready or was already applied")),
                }
            };
            let mut d = shared.lock().unwrap();
            let plan = crate::import::plan(&text, &d.items, &d.calories, &d.settings, &d.foods, &d.exercises.exercises, today);
            if !plan.errors.is_empty() {
                return Err(bad(plan.errors.join("; ")));
            }
            let ops = plan
                .ops
                .into_iter()
                .map(|op| match op {
                    crate::import::Op::LogCalories(mut c) => {
                        c.source = "claude".into();
                        crate::import::Op::LogCalories(c)
                    }
                    other => other,
                })
                .collect();
            let summary = crate::import::apply(&mut d, ops);
            done(&mut d, format!("Claude: {summary}"))
        }

        (Method::Post, "/api/ask/discard") => {
            if let Some(id) = v["job"].as_str() {
                JOBS.lock().unwrap().get_or_insert_with(HashMap::new).remove(id);
            }
            Ok(json!({"ok": true}))
        }

        _ => Err((404, "Unknown request".into())),
    }
}

/// A voice journal entry: the phone sends a 16 kHz mono 16-bit WAV. It's saved at once and
/// transcribed in the background with Whisper.
fn journal_audio(q: &HashMap<String, String>, body: Vec<u8>, shared: &Arc<Mutex<Data>>, ui: &UiHandle) -> ApiResult {
    let today = Local::now().date_naive();
    let le16 = |o: usize| u16::from_le_bytes([body[o], body[o + 1]]);
    let le32 = |o: usize| u32::from_le_bytes([body[o], body[o + 1], body[o + 2], body[o + 3]]);
    if body.len() < 44 || &body[..4] != b"RIFF" || &body[8..12] != b"WAVE" || le16(22) != 1 || le32(24) != 16_000 || le16(34) != 16 {
        return Err(bad("The recording isn't in the expected format"));
    }
    // the samples start after the "data" chunk header (usually at byte 44, later if the
    // format chunk is longer)
    let data_at = (12..body.len().saturating_sub(8)).find(|&i| &body[i..i + 4] == b"data").map(|i| i + 8).unwrap_or(44);
    let samples: Vec<f32> = body[data_at..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
    if samples.len() < 8000 {
        return Err(bad("The recording is too short"));
    }
    let empty = Value::Null;
    let mut e = crate::journal::JournalEntry::new(q.get("text").map(|t| t.trim().to_string()).unwrap_or_default());
    e.mood = q.get("mood").and_then(|m| m.parse::<u8>().ok()).filter(|m| (1..=5).contains(m));
    e.date = day_of(&empty, q)?;
    e.time = time_of(&empty, q).or(if e.date == today { e.time } else { None });
    let file = format!("{}_phone.wav", Local::now().format("%Y-%m-%d_%H%M%S"));
    let (model, lang, id) = {
        let mut d = shared.lock().unwrap();
        let path = d.paths.journal_audio.join(&file);
        let _ = std::fs::create_dir_all(&d.paths.journal_audio);
        std::fs::write(&path, &body).map_err(|err| (500, format!("Couldn't save the recording: {err}")))?;
        e.audio = Some(file);
        e.audio_secs = Some(samples.len() as f32 / 16_000.0);
        let id = e.id.clone();
        d.journal.push(e);
        d.save_journal();
        d.status = "Phone: voice journal entry saved, transcribing…".into();
        repaint(ui);
        (d.paths.models.join(&d.settings.whisper_model), d.settings.voice_language.clone(), id)
    };
    if !cfg!(feature = "voice") || !model.exists() {
        return Ok(json!({"ok": true, "message": "Recording saved. Download a voice model on your PC (Settings > Input & voice) to transcribe it."}));
    }
    TRANSCRIBING.lock().unwrap().get_or_insert_with(HashSet::new).insert(id.clone());
    #[cfg(feature = "voice")]
    {
        let (shared, ui) = (shared.clone(), ui.clone());
        std::thread::spawn(move || {
            let r = crate::voice::transcribe(&model, &samples, &lang);
            let mut d = shared.lock().unwrap();
            if let Some(e) = d.journal.iter_mut().find(|e| e.id == id) {
                match r {
                    Ok(text) => e.transcript = Some(crate::journal::clean_transcript(&text)),
                    Err(err) => d.status = format!("Phone recording: transcription failed: {err}"),
                }
                d.save_journal();
            }
            TRANSCRIBING.lock().unwrap().get_or_insert_with(HashSet::new).remove(&id);
            repaint(&ui);
        });
    }
    #[cfg(not(feature = "voice"))]
    let _ = lang;
    Ok(json!({"ok": true, "message": "Recording saved, transcribing on your PC"}))
}

fn hex(c: eframe::egui::Color32) -> String {
    crate::categories::to_hex(c)
}

/// Everything the phone's screens show, in one request.
/// Sends a test notification to one phone (in the background).
pub fn send_test(shared: &Arc<Mutex<Data>>, device: &str) -> Result<(), (u16, String)> {
    let (key, sub) = {
        let d = shared.lock().unwrap();
        let sub = d.settings.phone_devices.iter().find(|p| p.id == device).and_then(|p| p.push.clone());
        (d.settings.push_key.clone(), sub)
    };
    let sub = sub.ok_or_else(|| bad("Notifications aren't on for this phone yet"))?;
    let payload = json!({"title": "Morrowkeep", "body": "Test notification. Reminders will arrive like this.", "tag": "test", "go": "today"});
    let shared = shared.clone();
    let device = device.to_string();
    std::thread::spawn(move || {
        let r = crate::push::send(&key, &sub, &payload);
        let mut d = shared.lock().unwrap();
        d.status = match r {
            Ok(()) => "Test notification sent to your phone".into(),
            Err(crate::push::SendError::Gone) => {
                forget_push(&mut d, &device);
                "That phone's notifications were turned off; turn them on again in its app".into()
            }
            Err(crate::push::SendError::Other(e)) => format!("Couldn't send the test notification: {e}"),
        };
    });
    Ok(())
}

fn forget_push(d: &mut Data, device: &str) {
    if let Some(dev) = d.settings.phone_devices.iter_mut().find(|p| p.id == device) {
        dev.push = None;
        d.save_settings();
    }
}

/// Phones that have notifications on.
pub fn has_push(d: &Data) -> bool {
    d.settings.phone_devices.iter().any(|p| p.push.is_some())
}

/// Sends a notification to every phone that has them on. Runs the network part without
/// holding the data lock; subscriptions the push service reports as gone are dropped.
pub fn push_all(shared: &Arc<Mutex<Data>>, payload: &Value) -> usize {
    let (key, subs) = {
        let d = shared.lock().unwrap();
        let subs: Vec<(String, crate::push::Subscription)> = d.settings.phone_devices.iter().filter_map(|p| p.push.clone().map(|s| (p.id.clone(), s))).collect();
        (d.settings.push_key.clone(), subs)
    };
    if key.is_empty() {
        return 0;
    }
    let mut sent = 0;
    for (id, sub) in subs {
        match crate::push::send(&key, &sub, payload) {
            Ok(()) => sent += 1,
            Err(crate::push::SendError::Gone) => forget_push(&mut shared.lock().unwrap(), &id),
            Err(crate::push::SendError::Other(_)) => {}
        }
    }
    sent
}

/// A task for the phone, as it is on `day`.
fn item_json(it: &crate::model::Item, day: NaiveDate) -> Value {
    json!({"id": it.id, "title": it.title, "date": it.date.map(|x| x.to_string()), "day": day.to_string(), "time": it.time.map(|t| t.format("%H:%M").to_string()),
           "duration": it.duration_min, "done": it.is_done_on(day), "kind": it.kind, "color": hex(crate::ui::kind_color(&it.kind)),
           "icon": crate::categories::icon(&it.kind), "repeat": it.repeat, "notes": it.notes})
}

fn state(d: &Data, device: &str) -> Value {
    let now = Local::now();
    let today = now.date_naive();
    let th = crate::ui::theme::th();
    let theme = json!({
        "dark": th.dark,
        "page": hex(th.surface.page), "card": hex(th.surface.card), "border": hex(th.surface.card_border), "input": hex(th.surface.input),
        "text": hex(th.text.primary), "muted": hex(th.text.muted), "on_color": hex(th.text.on_color),
        "fill": hex(th.widget.fill), "selection": hex(th.widget.selection), "accent": hex(th.widget.selection_border),
        "success": hex(th.status.success), "danger": hex(th.status.danger), "warning": hex(th.status.warning),
        "brand": hex(th.data.brand), "calories": hex(crate::ui::calorie_color()), "weight": hex(crate::ui::weight_color()),
        "journal": hex(th.data.journal), "goal": hex(th.data.goal), "claude": hex(th.data.claude),
        "workout": hex(crate::ui::kind_color("workout")), "rest": hex(th.data.rest), "trend": hex(th.data.trend),
    });

    // tasks: scheduled first, then anytime (same order as the Habits board)
    let mut occ: Vec<usize> = crate::model::occurrences(&d.items, today, today).into_iter().map(|(_, i)| i).collect();
    occ.sort_by_key(|i| (d.items[*i].time.is_none(), d.items[*i].time));
    let tasks: Vec<Value> = occ
        .iter()
        .map(|i| {
            let it = &d.items[*i];
            json!({"id": it.id, "title": it.title, "time": it.time.map(|t| t.format("%H:%M").to_string()), "done": it.is_done_on(today),
                   "color": hex(crate::ui::kind_color(&it.kind)), "icon": crate::categories::icon(&it.kind), "repeat": it.repeat.is_some()})
        })
        .collect();

    let entries: Vec<Value> = d
        .calories
        .iter()
        .filter(|c| c.date == today)
        .map(|c| json!({"id": c.id, "time": c.time.map(|t| t.format("%H:%M").to_string()), "kcal": c.kcal, "description": c.description}))
        .collect();

    let last = d.weight_before(today).map(|w| json!({"kg": w.kg, "date": w.date.format("%b %-d").to_string()}));
    let goal = d.body.goal.as_ref().and_then(|g| {
        let p = crate::body::progress(g, &d.weights, today)?;
        (g.start_kg > g.target_kg).then(|| {
            if p.to_go <= 0.0 {
                format!("Goal {:.1} kg reached", g.target_kg)
            } else {
                format!("{:.1} kg to go to {:.1} kg · {}", p.to_go, g.target_kg, p.status())
            }
        })
    });

    let transcribing = TRANSCRIBING.lock().unwrap().clone().unwrap_or_default();
    let mut recent: Vec<&crate::journal::JournalEntry> = d.journal.iter().filter(|e| e.date >= today - Duration::days(2)).collect();
    recent.sort_by_key(|e| std::cmp::Reverse(crate::journal::sort_key(e)));
    let journal: Vec<Value> = recent
        .iter()
        .map(|e| {
            json!({"id": e.id, "date": e.date.to_string(), "time": e.time.map(|t| t.format("%H:%M").to_string()), "text": e.text,
                   "transcript": e.transcript, "mood": e.mood, "mood_label": e.mood.map(crate::journal::mood_label),
                   "audio_secs": e.audio_secs, "transcribing": transcribing.contains(&e.id), "kind": e.kind})
        })
        .collect();

    let favorite = |id: &str| d.foods.food(id).map(|f| f.favorite).or_else(|| d.foods.meals.iter().find(|m| m.id == id).map(|m| m.favorite)).unwrap_or(false);
    let foods: Vec<Value> = d
        .foods
        .all()
        .iter()
        .map(|p| {
            json!({"id": p.id, "name": p.name, "meal": p.is_meal, "kcal_serving": p.kcal(crate::foods::Amount::Servings(1.0)),
                   "serving_name": p.serving_name, "serving_g": p.serving_g, "kcal_100g": p.per100.kcal, "favorite": favorite(&p.id)})
        })
        .collect();
    let mut recent_foods: Vec<String> = Vec::new();
    let mut logged: Vec<&crate::model::CalorieEntry> = d.calories.iter().filter(|c| c.food.is_some()).collect();
    logged.sort_by_key(|c| std::cmp::Reverse((c.date, c.time)));
    for c in logged {
        if let Some(p) = c.food.as_deref().and_then(|n| d.foods.find(n)) {
            if !recent_foods.contains(&p.id) {
                recent_foods.push(p.id);
            }
        }
        if recent_foods.len() >= 6 {
            break;
        }
    }

    json!({
        "date": today.to_string(),
        "date_label": now.format("%a %-d %b").to_string(),
        "theme": theme,
        "calories": {"total": d.calories_on(today), "limit": d.daily_limit(), "recommended": d.recommended_for(today), "entries": entries},
        "weight": {"today": d.weight_on(today), "last": last, "goal": goal},
        "journal": {"today": d.journal_on(today), "streak": crate::journal::streak(&d.journal, today),
                    "prompt": crate::journal::PROMPTS[today.format("%j").to_string().parse::<usize>().unwrap_or(0) % crate::journal::PROMPTS.len()],
                    "recent": journal},
        "tasks": tasks,
        "foods": foods,
        "recent_foods": recent_foods,
        "claude": crate::claude::find_cli(&d.settings).is_some(),
        "categories": crate::categories::names().iter().map(|k| json!({"name": k, "icon": crate::categories::icon(k), "color": hex(crate::ui::kind_color(k))})).collect::<Vec<_>>(),
        "notifications": d.settings.phone_devices.iter().any(|p| p.id == device && p.push.is_some()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_and_tokens() {
        assert_eq!(url_decode("a%20b+c%C3%A5"), "a b cå");
        assert_eq!(query("/api/x?date=2026-10-16&text=hi%21")["text"], "hi!");
        assert_eq!(sha("abc").len(), 64);
        let c = new_pair_code();
        assert_eq!(c.len(), 8);
        assert_eq!(pair_code().unwrap().0, c);
    }
}
