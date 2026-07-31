/**
 * Minimal C++17 RAII client for aurum-ffi (JOE-1647).
 *
 * Build (after `cargo build -p aurum-ffi --release`):
 *   c++ -std=c++17 -I crates/aurum-ffi/include \
 *      crates/aurum-ffi/examples/engine_raii.cpp \
 *      -L target/release -laurum_ffi -lpthread -ldl -lm -o /tmp/aurum_engine_raii
 */
#include "aurum.h"
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>

class AurumEngineHandle {
public:
  explicit AurumEngineHandle(const char *cache_dir) {
    AurumEngineConfig cfg;
    std::memset(&cfg, 0, sizeof(cfg));
    cfg.cache_dir = cache_dir;
    cfg.local_only = 1;
    if (aurum_engine_create(&cfg, &engine_) != AURUM_OK) {
      throw std::runtime_error("aurum_engine_create failed");
    }
  }

  AurumEngineHandle(const AurumEngineHandle &) = delete;
  AurumEngineHandle &operator=(const AurumEngineHandle &) = delete;

  ~AurumEngineHandle() {
    if (engine_) {
      if (aurum_engine_close(engine_, 5000) != AURUM_OK) {
        aurum_engine_destroy(engine_);
      }
      engine_ = nullptr;
    }
  }

  AurumEngine *get() const { return engine_; }

private:
  AurumEngine *engine_ = nullptr;
};

int main() {
  AurumCapabilities caps;
  std::memset(&caps, 0, sizeof(caps));
  caps.struct_version = 1;
  if (aurum_capabilities(&caps) != AURUM_OK) {
    std::fprintf(stderr, "capabilities failed\n");
    return 1;
  }
  std::printf("abi=%u has_cleanup=%u\n", caps.abi_version, caps.has_cleanup);

  try {
    AurumEngineHandle engine("/tmp/aurum-ffi-cpp-example-cache");
    AurumJob *job = nullptr;
    if (aurum_job_start_cleanup(engine.get(), "um, hello from C++",
                                AURUM_CLEANUP_CLEAN, &job) != AURUM_OK) {
      std::fprintf(stderr, "start cleanup failed\n");
      return 1;
    }
    if (aurum_job_wait(job, 5000) != AURUM_OK) {
      aurum_job_free(job);
      std::fprintf(stderr, "wait failed\n");
      return 1;
    }
    char *out = nullptr;
    if (aurum_job_take_string(job, &out) != AURUM_OK) {
      aurum_job_free(job);
      std::fprintf(stderr, "take failed\n");
      return 1;
    }
    std::printf("cleaned: %s\n", out);
    aurum_string_free(out);
    aurum_job_free(job);
  } catch (const std::exception &e) {
    std::fprintf(stderr, "%s\n", e.what());
    return 1;
  }
  return 0;
}
