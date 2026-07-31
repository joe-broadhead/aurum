//! `extern "C"` surface. All behavior goes through [`crate::facade`].
//!
//! Safety contracts for each export are documented in `include/aurum.h`
//! (nullability, lifetimes, threading). Clippy safety sections are omitted
//! here to avoid duplicating the C header.

#![allow(clippy::missing_safety_doc)]

use crate::error::{FfiError, FfiStatus};
use crate::facade::{self, Engine};
use crate::jobs::JobState;
use crate::types::{
    CleanupStyle, EngineConfig, TranscribeOpts, Transcript, AURUM_ABI_VERSION, AURUM_SAMPLE_RATE,
};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_float, c_uint};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

/// Opaque engine handle for C.
pub struct AurumEngine {
    inner: Engine,
}

/// Owned transcript for C accessors.
pub struct AurumTranscript {
    inner: Transcript,
    text: CString,
    language: Option<CString>,
    model: CString,
    segment_texts: Vec<CString>,
}

#[repr(C)]
pub struct AurumEngineConfigC {
    pub cache_dir: *const c_char,
    pub local_only: u8,
    pub progress_logging: u8,
    pub reserved: [u8; 6],
}

#[repr(C)]
pub struct AurumTranscribeOptsC {
    pub model: *const c_char,
    pub language: *const c_char,
    pub timestamps: u8,
    pub reserved: [u8; 7],
}

#[repr(C)]
pub struct AurumSegmentC {
    pub start_s: f64,
    pub end_s: f64,
    pub text: *const c_char,
}

fn cstr<'a>(p: *const c_char) -> Result<&'a str, FfiError> {
    if p.is_null() {
        return Err(FfiError::invalid_arg("null string pointer"));
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| FfiError::invalid_arg("string is not valid UTF-8"))
}

fn cstr_opt<'a>(p: *const c_char) -> Result<Option<&'a str>, FfiError> {
    if p.is_null() {
        return Ok(None);
    }
    cstr(p).map(Some)
}

fn require_reserved_zero(bytes: &[u8]) -> Result<(), FfiError> {
    if bytes.iter().any(|&b| b != 0) {
        return Err(FfiError::invalid_arg(
            "reserved struct fields must be zero (memset the config/opts to 0 before use)",
        ));
    }
    Ok(())
}

fn status_ok() -> i32 {
    FfiStatus::Ok.as_i32()
}

fn status_err(e: &FfiError) -> i32 {
    e.status.as_i32()
}

fn catch_status(f: impl FnOnce() -> Result<(), FfiError>) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => status_ok(),
        Ok(Err(e)) => status_err(&e),
        Err(_) => FfiStatus::Internal.as_i32(),
    }
}

/// Like [`catch_status`], but records panic / error on the engine's last_error.
///
/// Holds [`ExportGuard`] for the **entire** exported call — including last_error
/// writes after the façade returns — so close/destroy cannot free the engine
/// while this wrapper still touches it (JOE-1647).
fn catch_status_engine(
    engine: *mut AurumEngine,
    f: impl FnOnce(&Engine) -> Result<(), FfiError>,
) -> i32 {
    if engine.is_null() {
        return FfiStatus::InvalidArg.as_i32();
    }
    let eng = unsafe { &*engine };
    let export = match eng.inner.begin_export() {
        Ok(g) => g,
        Err(e) => return status_err(&e),
    };
    let result = catch_unwind(AssertUnwindSafe(|| f(&eng.inner)));
    let code = match result {
        Ok(Ok(())) => status_ok(),
        Ok(Err(e)) => {
            // façade methods usually already store; ensure message is set
            eng.inner.set_last_error_message(e.message.clone());
            status_err(&e)
        }
        Err(_) => {
            eng.inner.set_last_error_message(
                "internal panic in aurum-ffi (see host logs); engine busy state was released if held",
            );
            FfiStatus::Internal.as_i32()
        }
    };
    drop(export);
    code
}

