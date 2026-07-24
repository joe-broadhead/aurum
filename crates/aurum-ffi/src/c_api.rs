//! `extern "C"` surface. All behavior goes through [`crate::facade`].
//!
//! Safety contracts for each export are documented in `include/aurum.h`
//! (nullability, lifetimes, threading). Clippy safety sections are omitted
//! here to avoid duplicating the C header.

#![allow(clippy::missing_safety_doc)]

use crate::error::{FfiError, FfiStatus};
use crate::facade::{self, Engine};
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
    /// Cached CString pointers so accessors can return `*const c_char`.
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

fn transcript_to_c(t: Transcript) -> Result<Box<AurumTranscript>, FfiError> {
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

/// ABI integer (`AURUM_ABI_VERSION`).
#[no_mangle]
pub extern "C" fn aurum_abi_version() -> c_uint {
    AURUM_ABI_VERSION
}

/// Required PCM sample rate (Hz).
#[no_mangle]
pub extern "C" fn aurum_sample_rate() -> c_uint {
    AURUM_SAMPLE_RATE
}

/// Crate SemVer string (static).
#[no_mangle]
pub extern "C" fn aurum_version() -> *const c_char {
    static VER: once_cell::sync::Lazy<CString> = once_cell::sync::Lazy::new(|| {
        CString::new(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| CString::new("0").unwrap())
    });
    VER.as_ptr()
}

/// Clear whisper context cache and drop the FFI Tokio runtime.
#[no_mangle]
pub extern "C" fn aurum_shutdown() {
    let _ = catch_unwind(|| {
        facade::shutdown();
    });
}

/* ---------- engine lifecycle ---------- */

/// Create an engine. On success writes `*out` and returns `AURUM_OK`.
#[no_mangle]
pub unsafe extern "C" fn aurum_engine_create(
    cfg: *const AurumEngineConfigC,
    out: *mut *mut AurumEngine,
) -> i32 {
    catch_status(|| {
        if cfg.is_null() || out.is_null() {
            return Err(FfiError::invalid_arg("cfg and out must be non-null"));
        }
        let cfg = unsafe { &*cfg };
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

/// Destroy an engine (no-op on null).
#[no_mangle]
pub unsafe extern "C" fn aurum_engine_destroy(engine: *mut AurumEngine) {
    if engine.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(engine));
    }));
}

/// Last error message for this engine (empty string if none). Never null if engine non-null.
#[no_mangle]
pub unsafe extern "C" fn aurum_engine_last_error(engine: *const AurumEngine) -> *const c_char {
    if engine.is_null() {
        return ptr::null();
    }
    // Leak-free: store in thread-local for pointer stability for this call pattern is hard;
    // use a per-call CString leaked into a slot on the engine via last_error rebuild.
    // We return a pointer into a thread-local buffer updated each call.
    thread_local! {
        static BUF: std::cell::RefCell<CString> = std::cell::RefCell::new(CString::new("").unwrap());
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
    catch_status(|| {
        if engine.is_null() {
            return Err(FfiError::invalid_arg("engine is null"));
        }
        let model = cstr(model)?;
        unsafe { &*engine }.inner.preload(model)
    })
}

#[no_mangle]
pub unsafe extern "C" fn aurum_engine_is_model_ready(
    engine: *mut AurumEngine,
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
    catch_status(|| {
        if engine.is_null() || out_transcript.is_null() {
            return Err(FfiError::invalid_arg(
                "engine and out_transcript must be non-null",
            ));
        }
        if opts.is_null() {
            return Err(FfiError::invalid_arg("opts must be non-null"));
        }
        if samples.is_null() && n_samples > 0 {
            return Err(FfiError::invalid_arg("samples is null"));
        }
        let opts_c = unsafe { &*opts };
        let model = cstr(opts_c.model)?.to_string();
        let language = cstr_opt(opts_c.language)?.unwrap_or("auto").to_string();
        let slice = if n_samples == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(samples, n_samples) }
        };
        let t = unsafe { &*engine }.inner.transcribe_pcm(
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
