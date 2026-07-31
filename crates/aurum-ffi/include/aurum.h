/**
 * aurum-ffi — C ABI for on-device speech I/O (aurum-core).
 *
 * Speech both ways. On-device by default.
 *
 * PCM STT input must be mono float32 at AURUM_SAMPLE_RATE Hz.
 * TTS output is mono int16 at the adapter native rate (query audio handle).
 *
 * =============================================================================
 * Threading & lifetime (JOE-1577 / JOE-1625)
 * =============================================================================
 *   - Blocking exclusive ops (preload/transcribe_pcm): at most one per engine.
 *   - Jobs (aurum_job_*): start is nonblocking; poll/wait/cancel/free are
 *     thread-safe for a given job. Cancel is safe from another thread.
 *   - Distinct engines may run concurrently.
 *   - Do NOT call blocking exports from inside a host async runtime task
 *     (nested Tokio). Prefer jobs from event-loop threads.
 *   - Ownership graph: engine -> jobs -> results (transcript/audio/string).
 *   - Result take transfers ownership exactly once; free invalidates pointers.
 *   - Engine destroy cancels jobs and best-effort drains briefly; prefer
 *     aurum_engine_shutdown for a defined wait. Jobs participate in process
 *     active-op accounting so aurum_shutdown_ex waits for them before cache clear.
 *   - aurum_job_wait returns BUSY on timeout if the job is not yet terminal.
 *   - aurum_engine_last_error: valid only until the next last_error call on
 *     the SAME thread — copy immediately.
 *
 * =============================================================================
 * ABI evolution (JOE-1624)
 * =============================================================================
 *   - AURUM_ABI_VERSION is monotonic. This build speaks v2 and still accepts
 *     v1 blocking structs (no struct_size field).
 *   - Versioned structs begin with struct_size + struct_version; zero-initialize
 *     reserved fields. Unsupported versions return AURUM_ERR_INVALID_ARG.
 *   - Out pointers are set to NULL/0 before fallible work where applicable.
 */
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Monotonic ABI version; bump on breaking header changes. */
#define AURUM_ABI_VERSION 2
/** Oldest ABI still supported by this library. */
#define AURUM_ABI_MIN_VERSION 1

/** Required STT PCM sample rate (Hz), mono f32. */
#define AURUM_SAMPLE_RATE 16000

typedef struct AurumEngine AurumEngine;
typedef struct AurumTranscript AurumTranscript;
typedef struct AurumJob AurumJob;
typedef struct AurumAudio AurumAudio;

typedef enum AurumStatus {
  AURUM_OK = 0,
  AURUM_ERR_INVALID_ARG = 1,
  AURUM_ERR_STATE = 2,
  AURUM_ERR_MODEL_NOT_READY = 3,
  AURUM_ERR_MODEL_DOWNLOAD = 4,
  AURUM_ERR_INFERENCE = 5,
  AURUM_ERR_CANCELLED = 6,
  AURUM_ERR_AUDIO = 7,
  AURUM_ERR_INTERNAL = 8,
  AURUM_ERR_UNSUPPORTED = 9,
  AURUM_ERR_NO_MEMORY = 10,
  AURUM_ERR_BUSY = 11,
  AURUM_ERR_DEADLINE = 12,
  AURUM_ERR_OVERLOAD = 13,
  AURUM_ERR_ARTIFACT_INTEGRITY = 14,
  AURUM_ERR_NETWORK = 15,
  AURUM_ERR_AUTH = 16,
  AURUM_ERR_QUOTA = 17,
  AURUM_ERR_RATE_LIMIT = 18,
  AURUM_ERR_FILESYSTEM = 19,
  AURUM_ERR_SHUTDOWN = 20
} AurumStatus;

typedef enum AurumCleanupStyle {
  AURUM_CLEANUP_RAW = 0,
  AURUM_CLEANUP_CLEAN = 1,
  AURUM_CLEANUP_BULLETS = 2,
  AURUM_CLEANUP_PROFESSIONAL = 3,
  AURUM_CLEANUP_SUMMARY = 4
} AurumCleanupStyle;

typedef enum AurumJobState {
  AURUM_JOB_QUEUED = 0,
  AURUM_JOB_RUNNING = 1,
  AURUM_JOB_CANCELLING = 2,
  AURUM_JOB_COMPLETED = 3,
  AURUM_JOB_FAILED = 4,
  AURUM_JOB_CANCELLED = 5,
  AURUM_JOB_DEADLINE_EXCEEDED = 6
} AurumJobState;

typedef enum AurumJobKind {
  AURUM_JOB_KIND_PRELOAD = 1,
  AURUM_JOB_KIND_TRANSCRIBE = 2,
  AURUM_JOB_KIND_CLEANUP = 3,
  AURUM_JOB_KIND_TTS = 4
} AurumJobKind;

/* ---- v1 engine config (still supported) ---- */
typedef struct AurumEngineConfig {
  const char *cache_dir;    /* required, UTF-8 */
  uint8_t local_only;       /* 1 = never download */
  uint8_t progress_logging; /* 0 default */
  uint8_t reserved[6];      /* must be zero */
} AurumEngineConfig;

typedef struct AurumTranscribeOpts {
  const char *model;    /* required */
  const char *language; /* nullable → "auto" */
  uint8_t timestamps;   /* 0/1 */
  uint8_t reserved[7];  /* must be zero */
} AurumTranscribeOpts;

typedef struct AurumSegment {
  double start_s;
  double end_s;
  const char *text; /* owned by parent transcript */
} AurumSegment;

