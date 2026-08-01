//! Fuzz direct WAV/hound load path (JOE-1884).
//!
//! Writes bounded input bytes to a temp file and exercises
//! [`aurum_core::audio::try_load_wav_file`] with tight safety limits.
#![no_main]

use aurum_core::audio::try_load_wav_file;
use libfuzzer_sys::fuzz_target;
use std::io::Write;

const MAX_INPUT: usize = 64 * 1024;
// ~2 s of 16 kHz mono f32 decoded — keeps RSS low under libFuzzer.
const MAX_DURATION_SECS: f64 = 2.0;
const MAX_DECODED_BYTES: usize = 16_000 * 2 * 4;

fuzz_target!(|data: &[u8]| {
    let data = if data.len() > MAX_INPUT {
        &data[..MAX_INPUT]
    } else {
        data
    };
    let Ok(mut tmp) = tempfile::Builder::new().prefix("aurum-fuzz-wav-").suffix(".wav").tempfile()
    else {
        return;
    };
    if tmp.write_all(data).is_err() {
        return;
    }
    let _ = tmp.flush();
    let path = tmp.path();
    let _ = try_load_wav_file(path, MAX_DURATION_SECS, MAX_DECODED_BYTES);
});
