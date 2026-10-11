//! Journal: dated entries, typed or recorded. Entries live in journal.json; voice recordings
//! are WAV files in the journal-audio folder, with Whisper's transcription kept next to the
//! entry. #words in an entry are its tags.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub date: NaiveDate,
    #[serde(with = "hm")]
    pub time: NaiveTime,
    /// What you typed (can be empty for a voice entry).
    #[serde(default)]
    pub text: String,
    /// Recording file name inside the journal-audio folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_secs: Option<f32>,
    /// Whisper's transcription of the recording (editable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    /// 1 (rough) .. 5 (great).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood: Option<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// "note" (yours) or "reflection" (written by Claude from your entries).
    #[serde(default = "note", skip_serializing_if = "is_note")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<DateTime<Local>>,
}

fn note() -> String {
    "note".into()
}
fn is_note(k: &str) -> bool {
    k == "note"
}

mod hm {
    use chrono::NaiveTime;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(t: &NaiveTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format("%H:%M").to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveTime, D::Error> {
        let s = String::deserialize(d)?;
        NaiveTime::parse_from_str(&s, "%H:%M").or_else(|_| NaiveTime::parse_from_str(&s, "%H:%M:%S")).map_err(serde::de::Error::custom)
    }
}

impl JournalEntry {
    pub fn new(text: impl Into<String>) -> Self {
        let now = Local::now();
        JournalEntry {
            id: uuid::Uuid::new_v4().to_string(),
            date: now.date_naive(),
            time: now.time(),
            text: text.into(),
            audio: None,
            audio_secs: None,
            transcript: None,
            mood: None,
            pinned: false,
            kind: note(),
            edited: None,
        }
    }

    /// Typed text and transcription together (for search, tags and export).
    pub fn full_text(&self) -> String {
        match (&self.text, &self.transcript) {
            (t, Some(tr)) if !t.trim().is_empty() => format!("{t}\n{tr}"),
            (_, Some(tr)) => tr.clone(),
            (t, None) => t.clone(),
        }
    }

    /// #tags in the entry, lowercase.
    pub fn tags(&self) -> BTreeSet<String> {
        tags_in(&self.full_text())
    }

    pub fn words(&self) -> usize {
        self.full_text().split_whitespace().count()
    }

    pub fn matches(&self, query: &str) -> bool {
        let text = self.full_text().to_lowercase();
        query.to_lowercase().split_whitespace().all(|w| text.contains(w))
    }
}

pub fn tags_in(text: &str) -> BTreeSet<String> {
    text.split_whitespace()
        .filter_map(|w| w.strip_prefix('#'))
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric() && c != '-').to_lowercase())
        .filter(|t| !t.is_empty() && !t.chars().all(|c| c.is_ascii_digit()))
        .collect()
}

/// Whisper's transcript without its non-speech markers ("[Music]", "[BLANK_AUDIO]", "(wind)").
pub fn clean_transcript(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub const MOODS: [&str; 5] = ["rough", "meh", "okay", "good", "great"];

pub fn mood_label(m: u8) -> &'static str {
    MOODS[(m.clamp(1, 5) - 1) as usize]
}

/// Writing prompts offered in the composer.
pub const PROMPTS: &[&str] = &[
    "What went well today?",
    "What's one thing you're grateful for right now?",
    "What took most of your energy today?",
    "What did you learn today?",
    "What would make tomorrow a good day?",
    "How did you take care of your body today?",
    "What's on your mind that you haven't said out loud?",
    "What are you looking forward to?",
    "What would you tell yourself a year ago?",
    "Who made your day better, and how?",
    "What's something small that made you smile?",
    "What's one habit you want to keep this week?",
    "What drained you today, and what recharged you?",
    "What did you eat today, and how did it make you feel?",
    "What's a worry you can let go of?",
    "Describe today in three words.",
];

