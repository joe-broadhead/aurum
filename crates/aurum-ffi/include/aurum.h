/**
 * aurum-ffi — C ABI for on-device speech-to-text (aurum-core).
 *
 * Audio in. Text out. On-device by default.
 *
 * PCM must be mono float32 at AURUM_SAMPLE_RATE Hz.
 *
 * Threading:
 *   - At most one exclusive op (preload or transcribe_pcm) per engine at a time.
 *   - Distinct engines may run concurrently (process-wide Tokio runtime).
 *   - aurum_engine_cancel is safe from another thread during transcribe.
 *   - Do NOT call blocking exports from inside a host async runtime task
 *     (e.g. nested Tokio); they use Handle::block_on and will fail/panic
 *     if already inside a runtime — call from a plain OS thread / queue.
 *
 * Lifecycle:
 *   - Zero-initialize config/opts structs (reserved fields must be 0).
 *   - Call aurum_engine_destroy on every engine, then aurum_shutdown() before
 *     process exit (Metal/ggml teardown). Do not start new work after shutdown.
 *   - aurum_engine_last_error: returned pointer is valid only until the next
 *     aurum_engine_last_error call on the SAME thread — copy immediately.
 */
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Monotonic ABI version; bump on breaking header changes. */
#define AURUM_ABI_VERSION 1

/** Required PCM sample rate (Hz), mono f32. */
#define AURUM_SAMPLE_RATE 16000

typedef struct AurumEngine AurumEngine;
typedef struct AurumTranscript AurumTranscript;

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
  AURUM_ERR_NO_MEMORY = 10
} AurumStatus;

typedef enum AurumCleanupStyle {
  AURUM_CLEANUP_RAW = 0,
  AURUM_CLEANUP_CLEAN = 1,
  AURUM_CLEANUP_BULLETS = 2,
  AURUM_CLEANUP_PROFESSIONAL = 3,
  AURUM_CLEANUP_SUMMARY = 4
} AurumCleanupStyle;

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

/* version / process */
uint32_t aurum_abi_version(void);
uint32_t aurum_sample_rate(void);
const char *aurum_version(void);
void aurum_shutdown(void);

/* engine lifecycle */
AurumStatus aurum_engine_create(const AurumEngineConfig *cfg, AurumEngine **out);
void aurum_engine_destroy(AurumEngine *engine);
/**
 * Last error for this engine.
 * Lifetime: valid until the next aurum_engine_last_error call on this thread.
 */
const char *aurum_engine_last_error(const AurumEngine *engine);

/* models */
AurumStatus aurum_engine_preload(AurumEngine *engine, const char *model);
/** Read-only; does not download or load. */
uint8_t aurum_engine_is_model_ready(const AurumEngine *engine, const char *model);

/* decode */
AurumStatus aurum_engine_transcribe_pcm(AurumEngine *engine,
                                        const float *samples, size_t n_samples,
                                        const AurumTranscribeOpts *opts,
                                        AurumTranscript **out_transcript);
void aurum_engine_cancel(AurumEngine *engine);

/* transcript (owned until aurum_transcript_free) */
const char *aurum_transcript_text(const AurumTranscript *t);
const char *aurum_transcript_language(const AurumTranscript *t);
const char *aurum_transcript_model(const AurumTranscript *t);
double aurum_transcript_duration_secs(const AurumTranscript *t);
uint8_t aurum_transcript_timestamps_reliable(const AurumTranscript *t);
size_t aurum_transcript_segment_count(const AurumTranscript *t);
AurumStatus aurum_transcript_segment(const AurumTranscript *t, size_t index,
                                     AurumSegment *out);
void aurum_transcript_free(AurumTranscript *t);

/* on-device rules cleanup (no engine required) */
AurumStatus aurum_cleanup_rules(const char *text, uint8_t style,
                                char **out_text);
void aurum_string_free(char *s);

#ifdef __cplusplus
}
#endif
