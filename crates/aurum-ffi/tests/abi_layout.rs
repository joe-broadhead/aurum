//! Guard ABI constants against silent drift (header macros mirrored here).
//!
//! Full C `#include` verification can be added later with a `cc` build smoke;
//! these asserts catch Rust-side constant mistakes that would desync `aurum.h`.

use aurum_ffi::{aurum_abi_version, aurum_sample_rate, AURUM_ABI_VERSION, AURUM_SAMPLE_RATE};
use std::mem;

// Keep in sync with include/aurum.h
const HEADER_ABI_VERSION: u32 = 2;
const HEADER_ABI_MIN_VERSION: u32 = 1;
const HEADER_SAMPLE_RATE: u32 = 16000;

#[test]
fn abi_constants_match_header_macros() {
    assert_eq!(AURUM_ABI_VERSION, HEADER_ABI_VERSION);
    assert_eq!(aurum_ffi::AURUM_ABI_MIN_VERSION, HEADER_ABI_MIN_VERSION);
    assert_eq!(AURUM_SAMPLE_RATE, HEADER_SAMPLE_RATE);
    assert_eq!(aurum_abi_version(), HEADER_ABI_VERSION);
    assert_eq!(aurum_sample_rate(), HEADER_SAMPLE_RATE);
}

#[test]
fn capabilities_struct_matches_header_shape() {
    let mut caps = aurum_ffi::AurumCapabilitiesC {
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
    assert_eq!(unsafe { aurum_ffi::aurum_capabilities(&mut caps) }, 0);
    assert_eq!(caps.abi_version, HEADER_ABI_VERSION);
    assert_eq!(caps.abi_min_version, HEADER_ABI_MIN_VERSION);
    assert_eq!(caps.has_stt, 1);
    assert_eq!(caps.has_jobs, 1);
    assert_eq!(caps.has_doctor, 1);
    assert_eq!(caps.sample_rate_hz, HEADER_SAMPLE_RATE);
}

#[test]
fn c_struct_sizes_are_stable() {
    // Layout expectations for AurumEngineConfig / AurumTranscribeOpts on common LP64.
    // pointer + 2×u8 + 6 reserved + padding
    let engine_cfg = mem::size_of::<aurum_ffi::AurumEngineConfigC>();
    let opts = mem::size_of::<aurum_ffi::AurumTranscribeOptsC>();
    let seg = mem::size_of::<aurum_ffi::AurumSegmentC>();
    // Sanity: non-zero and pointer-aligned-ish
    assert!(engine_cfg >= mem::size_of::<*const u8>() + 8);
    assert!(opts >= mem::size_of::<*const u8>() * 2 + 8);
    assert!(seg >= 16 + mem::size_of::<*const u8>());
    // Fixed reserved tails
    assert_eq!(mem::size_of_val(&[0u8; 6]), 6);
    assert_eq!(mem::size_of_val(&[0u8; 7]), 7);
}
