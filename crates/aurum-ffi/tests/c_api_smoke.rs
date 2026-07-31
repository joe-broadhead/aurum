//! Smoke tests that exercise the C ABI from Rust (same symbols hosts link).

use aurum_ffi::{
    aurum_abi_version, aurum_capabilities, aurum_cleanup_rules, aurum_doctor_json,
    aurum_engine_create, aurum_engine_destroy, aurum_engine_is_model_ready,
    aurum_engine_last_error, aurum_engine_preload, aurum_engine_shutdown,
    aurum_engine_transcribe_pcm, aurum_job_free, aurum_job_poll, aurum_job_start_cleanup,
    aurum_job_take_string, aurum_job_wait, aurum_sample_rate, aurum_string_free, aurum_version,
    AurumCapabilitiesC, AurumEngine, AurumEngineConfigC, AurumJobSnapshotC, AurumTranscribeOptsC,
    AURUM_ABI_VERSION, AURUM_SAMPLE_RATE,
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
    // Do not call aurum_shutdown here — process lifecycle is sticky for the
    // suite and would poison later job tests in this binary.
}

#[test]
fn capabilities_and_doctor_and_cleanup_job() {
    let mut caps = AurumCapabilitiesC {
        struct_size: 0,
        struct_version: 1,
        abi_version: 0,
        abi_min_version: 0,
        has_stt: 0,
        has_tts: 0,
        has_cleanup: 0,
        has_jobs: 0,
        has_doctor: 0,
        sample_rate_hz: 0,
        reserved: [0; 16],
    };
    assert_eq!(unsafe { aurum_capabilities(&mut caps) }, 0);
    assert_eq!(caps.has_jobs, 1);
    assert_eq!(caps.has_doctor, 1);

    let mut json: *mut std::os::raw::c_char = ptr::null_mut();
    assert_eq!(unsafe { aurum_doctor_json(&mut json) }, 0);
    assert!(!json.is_null());
    let s = unsafe { CStr::from_ptr(json) }.to_string_lossy();
    assert!(s.contains("schema_version"));
    unsafe { aurum_string_free(json) };

    let dir = tempdir().unwrap();
    let cache = CString::new(dir.path().to_str().unwrap()).unwrap();
    let cfg = AurumEngineConfigC {
        cache_dir: cache.as_ptr(),
        local_only: 1,
        progress_logging: 0,
        reserved: [0; 6],
    };
    let mut engine: *mut AurumEngine = ptr::null_mut();
    assert_eq!(unsafe { aurum_engine_create(&cfg, &mut engine) }, 0);

    let text = CString::new("um, hello job").unwrap();
    let mut job = ptr::null_mut();
    assert_eq!(
        unsafe { aurum_job_start_cleanup(engine, text.as_ptr(), 1, &mut job) },
        0
    );
    assert!(!job.is_null());
    assert_eq!(unsafe { aurum_job_wait(job, 5000) }, 0);
    let mut snap = AurumJobSnapshotC {
        struct_size: 0,
        struct_version: 0,
        job_id: 0,
        kind: 0,
        state: 0,
        progress_pct: 0,
        reserved: [0; 16],
    };
    assert_eq!(unsafe { aurum_job_poll(job, &mut snap) }, 0);
    assert_eq!(snap.state, 3); // COMPLETED
    let mut out: *mut c_char = ptr::null_mut();
    assert_eq!(unsafe { aurum_job_take_string(job, &mut out) }, 0);
    let cleaned = unsafe { CStr::from_ptr(out) }.to_string_lossy();
    assert!(cleaned.to_ascii_lowercase().contains("hello"));
    unsafe { aurum_string_free(out) };
    unsafe { aurum_job_free(job) };
    assert_eq!(unsafe { aurum_engine_shutdown(engine, 2000) }, 0);
    unsafe { aurum_engine_destroy(engine) };
}

/// Closed / shutdown engines must null out_job before returning (JOE-1647).
#[test]
fn job_start_nulls_out_on_closed_engine() {
    let dir = tempdir().unwrap();
    let cache = CString::new(dir.path().to_str().unwrap()).unwrap();
    let cfg = AurumEngineConfigC {
        cache_dir: cache.as_ptr(),
        local_only: 1,
        progress_logging: 0,
        reserved: [0; 6],
    };
    let mut engine: *mut AurumEngine = ptr::null_mut();
    assert_eq!(unsafe { aurum_engine_create(&cfg, &mut engine) }, 0);
    assert_eq!(unsafe { aurum_engine_shutdown(engine, 2000) }, 0);

    // Poisoned sentinel: if the API leaves this unchanged, the host would free junk.
    let mut job = ptr::dangling_mut::<aurum_ffi::AurumJob>();
    let text = CString::new("hello").unwrap();
    let st = unsafe { aurum_job_start_cleanup(engine, text.as_ptr(), 1, &mut job) };
    assert_ne!(st, 0);
    assert!(
        job.is_null(),
        "out_job must be nulled before closed-engine failure"
    );

    // Invalid cleanup style also nulls (before other work).
    let mut out: *mut c_char = ptr::dangling_mut::<c_char>();
    let st = unsafe { aurum_cleanup_rules(text.as_ptr(), 99, &mut out) };
    assert_ne!(st, 0);
    assert!(out.is_null(), "out_text must be nulled on invalid style");

    unsafe { aurum_engine_destroy(engine) };
}
