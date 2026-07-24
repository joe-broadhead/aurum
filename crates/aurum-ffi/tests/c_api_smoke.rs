//! Smoke tests that exercise the C ABI from Rust (same symbols hosts link).

use aurum_ffi::{
    aurum_abi_version, aurum_cleanup_rules, aurum_engine_create, aurum_engine_destroy,
    aurum_engine_is_model_ready, aurum_engine_last_error, aurum_engine_preload,
    aurum_engine_transcribe_pcm, aurum_sample_rate, aurum_shutdown, aurum_string_free,
    aurum_version, AurumEngine, AurumEngineConfigC, AurumTranscribeOptsC, AURUM_ABI_VERSION,
    AURUM_SAMPLE_RATE,
};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use tempfile::tempdir;

#[test]
fn versions() {
    assert_eq!(aurum_abi_version(), AURUM_ABI_VERSION);
    assert_eq!(aurum_sample_rate(), AURUM_SAMPLE_RATE);
    let v = unsafe { CStr::from_ptr(aurum_version()) };
    assert!(!v.to_bytes().is_empty());
}

#[test]
fn create_destroy_and_cleanup() {
    let dir = tempdir().unwrap();
    let cache = CString::new(dir.path().to_str().unwrap()).unwrap();
    let cfg = AurumEngineConfigC {
        cache_dir: cache.as_ptr(),
        local_only: 1,
        progress_logging: 0,
        reserved: [0; 6],
    };
    let mut engine: *mut AurumEngine = ptr::null_mut();
    let st = unsafe { aurum_engine_create(&cfg, &mut engine) };
    assert_eq!(st, 0);
    assert!(!engine.is_null());

    assert_eq!(
        unsafe { aurum_engine_is_model_ready(engine, c"tiny-q5_1".as_ptr()) },
        0
    );
    let preload_st = unsafe { aurum_engine_preload(engine, c"tiny-q5_1".as_ptr()) };
    assert_eq!(preload_st, 3); // MODEL_NOT_READY
    let err = unsafe { CStr::from_ptr(aurum_engine_last_error(engine)) };
    assert!(!err.to_bytes().is_empty());

    // empty pcm
    let model = CString::new("tiny-q5_1").unwrap();
    let lang = CString::new("en").unwrap();
    let opts = AurumTranscribeOptsC {
        model: model.as_ptr(),
        language: lang.as_ptr(),
        timestamps: 0,
        reserved: [0; 7],
    };
    let mut tr = ptr::null_mut();
    let st = unsafe { aurum_engine_transcribe_pcm(engine, ptr::null(), 0, &opts, &mut tr) };
    assert_eq!(st, 7); // AUDIO

    unsafe { aurum_engine_destroy(engine) };

    let text = CString::new("um, hello world").unwrap();
    let mut out: *mut c_char = ptr::null_mut();
    let st = unsafe { aurum_cleanup_rules(text.as_ptr(), 1, &mut out) };
    assert_eq!(st, 0);
    assert!(!out.is_null());
    let cleaned = unsafe { CStr::from_ptr(out) }.to_string_lossy();
    assert!(cleaned.to_ascii_lowercase().contains("hello"));
    unsafe { aurum_string_free(out) };

    aurum_shutdown();
}
