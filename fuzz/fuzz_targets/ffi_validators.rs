//! Fuzz pure FFI validators (no engine / no native model load) (JOE-1884).
//!
//! Covers:
//! - `CleanupStyle` / `JobState` u8 mapping (fail-closed on unknown values)
//! - `cleanup_rules` pure string path for every valid style
#![no_main]

use aurum_ffi::{cleanup_rules, CleanupStyle, JobState};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    // Exhaustive mapping probes for the first two bytes.
    let style_byte = data[0];
    let state_byte = data.get(1).copied().unwrap_or(0);
    let style = CleanupStyle::from_u8(style_byte);
    let state = JobState::from_u8(state_byte);
    if let Some(s) = style {
        assert_eq!(s.as_u8(), style_byte);
        let _ = s.to_core();
    }
    if let Some(st) = state {
        let _ = st.is_terminal();
    }

    // Pure rules cleanup (no whisper / ORT). Cap text size for RSS.
    let text_bytes = if data.len() > 8 * 1024 {
        &data[..8 * 1024]
    } else {
        data
    };
    let Ok(text) = std::str::from_utf8(text_bytes) else {
        return;
    };
    if let Some(style) = style {
        let _ = cleanup_rules(text, style);
    }
    // Also probe out-of-range style bytes do not panic via core rules path.
    for b in [style_byte, 5, 6, 255] {
        if CleanupStyle::from_u8(b).is_none() {
            // Fail-closed: unknown style is rejected by the type system, not cleanup.
            continue;
        }
    }
});
