//! Fuzz STT DTO JSON + validated domain conversion (JOE-1861 / JOE-1809).
#![no_main]

use aurum_core::dto::SttResultDto;
use aurum_core::providers::TranscriptionResult;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let data = if data.len() > 128 * 1024 {
        &data[..128 * 1024]
    } else {
        data
    };
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(dto) = serde_json::from_str::<SttResultDto>(s) {
            // Domain conversion must never panic; Err is fine.
            let _ = TranscriptionResult::try_from_dto(&dto);
            let _ = dto.to_json_pretty();
        }
    }
});
