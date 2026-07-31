/**
 * Minimal C11 client for aurum-ffi jobs (JOE-1626).
 *
 * Build (after `cargo build -p aurum-ffi`):
 *   cc -std=c11 -I crates/aurum-ffi/include \
 *      examples/job_cleanup.c \
 *      -L target/debug -laurum_ffi -lpthread -ldl -lm -o /tmp/aurum_job_cleanup
 *
 * Demonstrates: capabilities, engine create, cleanup job, take string, free.
 */
#include "aurum.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
  AurumCapabilities caps;
  memset(&caps, 0, sizeof(caps));
  caps.struct_version = 1;
  if (aurum_capabilities(&caps) != AURUM_OK || !caps.has_jobs) {
    fprintf(stderr, "capabilities failed\n");
    return 1;
  }
  printf("abi=%u jobs=%u cleanup=%u\n", caps.abi_version, caps.has_jobs,
         caps.has_cleanup);

  AurumEngineConfig cfg;
  memset(&cfg, 0, sizeof(cfg));
  cfg.cache_dir = "/tmp/aurum-ffi-example-cache";
  cfg.local_only = 1;

  AurumEngine *engine = NULL;
  if (aurum_engine_create(&cfg, &engine) != AURUM_OK) {
    fprintf(stderr, "engine create failed\n");
    return 1;
  }

  AurumJob *job = NULL;
  if (aurum_job_start_cleanup(engine, "um, hello from C", AURUM_CLEANUP_CLEAN,
                              &job) != AURUM_OK) {
    fprintf(stderr, "start cleanup failed: %s\n",
            aurum_engine_last_error(engine));
    aurum_engine_destroy(engine);
    return 1;
  }
  if (aurum_job_wait(job, 5000) != AURUM_OK) {
    fprintf(stderr, "wait failed\n");
    aurum_job_free(job);
    aurum_engine_destroy(engine);
    return 1;
  }

  char *out = NULL;
  if (aurum_job_take_string(job, &out) != AURUM_OK) {
    fprintf(stderr, "take failed\n");
    aurum_job_free(job);
    aurum_engine_destroy(engine);
    return 1;
  }
  printf("cleaned: %s\n", out);
  aurum_string_free(out);
  aurum_job_free(job);
  aurum_engine_shutdown(engine, 2000);
  aurum_engine_destroy(engine);
  return 0;
}