pub(crate) fn transcript_to_c(t: Transcript) -> Result<Box<AurumTranscript>, FfiError> {
    let text = CString::new(t.text.as_str())
        .map_err(|_| FfiError::internal("transcript text contains NUL"))?;
    let language = match &t.language {
        Some(l) => Some(
            CString::new(l.as_str()).map_err(|_| FfiError::internal("language contains NUL"))?,
        ),
        None => None,
    };
    let model =
        CString::new(t.model.as_str()).map_err(|_| FfiError::internal("model contains NUL"))?;
    let segment_texts = t
        .segments
        .iter()
        .map(|s| {
            CString::new(s.text.as_str())
                .map_err(|_| FfiError::internal("segment text contains NUL"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Box::new(AurumTranscript {
        inner: t,
        text,
        language,
        model,
        segment_texts,
    }))
}

/* ---------- version / process ---------- */

#[no_mangle]
pub extern "C" fn aurum_abi_version() -> c_uint {
    AURUM_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn aurum_sample_rate() -> c_uint {
    AURUM_SAMPLE_RATE
}

#[no_mangle]
pub extern "C" fn aurum_version() -> *const c_char {
    static VER: once_cell::sync::Lazy<CString> = once_cell::sync::Lazy::new(|| {
        CString::new(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| CString::new("0").unwrap())
    });
    VER.as_ptr()
}

/// Wait for in-flight ops, clear whisper context cache only if idle, reject new work.
/// Must not be called while the host still intends to use engines.
#[no_mangle]
pub extern "C" fn aurum_shutdown() {
    let _ = catch_unwind(|| {
        facade::shutdown();
    });
}

/// Drain with an explicit timeout. Returns `AURUM_OK` or `AURUM_ERR_BUSY`.
///
/// On BUSY, whisper contexts are **not** cleared.
#[no_mangle]
pub extern "C" fn aurum_shutdown_ex(timeout_ms: c_uint) -> i32 {
    match catch_unwind(|| {
        let ms = timeout_ms as u64;
        facade::shutdown_with_timeout(std::time::Duration::from_millis(ms))
    }) {
        Ok(Ok(())) => status_ok(),
        Ok(Err(e)) => status_err(&e),
        Err(_) => FfiStatus::Internal.as_i32(),
    }
}

/* ---------- engine lifecycle ---------- */

#[no_mangle]
pub unsafe extern "C" fn aurum_engine_create(
    cfg: *const AurumEngineConfigC,
    out: *mut *mut AurumEngine,
) -> i32 {
    // Initialize out before work (JOE-1647).
    if !out.is_null() {
        unsafe {
            *out = std::ptr::null_mut();
        }
    }
    catch_status(|| {
        if cfg.is_null() || out.is_null() {
            return Err(FfiError::invalid_arg("cfg and out must be non-null"));
        }
        let cfg = unsafe { &*cfg };
        require_reserved_zero(&cfg.reserved)?;
        let cache_dir = cstr(cfg.cache_dir)?.to_string();
        let engine = Engine::new(EngineConfig {
            cache_dir,
            local_only: cfg.local_only != 0,
            progress_logging: cfg.progress_logging != 0,
        })?;
        let boxed = Box::new(AurumEngine { inner: engine });
        unsafe {
            *out = Box::into_raw(boxed);
        }
        Ok(())
    })
}

/// Close an engine: reject new work, cancel in-flight ops, drain jobs, wait for
/// exclusive blocking ops, then free. Returns a status code (JOE-1647).
///
/// Preferred over [`aurum_engine_destroy`] when the host can handle failure.
/// On success the pointer is freed and must not be reused. On `AURUM_BUSY` the
/// engine remains valid and the host may retry close after waiting.
#[no_mangle]
pub unsafe extern "C" fn aurum_engine_close(engine: *mut AurumEngine, timeout_ms: u32) -> i32 {
    if engine.is_null() {
        return FfiStatus::InvalidArg.as_i32();
    }
    let timeout = std::time::Duration::from_millis(u64::from(timeout_ms.max(1)));
    let result = catch_unwind(AssertUnwindSafe(|| {
        let eng = unsafe { &*engine };
        eng.inner.shutdown_engine(timeout)
    }));
    match result {
        Ok(Ok(())) => {
            let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
                drop(Box::from_raw(engine));
            }));
            FfiStatus::Ok.as_i32()
        }
        Ok(Err(e)) => e.status.as_i32(),
        Err(_) => FfiStatus::Internal.as_i32(),
    }
}

/// Destroy an engine handle (void legacy API).
///
/// **Contract (JOE-1647):** closes admission, cancels work, and waits up to 30s
/// for exclusive blocking ops, export-boundary calls, and jobs. **Only frees on
/// successful drain.** If still BUSY after the wait, the pointer remains valid
/// and the host must retry [`aurum_engine_close`] (or process shutdown). This
/// avoids use-after-free of in-flight exported calls.
///
/// Prefer [`aurum_engine_close`] for status-returning teardown.
///
/// **Unsupported:** concurrent use of `engine` after a successful free; hosts
/// must serialize destroy/close with all other calls on the same handle.
#[no_mangle]
pub unsafe extern "C" fn aurum_engine_destroy(engine: *mut AurumEngine) {
    if engine.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        let eng = &*engine;
        match eng
            .inner
            .shutdown_engine(std::time::Duration::from_secs(30))
        {
            Ok(()) => {
                drop(Box::from_raw(engine));
            }
            Err(_) => {
                // Leave the box allocated; pointer remains valid for retry.
            }
        }
    }));
}

