//! Fuzz rules cleanup (synchronous via current-thread runtime) (JOE-1861).
#![no_main]

use aurum_core::cleanup::{CleanupStyle, RulesCleanup, TextCleanup};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let data = if data.len() > 32 * 1024 {
        &data[..32 * 1024]
    } else {
        data
    };
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let style = match data.first().copied().unwrap_or(0) % 5 {
        0 => CleanupStyle::Raw,
        1 => CleanupStyle::Clean,
        2 => CleanupStyle::Bullets,
        3 => CleanupStyle::Professional,
        _ => CleanupStyle::Summary,
    };
    let rules = RulesCleanup::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let _ = rt.block_on(async { rules.cleanup(text, style).await });
});
