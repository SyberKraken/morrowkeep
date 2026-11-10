//! The current time and fresh ids. On the PC these come from the system; in the phone's
//! WebAssembly build there is no clock or random source, so the phone passes the time and some
//! randomness in with every call (`set`).

use chrono::{DateTime, Local};

#[cfg(not(target_arch = "wasm32"))]
pub fn now() -> DateTime<Local> {
    Local::now()
}

/// A new random id, formatted like the PC's (UUID v4).
#[cfg(not(target_arch = "wasm32"))]
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(target_arch = "wasm32")]
mod given {
    use std::cell::Cell;
    thread_local! {
        /// Milliseconds since 1970 (UTC) and the phone's offset from UTC in minutes.
        pub static NOW: Cell<(i64, i32)> = const { Cell::new((0, 0)) };
        /// xorshift state, seeded by the phone.
        pub static SEED: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
    }
}

/// Sets the time (ms since 1970, UTC offset in minutes) and mixes in randomness from the phone.
#[cfg(target_arch = "wasm32")]
pub fn set(now_ms: i64, offset_min: i32, seed: u64) {
    given::NOW.with(|n| n.set((now_ms, offset_min)));
    given::SEED.with(|s| s.set(s.get() ^ seed | 1));
}

/// The phone's wall-clock time. (In WebAssembly chrono's `Local` has no time zone database and
/// works as UTC, so the phone's local time is carried as-is.)
#[cfg(target_arch = "wasm32")]
pub fn now() -> DateTime<Local> {
    use chrono::TimeZone;
    let (ms, off) = given::NOW.with(|n| n.get());
    let wall = chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().naive_utc() + chrono::Duration::minutes(off as i64);
    Local.from_local_datetime(&wall).earliest().unwrap_or_else(|| Local.from_utc_datetime(&wall))
}

#[cfg(target_arch = "wasm32")]
pub fn new_id() -> String {
    let mut bytes = [0u8; 16];
    given::SEED.with(|s| {
        let mut x = s.get();
        for chunk in bytes.chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            chunk.copy_from_slice(&x.to_le_bytes());
        }
        s.set(x);
    });
    uuid::Builder::from_random_bytes(bytes).into_uuid().to_string()
}
