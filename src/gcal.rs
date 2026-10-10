//! Google Calendar mirror.
//!
//! Local files are the source of truth. Items are pushed (create/update/delete) into a
//! dedicated calendar, so Morrowkeep never touches your other events. Events from the
//! calendars in `pull_calendars` are fetched read-only and shown alongside your items.

use crate::model::{ExtEvent, Item};
use crate::store::{read_json, write_json, Data, Settings};
use base64::Engine;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API: &str = "https://www.googleapis.com/calendar/v3";
const SCOPE: &str = "https://www.googleapis.com/auth/calendar";

#[derive(Serialize, Deserialize)]
struct Token {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
}

enum ApiErr {
    Status(u16, String),
    Other(String),
}

impl std::fmt::Display for ApiErr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ApiErr::Status(c, body) => {
                let msg = serde_json::from_str::<Value>(body)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(String::from))
                    .unwrap_or_else(|| body.chars().take(200).collect());
                write!(f, "Google API error {c}: {msg}")
            }
            ApiErr::Other(s) => write!(f, "{s}"),
        }
    }
}

fn map_err(e: ureq::Error) -> ApiErr {
    match e {
        ureq::Error::Status(code, resp) => ApiErr::Status(code, resp.into_string().unwrap_or_default()),
        other => ApiErr::Other(format!("network error: {other}")),
    }
}

/// Percent-encodes a path segment (calendar ids contain '@' and '#').
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 3;
                    continue;
                }
                Err(_) => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------- OAuth

