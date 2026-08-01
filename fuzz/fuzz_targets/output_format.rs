//! Fuzz output format parsing + JSON emit for random results (JOE-1861).
#![no_main]

use aurum_core::output::OutputFormat;
use aurum_core::postprocess::normalize_result_with_report;
use aurum_core::providers::{Segment, TranscriptionResult};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if !data.is_empty() {
        let s = String::from_utf8_lossy(&data[..data.len().min(64)]);
        let _ = OutputFormat::parse(s.trim());
    }
    if data.len() < 24 {
        return;
    }
    let d = f64::from_le_bytes(data[0..8].try_into().unwrap());
    let start = f64::from_le_bytes(data[8..16].try_into().unwrap());
    let end = f64::from_le_bytes(data[16..24].try_into().unwrap());
    let text = String::from_utf8_lossy(&data[24..data.len().min(24 + 2048)]).into_owned();
    let segs = vec![Segment::from_parts_unchecked(start, end, text.clone())];
    let r = TranscriptionResult::local(text, segs, None, "fuzz".into(), d);
    let (norm, _) = normalize_result_with_report(r);
    for fmt in [OutputFormat::Txt, OutputFormat::Json, OutputFormat::Srt] {
        let _ = aurum_core::output::format_result(&norm, fmt);
    }
});
