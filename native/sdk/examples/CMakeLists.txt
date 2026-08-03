# Minimal consumer that builds the bundled C11 example via Aurum::aurum_ffi.
# Used by scripts/qualify_native_sdk_bundle.sh (v0.0.23 B).
cmake_minimum_required(VERSION 3.16)
project(aurum_sdk_cmake_consumer C CXX)

if(NOT AURUM_SDK_ROOT)
  message(FATAL_ERROR "AURUM_SDK_ROOT must point at the extracted SDK root")
endif()

include("${AURUM_SDK_ROOT}/cmake/AurumConfig.cmake")

add_executable(aurum_job_cleanup_cmake "${AURUM_SDK_ROOT}/examples/job_cleanup.c")
target_link_libraries(aurum_job_cleanup_cmake PRIVATE Aurum::aurum_ffi)

if(EXISTS "${AURUM_SDK_ROOT}/examples/engine_raii.cpp")
  add_executable(aurum_engine_raii_cmake "${AURUM_SDK_ROOT}/examples/engine_raii.cpp")
  target_link_libraries(aurum_engine_raii_cmake PRIVATE Aurum::aurum_ffi)
endif()
