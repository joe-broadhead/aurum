//! Fuzz config TOML parse/load (JOE-1861).
#![no_main]

use aurum_core::config::Config;
use libfuzzer_sys::fuzz_target;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);

fuzz_target!(|data: &[u8]| {
    // Cap input to keep disk and parse time bounded.
    let data = if data.len() > 64 * 1024 {
        &data[..64 * 1024]
    } else {
        data
    };
    let id = N.fetch_add(1, Ordering::Relaxed);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("f-{id}.toml"));
    let _ = fs::write(&path, data);
    let _ = Config::load_from(&path);
    let _ = Config::load_from_required(&path);
});