typedef struct AurumCapabilities {
  uint32_t struct_size;    /* set by host or 0 */
  uint32_t struct_version; /* 1 */
  uint32_t abi_version;
  uint32_t abi_min_version;
  uint8_t has_stt;
  uint8_t has_tts;
  uint8_t has_cleanup;
  uint8_t has_jobs;
  uint8_t has_doctor;
  uint32_t sample_rate_hz;
  uint8_t reserved[16];
} AurumCapabilities;

typedef struct AurumJobSnapshot {
  uint32_t struct_size;
  uint32_t struct_version; /* 1 */
  uint64_t job_id;
  uint8_t kind;            /* AurumJobKind */
  uint8_t state;           /* AurumJobState */
  uint32_t progress_pct;   /* 0..100 */
  uint8_t reserved[16];
} AurumJobSnapshot;

typedef struct AurumTtsOpts {
  uint32_t struct_size;
  uint32_t struct_version; /* 1 */
  const char *model;       /* nullable → default */
  const char *voice;       /* nullable → default */
  const char *language;    /* nullable → "en" */
  float speaking_rate;     /* 0 → 1.0 */
  uint8_t reserved[16];
} AurumTtsOpts;

/* version / process */
uint32_t aurum_abi_version(void);
uint32_t aurum_sample_rate(void);
const char *aurum_version(void);
void aurum_shutdown(void);
AurumStatus aurum_shutdown_ex(uint32_t timeout_ms);

/** Feature query — does not start work. Zero-initialize `out` first. */
AurumStatus aurum_capabilities(AurumCapabilities *out);

/** Read-only diagnostics as JSON (free with aurum_string_free). */
AurumStatus aurum_doctor_json(char **out_json);

/* engine lifecycle */
AurumStatus aurum_engine_create(const AurumEngineConfig *cfg, AurumEngine **out);
void aurum_engine_destroy(AurumEngine *engine);
/**
 * Drain this engine's jobs (does not poison other engines).
 * Returns BUSY if jobs remain after timeout_ms.
 */
AurumStatus aurum_engine_shutdown(AurumEngine *engine, uint32_t timeout_ms);
const char *aurum_engine_last_error(const AurumEngine *engine);

/* models (blocking) */
AurumStatus aurum_engine_preload(AurumEngine *engine, const char *model);
uint8_t aurum_engine_is_model_ready(const AurumEngine *engine, const char *model);

/* decode (blocking) */
AurumStatus aurum_engine_transcribe_pcm(AurumEngine *engine,
                                        const float *samples, size_t n_samples,
                                        const AurumTranscribeOpts *opts,
                                        AurumTranscript **out_transcript);
void aurum_engine_cancel(AurumEngine *engine);

/* transcript */
const char *aurum_transcript_text(const AurumTranscript *t);
const char *aurum_transcript_language(const AurumTranscript *t);
const char *aurum_transcript_model(const AurumTranscript *t);
double aurum_transcript_duration_secs(const AurumTranscript *t);
uint8_t aurum_transcript_timestamps_reliable(const AurumTranscript *t);
size_t aurum_transcript_segment_count(const AurumTranscript *t);
AurumStatus aurum_transcript_segment(const AurumTranscript *t, size_t index,
                                     AurumSegment *out);
void aurum_transcript_free(AurumTranscript *t);

/* on-device rules cleanup (blocking; no engine required) */
AurumStatus aurum_cleanup_rules(const char *text, uint8_t style,
                                char **out_text);
void aurum_string_free(char *s);

/* ---- jobs (nonblocking start; JOE-1623) ---- */
AurumStatus aurum_job_start_preload(AurumEngine *engine, const char *model,
                                    AurumJob **out_job);
AurumStatus aurum_job_start_transcribe(AurumEngine *engine,
                                       const float *samples, size_t n_samples,
                                       const AurumTranscribeOpts *opts,
                                       AurumJob **out_job);
AurumStatus aurum_job_start_cleanup(AurumEngine *engine, const char *text,
                                    uint8_t style, AurumJob **out_job);
AurumStatus aurum_job_start_tts(AurumEngine *engine, const char *text,
                                size_t text_len, const AurumTtsOpts *opts,
                                AurumJob **out_job);

AurumStatus aurum_job_poll(const AurumJob *job, AurumJobSnapshot *out);
/**
 * Wait until the job is terminal.
 * timeout_ms == 0 waits forever. Does not cancel on timeout.
 * Returns AURUM_OK only when terminal; AURUM_ERR_BUSY if still running after timeout.
 */
AurumStatus aurum_job_wait(const AurumJob *job, uint32_t timeout_ms);
void aurum_job_cancel(AurumJob *job);
/** Transfer result exactly once. */
AurumStatus aurum_job_take_transcript(AurumJob *job,
                                      AurumTranscript **out_transcript);
AurumStatus aurum_job_take_string(AurumJob *job, char **out_text);
AurumStatus aurum_job_take_audio(AurumJob *job, AurumAudio **out_audio);
void aurum_job_free(AurumJob *job);

/* audio result (TTS) */
const int16_t *aurum_audio_samples(const AurumAudio *a);
size_t aurum_audio_len(const AurumAudio *a);
uint32_t aurum_audio_sample_rate(const AurumAudio *a);
uint16_t aurum_audio_channels(const AurumAudio *a);
uint64_t aurum_audio_duration_ms(const AurumAudio *a);
const char *aurum_audio_model(const AurumAudio *a);
const char *aurum_audio_voice(const AurumAudio *a);
void aurum_audio_free(AurumAudio *a);

#ifdef __cplusplus
}
#endif