/// Last error message for this engine.
///
/// **Lifetime:** pointer is valid only until the next `aurum_engine_last_error`
/// call **on the same thread**. Copy immediately if you need to keep it.
#[no_mangle]
pub unsafe extern "C" fn aurum_engine_last_error(engine: *const AurumEngine) -> *const c_char {
    if engine.is_null() {
        return ptr::null();
    }
    thread_local! {
        static BUF: std::cell::RefCell<CString> =
            std::cell::RefCell::new(CString::new("").unwrap());
    }
    let msg = unsafe { &*engine }.inner.last_error();
    BUF.with(|b| {
        let c = CString::new(msg).unwrap_or_else(|_| CString::new("invalid error utf8").unwrap());
        *b.borrow_mut() = c;
        b.borrow().as_ptr()
    })
}

/* ---------- models ---------- */

#[no_mangle]
pub unsafe extern "C" fn aurum_engine_preload(
    engine: *mut AurumEngine,
    model: *const c_char,
) -> i32 {
    catch_status_engine(engine, |inner| {
        let model = cstr(model)?;
        inner.preload(model)
    })
}

/// Read-only model cache probe (does not download or load).
#[no_mangle]
pub unsafe extern "C" fn aurum_engine_is_model_ready(
    engine: *const AurumEngine,
    model: *const c_char,
) -> u8 {
    if engine.is_null() || model.is_null() {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let model = match cstr(model) {
            Ok(m) => m,
            Err(_) => return 0u8,
        };
        u8::from(unsafe { &*engine }.inner.is_model_ready(model))
    }))
    .unwrap_or_default()
}

/* ---------- decode ---------- */

#[no_mangle]
pub unsafe extern "C" fn aurum_engine_transcribe_pcm(
    engine: *mut AurumEngine,
    samples: *const c_float,
    n_samples: usize,
    opts: *const AurumTranscribeOptsC,
    out_transcript: *mut *mut AurumTranscript,
) -> i32 {
    if !out_transcript.is_null() {
        unsafe {
            *out_transcript = ptr::null_mut();
        }
    }
    catch_status_engine(engine, |inner| {
        if out_transcript.is_null() {
            return Err(FfiError::invalid_arg("out_transcript must be non-null"));
        }
        if opts.is_null() {
            return Err(FfiError::invalid_arg("opts must be non-null"));
        }
        if samples.is_null() && n_samples > 0 {
            return Err(FfiError::invalid_arg("samples is null"));
        }
        let opts_c = unsafe { &*opts };
        require_reserved_zero(&opts_c.reserved)?;
        let model = cstr(opts_c.model)?.to_string();
        let language = cstr_opt(opts_c.language)?.unwrap_or("auto").to_string();
        let slice = if n_samples == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(samples, n_samples) }
        };
        let t = inner.transcribe_pcm(
            slice,
            &TranscribeOpts {
                model,
                language,
                timestamps: opts_c.timestamps != 0,
            },
        )?;
        let boxed = transcript_to_c(t)?;
        unsafe {
            *out_transcript = Box::into_raw(boxed);
        }
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_engine_cancel(engine: *mut AurumEngine) {
    if engine.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        unsafe { &*engine }.inner.cancel();
    }));
}

/* ---------- transcript ---------- */

