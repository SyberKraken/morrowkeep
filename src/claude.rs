//! Optional "Claude" input mode: requests go to the Claude Code CLI (`claude -p`) signed in
//! with your own Claude account, so they count toward your plan's usage limits instead of
//! needing API credits. Claude answers with Morrowkeep import JSON, which is previewed before use.

use crate::import;
use crate::store::{Data, Settings};
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MODELS: &[(&str, &str)] = &[
    ("haiku", "Haiku: fastest, lightest on your limit"),
    ("sonnet", "Sonnet: good planning (recommended)"),
    ("opus", "Opus: most capable, uses the most"),
];

const SYSTEM_PROMPT: &str = "You are the planning assistant inside Morrowkeep, a personal planner for tasks, chores, meals and workouts. \
You turn the user's request into a Morrowkeep import JSON document. Reply with ONLY that JSON object: no prose, no explanation. \
If you need to tell the user something (e.g. an assumption you made), put one short sentence in a top-level \"message\" field. When the user describes food or drink they had, log it in \"calories\" and estimate on the HIGH side: generous portions when no amount is given, include cooking fat, sauces and drinks, round up. The user prefers overcounting to undercounting.";

/// The CLI to run: the path from settings, else `claude` on PATH, the standard installer
/// location, or the newest copy bundled with the Claude desktop app.
pub fn find_cli(settings: &Settings) -> Option<PathBuf> {
    let configured = settings.claude_path.trim();
    if !configured.is_empty() {
        return Some(PathBuf::from(configured));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["claude.exe", "claude.cmd"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    let home = dirs::home_dir()?;
    let local = home.join(".local").join("bin").join("claude.exe");
    if local.is_file() {
        return Some(local);
    }
    // %APPDATA%\Claude\claude-code\<version>\<hash>\claude.exe, newest modified first
    let bundled = dirs::config_dir()?.join("Claude").join("claude-code");
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for ver in std::fs::read_dir(bundled).ok()?.flatten() {
        for sub in std::fs::read_dir(ver.path()).into_iter().flatten().flatten() {
            let exe = sub.path().join("claude.exe");
            if let Ok(m) = std::fs::metadata(&exe).and_then(|m| m.modified()) {
                found.push((m, exe));
            }
        }
    }
    found.sort();
    found.pop().map(|(_, p)| p)
}

/// Runs one headless request and returns Claude's text answer.
/// Blocking; call from a background thread.
pub fn ask(cli: &Path, model: &str, prompt: &str, workdir: &Path) -> Result<String, String> {
    let mut child = Command::new(cli)
        .args(["-p", "--output-format", "json", "--model", model])
        // keep requests small and side-effect free: no tools, no MCP servers, no user/project
        // settings or CLAUDE.md, and no saved session
        .args(["--tools", "", "--strict-mcp-config", "--setting-sources", "", "--no-session-persistence"])
        .args(["--system-prompt", SYSTEM_PROMPT])
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| format!("Couldn't start Claude Code at {}: {e}", cli.display()))?;

    // The prompt goes over stdin: it can be longer than Windows allows on a command line.
    let mut stdin = child.stdin.take().unwrap();
    let prompt = prompt.to_string();
    std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let mut stderr = child.stderr.take().unwrap();
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() > deadline => {
                let _ = child.kill();
                return Err("Claude took longer than 3 minutes and was stopped".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let out = reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();

    let v: serde_json::Value = serde_json::from_str(out.trim()).map_err(|_| {
        let msg = if err.trim().is_empty() { out.trim() } else { err.trim() };
        format!("Unexpected output from Claude Code: {}", msg.chars().take(300).collect::<String>())
    })?;
    let result = v["result"].as_str().unwrap_or_default().to_string();
    if v["is_error"].as_bool().unwrap_or(false) {
        if result.contains("Not logged in") || result.contains("/login") {
            return Err(format!(
                "Claude Code isn't signed in. Open a terminal, run \"{}\", type /login and choose your Claude account (Pro). Then try again.",
                cli.display()
            ));
        }
        return Err(format!("Claude: {}", if result.is_empty() { "request failed" } else { &result }));
    }
    Ok(result)
}

/// The full request: import format, today's date, current items, and what the user asked.
pub fn build_prompt(data: &Data, request: &str) -> String {
    format!(
        "{}\n\n---\n\nThe user's request (typed or dictated, may contain transcription errors):\n\n\"{}\"\n\n\
         Turn it into a Morrowkeep import JSON object. A single simple item becomes a single item. \
         Use \"id\" + changed fields to modify existing items and \"delete\" to remove them, only when the request asks for it. \
         Reply with only the JSON.",
        import::ai_context(data, 14),
        request.trim()
    )
}
