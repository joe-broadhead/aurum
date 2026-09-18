//! Binary-adjacent library surface for the `aurum` CLI crate.
//! Keeps CLI modules unit-testable without exposing them from `aurum-core`.

pub mod audio_io;
pub mod batch_cmd;
pub mod cli;
pub mod completions_cmd;
pub mod converse_cmd;
pub mod llm;
pub mod stdio_proto;
pub mod support_cmd;