#[no_mangle]
pub unsafe extern "C" fn aurum_transcript_text(t: *const AurumTranscript) -> *const c_char {
    if t.is_null() {
        return ptr::null();
    }
    unsafe { &*t }.text.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn aurum_transcript_language(t: *const AurumTranscript) -> *const c_char {
    if t.is_null() {
        return ptr::null();
    }
    match &unsafe { &*t }.language {
        Some(c) => c.as_ptr(),
        None => ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn aurum_transcript_model(t: *const AurumTranscript) -> *const c_char {
    if t.is_null() {
        return ptr::null();
    }
    unsafe { &*t }.model.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn aurum_transcript_duration_secs(t: *const AurumTranscript) -> f64 {
    if t.is_null() {
        return 0.0;
    }
    unsafe { &*t }.inner.duration_secs
}

#[no_mangle]
pub unsafe extern "C" fn aurum_transcript_timestamps_reliable(t: *const AurumTranscript) -> u8 {
    if t.is_null() {
        return 0;
    }
    u8::from(unsafe { &*t }.inner.timestamps_reliable)
}

#[no_mangle]
pub unsafe extern "C" fn aurum_transcript_segment_count(t: *const AurumTranscript) -> usize {
    if t.is_null() {
        return 0;
    }
    unsafe { &*t }.inner.segments.len()
}

#[no_mangle]
pub unsafe extern "C" fn aurum_transcript_segment(
    t: *const AurumTranscript,
    index: usize,
    out: *mut AurumSegmentC,
) -> i32 {
    // Zero complete output before any validation (JOE-1647 fourth-pass).
    if !out.is_null() {
        unsafe {
            ptr::write_bytes(out as *mut u8, 0, std::mem::size_of::<AurumSegmentC>());
        }
    }
    catch_status(|| {
        if t.is_null() || out.is_null() {
            return Err(FfiError::invalid_arg("t and out must be non-null"));
        }
        let tr = unsafe { &*t };
        let seg = tr
            .inner
            .segments
            .get(index)
            .ok_or_else(|| FfiError::invalid_arg("segment index out of range"))?;
        let text_ptr = tr
            .segment_texts
            .get(index)
            .map(|c| c.as_ptr())
            .ok_or_else(|| FfiError::internal("segment text cache missing"))?;
        unsafe {
            *out = AurumSegmentC {
                start_s: seg.start_s,
                end_s: seg.end_s,
                text: text_ptr,
            };
        }
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_transcript_free(t: *mut AurumTranscript) {
    if t.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(t));
    }));
}

/* ---------- cleanup ---------- */

#[no_mangle]
pub unsafe extern "C" fn aurum_cleanup_rules(
    text: *const c_char,
    style: u8,
    out_text: *mut *mut c_char,
) -> i32 {
    // Null out before any fallible work (JOE-1647 third-pass).
    if !out_text.is_null() {
        unsafe {
            *out_text = ptr::null_mut();
        }
    }
    catch_status(|| {
        if out_text.is_null() {
            return Err(FfiError::invalid_arg("out_text must be non-null"));
        }
        let text = cstr(text)?;
        let style = CleanupStyle::from_u8(style)
            .ok_or_else(|| FfiError::invalid_arg("unknown cleanup style (use 0..4)"))?;
        let cleaned = facade::cleanup_rules(text, style)?;
        let c =
            CString::new(cleaned).map_err(|_| FfiError::internal("cleanup result contains NUL"))?;
        unsafe {
            *out_text = c.into_raw();
        }
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(CString::from_raw(s));
    }));
}

/* ---------- ABI capabilities / doctor (JOE-1624 / JOE-1628) ---------- */

#[repr(C)]
pub struct AurumCapabilitiesC {
    pub struct_size: u32,
    pub struct_version: u32,
    pub abi_version: u32,
    pub abi_min_version: u32,
    pub has_stt: u8,
    pub has_tts: u8,
    pub has_cleanup: u8,
    pub has_jobs: u8,
    pub has_doctor: u8,
    pub sample_rate_hz: u32,
    pub reserved: [u8; 16],
}

/// Fill `out` with supported features. `out->struct_size` must be set by host
/// (or zero for "use sizeof this build"). Rejects unsupported struct_version.
#[no_mangle]
pub unsafe extern "C" fn aurum_capabilities(out: *mut AurumCapabilitiesC) -> i32 {
    // Snapshot host fields, then zero the full structure before validation so
    // unsupported-version failure never leaves stale capability bits (JOE-1647).
    let (host_size, host_ver) = if out.is_null() {
        (0u32, 0u32)
    } else {
        unsafe {
            let size = (*out).struct_size;
            let ver = (*out).struct_version;
            ptr::write_bytes(out as *mut u8, 0, std::mem::size_of::<AurumCapabilitiesC>());
            (size, ver)
        }
    };
    catch_status(|| {
        if out.is_null() {
            return Err(FfiError::invalid_arg("out must be non-null"));
        }
        let caps = crate::jobs::AbiCapabilities::current();
        if host_ver != 0 && host_ver != 1 {
            return Err(FfiError::invalid_arg(format!(
                "unsupported capabilities struct_version {host_ver} (need 1)"
            )));
        }
        unsafe {
            (*out).struct_size = caps.struct_size;
            (*out).struct_version = caps.struct_version;
            (*out).abi_version = caps.abi_version;
            (*out).abi_min_version = caps.abi_min_version;
            (*out).has_stt = caps.has_stt;
            (*out).has_tts = caps.has_tts;
            (*out).has_cleanup = caps.has_cleanup;
            (*out).has_jobs = caps.has_jobs;
            (*out).has_doctor = caps.has_doctor;
            (*out).sample_rate_hz = caps.sample_rate_hz;
            let _ = host_size; // accepted for forward compat
        }
        Ok(())
    })
}

/// Run doctor checks; writes JSON to *out_json (free with aurum_string_free).
#[no_mangle]
pub unsafe extern "C" fn aurum_doctor_json(out_json: *mut *mut c_char) -> i32 {
    catch_status(|| {
        if out_json.is_null() {
            return Err(FfiError::invalid_arg("out_json must be non-null"));
        }
        unsafe {
            *out_json = ptr::null_mut();
        }
        let cfg = aurum_core::config::Config::load().map_err(FfiError::from)?;
        let report = aurum_core::doctor::run_doctor(&cfg);
        let json = report.to_json_pretty().map_err(FfiError::from)?;
        let c = CString::new(json).map_err(|_| FfiError::internal("doctor json NUL"))?;
        unsafe {
            *out_json = c.into_raw();
        }
        Ok(())
    })
}

/* ---------- engine shutdown + jobs (JOE-1622/1623) ---------- */

/// Drain this engine's jobs. Does not affect other engines.
///
/// Does **not** hold `ExportGuard` for the full wait — otherwise close would
/// deadlock waiting for its own export_depth (JOE-1647).
#[no_mangle]
pub unsafe extern "C" fn aurum_engine_shutdown(
    engine: *mut AurumEngine,
    timeout_ms: c_uint,
) -> i32 {
    if engine.is_null() {
        return FfiStatus::InvalidArg.as_i32();
    }
    let eng = unsafe { &*engine };
    catch_status(|| {
        eng.inner
            .shutdown_engine(std::time::Duration::from_millis(timeout_ms as u64))
    })
}

/// Opaque job handle.
pub struct AurumJob {
    inner: crate::jobs::Job,
}

#[repr(C)]
pub struct AurumJobSnapshotC {
    pub struct_size: u32,
    pub struct_version: u32,
    pub job_id: u64,
    pub kind: u8,
    pub state: u8,
    pub progress_pct: u32,
    pub reserved: [u8; 16],
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_start_cleanup(
    engine: *mut AurumEngine,
    text: *const c_char,
    style: u8,
    out_job: *mut *mut AurumJob,
) -> i32 {
    // Before export admission / any fallible path (JOE-1647 third-pass).
    if !out_job.is_null() {
        unsafe {
            *out_job = ptr::null_mut();
        }
    }
    catch_status_engine(engine, |inner| {
        if out_job.is_null() {
            return Err(FfiError::invalid_arg("out_job must be non-null"));
        }
        let text = cstr(text)?;
        let style = CleanupStyle::from_u8(style)
            .ok_or_else(|| FfiError::invalid_arg("unknown cleanup style"))?;
        let job = inner.start_cleanup_job(text, style)?;
        unsafe {
            *out_job = Box::into_raw(Box::new(AurumJob { inner: job }));
        }
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_start_preload(
    engine: *mut AurumEngine,
    model: *const c_char,
    out_job: *mut *mut AurumJob,
) -> i32 {
    if !out_job.is_null() {
        unsafe {
            *out_job = ptr::null_mut();
        }
    }
    catch_status_engine(engine, |inner| {
        if out_job.is_null() {
            return Err(FfiError::invalid_arg("out_job must be non-null"));
        }
        let model = cstr(model)?;
        let job = inner.start_preload_job(model)?;
        unsafe {
            *out_job = Box::into_raw(Box::new(AurumJob { inner: job }));
        }
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_start_transcribe(
    engine: *mut AurumEngine,
    samples: *const c_float,
    n_samples: usize,
    opts: *const AurumTranscribeOptsC,
    out_job: *mut *mut AurumJob,
) -> i32 {
    if !out_job.is_null() {
        unsafe {
            *out_job = ptr::null_mut();
        }
    }
    catch_status_engine(engine, |inner| {
        if out_job.is_null() || opts.is_null() {
            return Err(FfiError::invalid_arg("out_job and opts must be non-null"));
        }
        if samples.is_null() && n_samples > 0 {
            return Err(FfiError::invalid_arg("samples is null"));
        }
        let opts_c = unsafe { &*opts };
        require_reserved_zero(&opts_c.reserved)?;
        let model = cstr(opts_c.model)?.to_string();
        let language = cstr_opt(opts_c.language)?.unwrap_or("auto").to_string();
        let slice = if n_samples == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(samples, n_samples) }
        };
        let job = inner.start_transcribe_job(
            slice,
            &TranscribeOpts {
                model,
                language,
                timestamps: opts_c.timestamps != 0,
            },
        )?;
        unsafe {
            *out_job = Box::into_raw(Box::new(AurumJob { inner: job }));
        }
        Ok(())
    })
}

#[cfg(feature = "tts")]
#[repr(C)]
pub struct AurumTtsOptsC {
    pub struct_size: u32,
    pub struct_version: u32,
    pub model: *const c_char,
    pub voice: *const c_char,
    pub language: *const c_char,
    pub speaking_rate: f32,
    pub reserved: [u8; 16],
}

#[cfg(feature = "tts")]
#[no_mangle]
pub unsafe extern "C" fn aurum_job_start_tts(
    engine: *mut AurumEngine,
    text: *const c_char,
    text_len: usize,
    opts: *const AurumTtsOptsC,
    out_job: *mut *mut AurumJob,
) -> i32 {
    if !out_job.is_null() {
        unsafe {
            *out_job = ptr::null_mut();
        }
    }
    catch_status_engine(engine, |inner| {
        if out_job.is_null() || opts.is_null() {
            return Err(FfiError::invalid_arg("out_job and opts must be non-null"));
        }
        if text.is_null() {
            return Err(FfiError::invalid_arg("text is null"));
        }
        let opts_c = unsafe { &*opts };
        if opts_c.struct_version != 0 && opts_c.struct_version != 1 {
            return Err(FfiError::invalid_arg("unsupported tts opts version"));
        }
        if opts_c.reserved.iter().any(|&b| b != 0) {
            return Err(FfiError::invalid_arg("tts opts reserved must be zero"));
        }
        let bytes = unsafe { std::slice::from_raw_parts(text as *const u8, text_len) };
        let text = std::str::from_utf8(bytes)
            .map_err(|_| FfiError::invalid_arg("TTS text is not valid UTF-8"))?;
        let model = cstr_opt(opts_c.model)?
            .unwrap_or(aurum_core::tts::DEFAULT_TTS_MODEL)
            .to_string();
        let voice = cstr_opt(opts_c.voice)?
            .unwrap_or(aurum_core::tts::DEFAULT_TTS_VOICE)
            .to_string();
        let language = cstr_opt(opts_c.language)?.unwrap_or("en").to_string();
        let rate = if opts_c.speaking_rate == 0.0 {
            1.0
        } else {
            opts_c.speaking_rate
        };
        let job = inner.start_tts_job(text, &model, &voice, &language, rate)?;
        unsafe {
            *out_job = Box::into_raw(Box::new(AurumJob { inner: job }));
        }
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_poll(job: *const AurumJob, out: *mut AurumJobSnapshotC) -> i32 {
    // Zero snapshot before validating job (JOE-1647 fourth-pass).
    if !out.is_null() {
        unsafe {
            ptr::write_bytes(out as *mut u8, 0, std::mem::size_of::<AurumJobSnapshotC>());
        }
    }
    catch_status(|| {
        if job.is_null() || out.is_null() {
            return Err(FfiError::invalid_arg("job and out must be non-null"));
        }
        let j = unsafe { &*job };
        let (state, prog) = j.inner.poll();
        unsafe {
            (*out).struct_size = std::mem::size_of::<AurumJobSnapshotC>() as u32;
            (*out).struct_version = 1;
            (*out).job_id = j.inner.id();
            (*out).kind = j.inner.kind() as u8;
            (*out).state = state as u8;
            (*out).progress_pct = prog;
        }
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_wait(job: *const AurumJob, timeout_ms: c_uint) -> i32 {
    catch_status(|| {
        if job.is_null() {
            return Err(FfiError::invalid_arg("job is null"));
        }
        let j = unsafe { &*job };
        let timeout = if timeout_ms == 0 {
            None
        } else {
            Some(std::time::Duration::from_millis(timeout_ms as u64))
        };
        let _ = j.inner.wait(timeout)?;
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_cancel(job: *mut AurumJob) {
    if job.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        unsafe { &*job }.inner.cancel();
    }));
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_take_transcript(
    job: *mut AurumJob,
    out_transcript: *mut *mut AurumTranscript,
) -> i32 {
    // Null out before validating job (JOE-1647 fourth-pass).
    if !out_transcript.is_null() {
        unsafe {
            *out_transcript = ptr::null_mut();
        }
    }
    catch_status(|| {
        if job.is_null() || out_transcript.is_null() {
            return Err(FfiError::invalid_arg("job and out must be non-null"));
        }
        let j = unsafe { &*job };
        match j.inner.take_result()? {
            crate::jobs::JobResult::Transcript(t) => {
                let boxed = transcript_to_c(t)?;
                unsafe {
                    *out_transcript = Box::into_raw(boxed);
                }
                Ok(())
            }
            _ => Err(FfiError::state("job result is not a transcript")),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_take_string(
    job: *mut AurumJob,
    out_text: *mut *mut c_char,
) -> i32 {
    if !out_text.is_null() {
        unsafe {
            *out_text = ptr::null_mut();
        }
    }
    catch_status(|| {
        if job.is_null() || out_text.is_null() {
            return Err(FfiError::invalid_arg("job and out must be non-null"));
        }
        let j = unsafe { &*job };
        match j.inner.take_result()? {
            crate::jobs::JobResult::Cleanup { text }
            | crate::jobs::JobResult::Preload { model: text } => {
                let c = CString::new(text)
                    .map_err(|_| FfiError::internal("result string contains NUL"))?;
                unsafe {
                    *out_text = c.into_raw();
                }
                Ok(())
            }
            _ => Err(FfiError::state("job result is not a string payload")),
        }
    })
}

/// Owned mono PCM result for TTS jobs.
pub struct AurumAudio {
    pcm: Vec<i16>,
    sample_rate_hz: u32,
    channels: u16,
    model: CString,
    voice: CString,
    duration_ms: u64,
}

#[cfg(feature = "tts")]
#[no_mangle]
pub unsafe extern "C" fn aurum_job_take_audio(
    job: *mut AurumJob,
    out_audio: *mut *mut AurumAudio,
) -> i32 {
    if !out_audio.is_null() {
        unsafe {
            *out_audio = ptr::null_mut();
        }
    }
    catch_status(|| {
        if job.is_null() || out_audio.is_null() {
            return Err(FfiError::invalid_arg("job and out must be non-null"));
        }
        let j = unsafe { &*job };
        match j.inner.take_result()? {
            crate::jobs::JobResult::Audio {
                pcm_i16,
                sample_rate_hz,
                channels,
                model,
                voice,
                duration_ms,
            } => {
                let boxed = Box::new(AurumAudio {
                    pcm: pcm_i16,
                    sample_rate_hz,
                    channels,
                    model: CString::new(model)
                        .map_err(|_| FfiError::internal("model contains NUL"))?,
                    voice: CString::new(voice)
                        .map_err(|_| FfiError::internal("voice contains NUL"))?,
                    duration_ms,
                });
                unsafe {
                    *out_audio = Box::into_raw(boxed);
                }
                Ok(())
            }
            _ => Err(FfiError::state("job result is not audio")),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_audio_samples(a: *const AurumAudio) -> *const i16 {
    if a.is_null() {
        return ptr::null();
    }
    unsafe { &*a }.pcm.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn aurum_audio_len(a: *const AurumAudio) -> usize {
    if a.is_null() {
        return 0;
    }
    unsafe { &*a }.pcm.len()
}

#[no_mangle]
pub unsafe extern "C" fn aurum_audio_sample_rate(a: *const AurumAudio) -> c_uint {
    if a.is_null() {
        return 0;
    }
    unsafe { &*a }.sample_rate_hz
}

#[no_mangle]
pub unsafe extern "C" fn aurum_audio_channels(a: *const AurumAudio) -> u16 {
    if a.is_null() {
        return 0;
    }
    unsafe { &*a }.channels
}

#[no_mangle]
pub unsafe extern "C" fn aurum_audio_duration_ms(a: *const AurumAudio) -> u64 {
    if a.is_null() {
        return 0;
    }
    unsafe { &*a }.duration_ms
}

#[no_mangle]
pub unsafe extern "C" fn aurum_audio_model(a: *const AurumAudio) -> *const c_char {
    if a.is_null() {
        return ptr::null();
    }
    unsafe { &*a }.model.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn aurum_audio_voice(a: *const AurumAudio) -> *const c_char {
    if a.is_null() {
        return ptr::null();
    }
    unsafe { &*a }.voice.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn aurum_audio_free(a: *mut AurumAudio) {
    if a.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(a));
    }));
}

#[no_mangle]
pub unsafe extern "C" fn aurum_job_free(job: *mut AurumJob) {
    if job.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        // Cancel if still running so work does not outlive the handle forever.
        let j = Box::from_raw(job);
        if !matches!(
            j.inner.state(),
            JobState::Completed
                | JobState::Failed
                | JobState::Cancelled
                | JobState::DeadlineExceeded
        ) {
            j.inner.cancel();
        }
        drop(j);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CleanupStyle, Segment};
    use std::ffi::CStr;

    #[test]
    fn transcript_accessors_and_segments() {
        let t = Transcript {
            text: "hello world".into(),
            language: Some("en".into()),
            model: "tiny-q5_1".into(),
            duration_secs: 1.5,
            timestamps_reliable: true,
            segments: vec![
                Segment {
                    start_s: 0.0,
                    end_s: 0.8,
                    text: "hello".into(),
                },
                Segment {
                    start_s: 0.8,
                    end_s: 1.5,
                    text: "world".into(),
                },
            ],
            cleanup_style: CleanupStyle::Raw,
        };
        let boxed = transcript_to_c(t).unwrap();
        let ptr = Box::into_raw(boxed);

        unsafe {
            let text = CStr::from_ptr(aurum_transcript_text(ptr)).to_str().unwrap();
            assert_eq!(text, "hello world");
            let lang = CStr::from_ptr(aurum_transcript_language(ptr))
                .to_str()
                .unwrap();
            assert_eq!(lang, "en");
            let model = CStr::from_ptr(aurum_transcript_model(ptr))
                .to_str()
                .unwrap();
            assert_eq!(model, "tiny-q5_1");
            assert!((aurum_transcript_duration_secs(ptr) - 1.5).abs() < 1e-9);
            assert_eq!(aurum_transcript_timestamps_reliable(ptr), 1);
            assert_eq!(aurum_transcript_segment_count(ptr), 2);

            let mut seg = AurumSegmentC {
                start_s: 0.0,
                end_s: 0.0,
                text: ptr::null(),
            };
            assert_eq!(aurum_transcript_segment(ptr, 0, &mut seg), 0);
            assert!((seg.start_s - 0.0).abs() < 1e-9);
            assert_eq!(CStr::from_ptr(seg.text).to_str().unwrap(), "hello");
            assert_eq!(aurum_transcript_segment(ptr, 2, &mut seg), 1); // INVALID_ARG
                                                                       // Out-of-range leaves a zeroed segment (no stale text pointer).
            assert!(seg.text.is_null());
            aurum_transcript_free(ptr);
        }
    }

    #[test]
    fn reserved_nonzero_rejected_on_config_and_opts() {
        use std::ffi::CString;
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let cache = CString::new(dir.path().to_str().unwrap()).unwrap();
        let mut cfg = AurumEngineConfigC {
            cache_dir: cache.as_ptr(),
            local_only: 1,
            progress_logging: 0,
            reserved: [0; 6],
        };
        cfg.reserved[0] = 1;
        let mut out = ptr::null_mut();
        let st = unsafe { aurum_engine_create(&cfg, &mut out) };
        assert_eq!(st, FfiStatus::InvalidArg.as_i32());

        cfg.reserved = [0; 6];
        let mut engine = ptr::null_mut();
        assert_eq!(unsafe { aurum_engine_create(&cfg, &mut engine) }, 0);

        let model = CString::new("tiny-q5_1").unwrap();
        let mut opts = AurumTranscribeOptsC {
            model: model.as_ptr(),
            language: ptr::null(),
            timestamps: 0,
            reserved: [0; 7],
        };
        opts.reserved[3] = 1;
        let mut tr = ptr::null_mut();
        let st = unsafe {
            aurum_engine_transcribe_pcm(engine, [0.0f32; 16].as_ptr(), 16, &opts, &mut tr)
        };
        assert_eq!(st, FfiStatus::InvalidArg.as_i32());
        unsafe { aurum_engine_destroy(engine) };
    }
}