/// Runs the browser sign-in flow (loopback redirect + PKCE) and stores the token.
/// Blocks for up to 5 minutes; call from a background thread.
pub fn connect(settings: &Settings, token_path: &Path) -> Result<(), String> {
    if settings.google_client_id.trim().is_empty() || settings.google_client_secret.trim().is_empty() {
        return Err("Enter your Google OAuth client id and secret in Settings first (see README).".into());
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = uuid::Uuid::new_v4().simple().to_string();
    let url = format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        enc(settings.google_client_id.trim()),
        enc(&redirect),
        enc(SCOPE)
    );
    open::that(&url).map_err(|e| format!("couldn't open browser: {e}"))?;

    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let code = loop {
        if std::time::Instant::now() > deadline {
            return Err("Timed out waiting for Google sign-in".into());
        }
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let path = req.split_whitespace().nth(1).unwrap_or("");
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let params: Vec<(String, String)> = query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), pct_decode(v)))
            .collect();
        let get = |k: &str| params.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone());
        let (ok, body) = match (get("code"), get("error")) {
            (Some(_), _) if get("state").as_deref() != Some(&state) => (false, "State mismatch, please try again."),
            (Some(_), _) => (true, "Morrowkeep is connected to Google Calendar. You can close this tab."),
            (None, Some(_)) => (false, "Google sign-in was cancelled or failed. You can close this tab."),
            (None, None) => {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                continue; // e.g. favicon request
            }
        };
        let html = format!("<html><body style=\"font-family:sans-serif;padding:40px\"><h2>{body}</h2></body></html>");
        let _ = stream.write_all(
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len()).as_bytes(),
        );
        if !ok {
            return Err(get("error").unwrap_or_else(|| body.to_string()));
        }
        break get("code").unwrap();
    };

    let resp: Value = ureq::post(TOKEN_URL)
        .send_form(&[
            ("code", code.as_str()),
            ("client_id", settings.google_client_id.trim()),
            ("client_secret", settings.google_client_secret.trim()),
            ("redirect_uri", redirect.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
        ])
        .map_err(|e| map_err(e).to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let token = Token {
        access_token: resp["access_token"].as_str().ok_or("no access_token in response")?.to_string(),
        refresh_token: resp["refresh_token"].as_str().ok_or("no refresh_token in response")?.to_string(),
        expires_at: Utc::now().timestamp() + resp["expires_in"].as_i64().unwrap_or(3600),
    };
    write_json(token_path, &token)
}

fn access_token(settings: &Settings, token_path: &Path) -> Result<String, ApiErr> {
    let mut token: Token = read_json(token_path)
        .map_err(ApiErr::Other)?
        .ok_or_else(|| ApiErr::Other("not connected to Google".into()))?;
    if Utc::now().timestamp() < token.expires_at - 60 {
        return Ok(token.access_token);
    }
    let resp: Value = ureq::post(TOKEN_URL)
        .send_form(&[
            ("client_id", settings.google_client_id.trim()),
            ("client_secret", settings.google_client_secret.trim()),
            ("refresh_token", token.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .map_err(map_err)?
        .into_json()
        .map_err(|e| ApiErr::Other(e.to_string()))?;
    token.access_token = resp["access_token"].as_str().unwrap_or_default().to_string();
    token.expires_at = Utc::now().timestamp() + resp["expires_in"].as_i64().unwrap_or(3600);
    write_json(token_path, &token).map_err(ApiErr::Other)?;
    Ok(token.access_token)
}

// ---------------------------------------------------------------- API helpers

struct Client {
    token: String,
}

impl Client {
    fn call(&self, method: &str, url: &str, query: &[(&str, &str)], body: Option<&Value>) -> Result<Value, ApiErr> {
        let mut req = ureq::request(method, url).set("Authorization", &format!("Bearer {}", self.token));
        for (k, v) in query {
            req = req.query(k, v);
        }
        let resp = match body {
            Some(b) => req.send_json(b.clone()),
            None => req.call(),
        }
        .map_err(map_err)?;
        let text = resp.into_string().map_err(|e| ApiErr::Other(e.to_string()))?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| ApiErr::Other(e.to_string()))
    }
}

fn ensure_calendar(c: &Client, name: &str, known: Option<&str>, tz: &str) -> Result<String, ApiErr> {
    if let Some(id) = known {
        match c.call("GET", &format!("{API}/calendars/{}", enc(id)), &[], None) {
            Ok(_) => return Ok(id.to_string()),
            Err(ApiErr::Status(404, _)) | Err(ApiErr::Status(410, _)) => {}
            Err(e) => return Err(e),
        }
    }
    let mut page: Option<String> = None;
    loop {
        let mut q = vec![("minAccessRole", "owner")];
        if let Some(p) = &page {
            q.push(("pageToken", p.as_str()));
        }
        let list = c.call("GET", &format!("{API}/users/me/calendarList"), &q, None)?;
        if let Some(found) = list["items"].as_array().and_then(|a| a.iter().find(|cal| cal["summary"] == name)) {
            return Ok(found["id"].as_str().unwrap_or_default().to_string());
        }
        match list["nextPageToken"].as_str() {
            Some(p) => page = Some(p.to_string()),
            None => break,
        }
    }
    let created = c.call("POST", &format!("{API}/calendars"), &[], Some(&json!({ "summary": name, "timeZone": tz })))?;
    Ok(created["id"].as_str().unwrap_or_default().to_string())
}

fn color_id(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "workout" => "10", // basil
        "meal" => "6",     // tangerine
        "chore" => "5",    // banana
        "task" => "9",     // blueberry
        "habit" => "3",    // grape
        "event" => "7",    // peacock
        _ => return None,
    })
}

fn event_body(item: &Item, tz: &str) -> Value {
    let rule = item.repeat_rule();
    let start_date = item.date.unwrap();
    let first = rule.as_ref().map_or(start_date, |r| r.first_occurrence(start_date));
    let (start, end) = match item.time {
        Some(t) => {
            let s = first.and_time(t);
            let e = s + Duration::minutes(item.duration_min.unwrap_or(30) as i64);
            (
                json!({ "dateTime": s.format("%Y-%m-%dT%H:%M:%S").to_string(), "timeZone": tz }),
                json!({ "dateTime": e.format("%Y-%m-%dT%H:%M:%S").to_string(), "timeZone": tz }),
            )
        }
        None => (
            json!({ "date": first.to_string() }),
            json!({ "date": first.succ_opt().unwrap().to_string() }),
        ),
    };
    let mut title = item.title.clone();
    if item.done && rule.is_none() {
        title = format!("✓ {title}");
    }
    let mut desc = item.notes.clone();
    if !desc.is_empty() {
        desc += "\n\n";
    }
    desc += &format!("[Morrowkeep {}] Edit this item in Morrowkeep; changes made here are overwritten.", item.kind);
    let mut body = json!({
        "summary": title,
        "description": desc,
        "start": start,
        "end": end,
        "extendedProperties": { "private": { "morrowkeep_id": item.id } },
    });
    if let Some(c) = color_id(&item.kind) {
        body["colorId"] = json!(c);
    }
    if let Some(r) = rule {
        let mut rrule = r.rrule();
        if let Some(u) = item.until {
            if item.time.is_some() {
                let local_end = u.and_hms_opt(23, 59, 59).unwrap();
                let utc = Local.from_local_datetime(&local_end).earliest().map(|d| d.with_timezone(&Utc));
                if let Some(utc) = utc {
                    rrule += &format!(";UNTIL={}", utc.format("%Y%m%dT%H%M%SZ"));
                }
            } else {
                rrule += &format!(";UNTIL={}", u.format("%Y%m%d"));
            }
        }
        body["recurrence"] = json!([rrule]);
    }
    body
}

fn parse_event_time(v: &Value) -> Option<(NaiveDateTime, bool)> {
    if let Some(dt) = v["dateTime"].as_str() {
        let dt = DateTime::parse_from_rfc3339(dt).ok()?;
        return Some((dt.with_timezone(&Local).naive_local(), false));
    }
    let d = NaiveDate::parse_from_str(v["date"].as_str()?, "%Y-%m-%d").ok()?;
    Some((d.and_hms_opt(0, 0, 0)?, true))
}

fn pull_events(c: &Client, calendar: &str, from: NaiveDate, to: NaiveDate) -> Result<Vec<ExtEvent>, ApiErr> {
    let time_min = Local.from_local_datetime(&from.and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap().to_rfc3339();
    let time_max = Local.from_local_datetime(&to.and_hms_opt(23, 59, 59).unwrap()).earliest().unwrap().to_rfc3339();
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut q = vec![
            ("timeMin", time_min.as_str()),
            ("timeMax", time_max.as_str()),
            ("singleEvents", "true"),
            ("orderBy", "startTime"),
            ("maxResults", "2500"),
        ];
        if let Some(p) = &page {
            q.push(("pageToken", p.as_str()));
        }
        let resp = c.call("GET", &format!("{API}/calendars/{}/events", enc(calendar)), &q, None)?;
        for ev in resp["items"].as_array().into_iter().flatten() {
            let private = &ev["extendedProperties"]["private"];
            // "lifehub_id": events created before the app was renamed
            if ev["status"] == "cancelled" || !private["morrowkeep_id"].is_null() || !private["lifehub_id"].is_null() {
                continue;
            }
            let (Some((start, all_day)), Some((end, _))) = (parse_event_time(&ev["start"]), parse_event_time(&ev["end"])) else {
                continue;
            };
            out.push(ExtEvent {
                title: ev["summary"].as_str().unwrap_or("(no title)").to_string(),
                calendar: calendar.to_string(),
                start,
                end,
                all_day,
            });
        }
        match resp["nextPageToken"].as_str() {
            Some(p) => page = Some(p.to_string()),
            None => break,
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- sync

enum Push {
    Linked { item_id: String, google_id: String, synced: DateTime<Local> },
    Unlinked { item_id: String },
}

/// One full sync round. The data lock is only held briefly at the start and end,
/// never during network calls.
pub fn sync(shared: &Arc<Mutex<Data>>) -> Result<String, String> {
    let (items, settings, mut state, token_path) = {
        let d = shared.lock().unwrap();
        (d.items.clone(), d.settings.clone(), d.sync.clone(), d.paths.token.clone())
    };
    let orig_pending = state.pending_deletes.len();
    let tz = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into());
    let client = Client { token: access_token(&settings, &token_path).map_err(|e| e.to_string())? };
    let cal = ensure_calendar(&client, &settings.google_calendar_name, state.calendar_id.as_deref(), &tz)
        .map_err(|e| e.to_string())?;
    let cal_changed = state.calendar_id.as_deref() != Some(cal.as_str());
    state.calendar_id = Some(cal.clone());
    let events_url = format!("{API}/calendars/{}/events", enc(&cal));

    // Remote deletes for items deleted locally.
    let mut still_pending = Vec::new();
    for gid in std::mem::take(&mut state.pending_deletes) {
        match client.call("DELETE", &format!("{events_url}/{}", enc(&gid)), &[], None) {
            Ok(_) | Err(ApiErr::Status(404, _)) | Err(ApiErr::Status(410, _)) => {}
            Err(_) => still_pending.push(gid),
        }
    }
    state.pending_deletes = still_pending;

    let mut results = Vec::new();
    let mut errors = 0;
    let mut pushed = 0;
    for item in &items {
        let wanted = item.sync_google && item.date.is_some();
        let up_to_date = item.google_synced == Some(item.updated) && !cal_changed;
        match (&item.google_id, wanted) {
            (Some(gid), false) => {
                match client.call("DELETE", &format!("{events_url}/{}", enc(gid)), &[], None) {
                    Ok(_) | Err(ApiErr::Status(404, _)) | Err(ApiErr::Status(410, _)) => {
                        results.push(Push::Unlinked { item_id: item.id.clone() })
                    }
                    Err(_) => errors += 1,
                }
            }
            (None, false) => {}
            (Some(_), true) if up_to_date => {}
            (gid, true) => {
                let body = event_body(item, &tz);
                let updated = match gid {
                    Some(gid) if !cal_changed => match client.call("PUT", &format!("{events_url}/{}", enc(gid)), &[], Some(&body)) {
                        Ok(v) => Some(Ok(v)),
                        Err(ApiErr::Status(404, _)) | Err(ApiErr::Status(410, _)) => None, // deleted on Google: recreate
                        Err(e) => Some(Err(e)),
                    },
                    _ => None,
                };
                let res = match updated {
                    Some(r) => r,
                    None => client.call("POST", &events_url, &[], Some(&body)),
                };
                match res {
                    Ok(v) => {
                        pushed += 1;
                        results.push(Push::Linked {
                            item_id: item.id.clone(),
                            google_id: v["id"].as_str().unwrap_or_default().to_string(),
                            synced: item.updated,
                        })
                    }
                    Err(_) => errors += 1,
                }
            }
        }
    }

    // Read-only events from your other calendars.
    let today = Local::now().date_naive();
    let mut external = Vec::new();
    let mut pull_ok = true;
    for c in settings.pull_calendars.iter().filter(|c| !c.trim().is_empty() && **c != cal) {
        match pull_events(&client, c.trim(), today - Duration::days(31), today + Duration::days(180)) {
            Ok(mut ev) => external.append(&mut ev),
            Err(_) => pull_ok = false,
        }
    }

    let mut d = shared.lock().unwrap();
    // Deletes queued while we were busy were appended after the ones we started with.
    let keep = orig_pending.min(d.sync.pending_deletes.len());
    let mut late_deletes = d.sync.pending_deletes.split_off(keep);
    for r in results {
        match r {
            Push::Linked { item_id, google_id, synced } => match d.item_index(&item_id) {
                Some(idx) => {
                    d.items[idx].google_id = Some(google_id);
                    d.items[idx].google_synced = Some(synced);
                }
                // deleted while we were syncing
                None => late_deletes.push(google_id),
            },
            Push::Unlinked { item_id } => {
                if let Some(idx) = d.item_index(&item_id) {
                    d.items[idx].google_id = None;
                    d.items[idx].google_synced = None;
                }
            }
        }
    }
    d.sync = state;
    d.sync.pending_deletes.extend(late_deletes);
    d.sync.last_sync = Some(Local::now());
    if pull_ok {
        d.gcache = external;
    }
    d.save_items();
    d.save_sync();

    let mut msg = format!("Synced {}", Local::now().format("%H:%M"));
    if pushed > 0 {
        msg += &format!(" · {pushed} pushed");
    }
    if errors > 0 || !pull_ok {
        msg += " · some requests failed, will retry";
    }
    Ok(msg)
}