/// Days in a row with at least one entry, ending today (or yesterday, if today has none yet).
pub fn streak(entries: &[JournalEntry], today: NaiveDate) -> u32 {
    let days: BTreeSet<NaiveDate> = entries.iter().filter(|e| e.kind == "note").map(|e| e.date).collect();
    let mut d = if days.contains(&today) { today } else { today - Duration::days(1) };
    let mut n = 0;
    while days.contains(&d) {
        n += 1;
        d -= Duration::days(1);
    }
    n
}

/// Writes 16 kHz mono samples as a 16-bit PCM WAV file.
pub fn write_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    const RATE: u32 = 16_000;
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
}

/// Reads a WAV written by `write_wav` back into samples (for transcribing it again later).
pub fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    if b.len() < 44 || &b[..4] != b"RIFF" {
        return Err("not a WAV file".into());
    }
    Ok(b[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect())
}

/// The whole journal as Markdown, oldest first.
pub fn to_markdown(entries: &[JournalEntry]) -> String {
    let mut sorted: Vec<&JournalEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| (e.date, e.time));
    let mut out = String::from("# Journal\n");
    let mut day = None;
    for e in sorted {
        if day != Some(e.date) {
            day = Some(e.date);
            out += &format!("\n## {}\n", e.date.format("%A, %B %-d, %Y"));
        }
        let mut head = format!("\n### {}", e.time.format("%H:%M"));
        if e.kind == "reflection" {
            head += " · Reflection (Claude)";
        }
        if let Some(m) = e.mood {
            head += &format!(" · mood: {}", mood_label(m));
        }
        if e.pinned {
            head += " · pinned";
        }
        out += &head;
        out += "\n\n";
        if !e.text.trim().is_empty() {
            out += e.text.trim();
            out += "\n";
        }
        if let Some(t) = &e.transcript {
            out += &format!("\n> 🎤 {}\n", t.trim().replace('\n', "\n> "));
        }
        if let Some(a) = &e.audio {
            out += &format!("\nRecording: `journal-audio/{a}`\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_streak_wav_and_markdown() {
        assert_eq!(clean_transcript(" [Music] Went for a walk (wind) today. [BLANK_AUDIO]"), "Went for a walk today.");
        assert_eq!(clean_transcript("[Music]"), "");
        assert_eq!(tags_in("Long run today #Running, felt #great! #1 #").into_iter().collect::<Vec<_>>(), ["great", "running"]);
        let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let mut entries = Vec::new();
        for k in [1, 2, 3, 5] {
            let mut e = JournalEntry::new("x");
            e.date = today - Duration::days(k);
            entries.push(e);
        }
        assert_eq!(streak(&entries, today), 3); // yesterday and the two days before
        let mut t = JournalEntry::new("today");
        t.date = today;
        entries.push(t);
        assert_eq!(streak(&entries, today), 4);

        let dir = std::env::temp_dir().join(format!("mk-wav-{}", std::process::id()));
        let path = dir.join("a.wav");
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 / 20.0).sin() * 0.5).collect();
        write_wav(&path, &samples).unwrap();
        let back = read_wav(&path).unwrap();
        assert_eq!(back.len(), 1600);
        assert!((back[100] - samples[100]).abs() < 0.001);
        let _ = std::fs::remove_dir_all(&dir);

        let mut v = JournalEntry::new("");
        v.transcript = Some("spoken words #walk".into());
        v.audio = Some("x.wav".into());
        v.mood = Some(4);
        assert!(v.matches("SPOKEN walk"));
        assert!(v.tags().contains("walk"));
        let md = to_markdown(&[v]);
        assert!(md.contains("mood: good") && md.contains("> 🎤 spoken words") && md.contains("journal-audio/x.wav"));
        // stored times are HH:MM
        let json = serde_json::to_string(&JournalEntry::new("t")).unwrap();
        assert!(json.contains("\"time\":\"") && !json.contains("\"kind\""));
    }
}
