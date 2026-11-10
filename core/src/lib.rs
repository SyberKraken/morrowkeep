//! Morrowkeep's rules, shared by the desktop app and the phone app (compiled to WebAssembly):
//! the entry-bar parser, tasks and repeats, the food library, body numbers, the journal, workouts,
//! and the steps that change your data (so the phone can show what the PC will do).

pub mod api;
pub mod body;
pub mod budget;
pub mod clock;
pub mod foods;
pub mod journal;
pub mod kinds;
pub mod model;
pub mod parse;
pub mod steps;
pub mod views;
pub mod workouts;
#[cfg(target_arch = "wasm32")]
mod wasm;
