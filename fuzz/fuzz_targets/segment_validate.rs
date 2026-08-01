//! Fuzz Segment construction / validation (JOE-1861).
#![no_main]

use aurum_core::providers::Segment;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }
    let start = f64::from_le_bytes(data[0..8].try_into().unwrap());
    let end = f64::from_le_bytes(data[8..16].try_into().unwrap());
    let text = String::from_utf8_lossy(&data[16..data.len().min(16 + 4096)]);
    let _ = Segment::try_new(start, end, text.as_ref());
    let s = Segment::from_parts_unchecked(start, end, text.as_ref());
    let _ = s.validate();
});
