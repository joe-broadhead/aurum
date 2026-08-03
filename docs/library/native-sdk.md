# Native SDK bundles (JOE-2225)

First-class C/C++ SDK archives for Tier A platforms. Downstream hosts should
**download a verified release archive**, not assemble libraries from a Cargo
workspace.

## Package

```bash
# STT/cleanup-only staticlib (matches CI native examples)
./scripts/package_native_sdk.sh --features none --out-dir dist/native-sdk
```

Produces `aurum-sdk-<version>-<triple>.tar.gz` and `.sha256`.

### Archive layout

```
aurum-sdk-<ver>-<triple>/
  include/aurum.h
  lib/libaurum_ffi.a   # or platform static/import lib
  cmake/AurumConfig.cmake
  pkg-config/aurum.pc  # Unix
  examples/job_cleanup.c
  examples/engine_raii.cpp
  BUILD.md
  SDK_MANIFEST.json
  share/doc/...
```

## Qualify a downloaded artifact

```bash
./scripts/qualify_native_sdk_bundle.sh --archive dist/native-sdk/aurum-sdk-*.tar.gz
```

Checks: no path traversal/symlinks, manifest digests, ABI constants, CMake
package content (system link deps), C11 (and C++17) compile/link/run using
**only** bundle paths, optional CMake consumer (`Aurum::aurum_ffi`), and
pkg-config on Unix. On Windows, MSVC `cl` is used when available; otherwise
layout + CMake package validation still run. Fails if the header mentions
remote provider surfaces.

## Install / upgrade / uninstall

1. Verify archive SHA-256 against release `SHA256SUMS` / cosign evidence.
2. Extract to a versioned prefix (side-by-side versions supported).
3. Point `CMAKE_PREFIX_PATH` or `-I`/`-L` at that prefix.
4. Uninstall by deleting the prefix; model caches under the user cache dir are
   **not** removed.

## Contracts

* ABI v2 only (`AURUM_ABI_VERSION` / min = 2).
* Remote providers remain **unavailable** through C.
* Models/packs are not bundled; first-run verified download uses the engine cache.
* FFmpeg is not bundled.
* Dynamic library search paths must not include untrusted directories.

## Release evidence

SDK archives should be listed in release `SHA256SUMS`, provenance, and SBOM
inventory alongside CLI binaries when published from the release workflow.
