# Observability (JOE-2222)

Privacy-safe metrics and operation events for hosts and diagnostics.

## Surfaces

| Type | Role |
|------|------|
| `Metrics` / `MetricsSnapshot` | Counters + duration totals (schema v2) |
| `OpEvent` | Versioned stage/terminal events (schema v1) |
| `TerminalGuard` | Exactly-one terminal outcome (drop = failed) |
| `BoundedEventSink` | Host callback queue; overflow drops |
| `DiagnosticBundle` | Support/doctor enrichment (no payloads) |

## Rules

* No audio, transcript, synthesis text, API keys, private paths, or raw provider bodies.
* Controlled enums for operation/stage/terminal — not free-form labels.
* Engine metrics use `MetricsScope::EngineLocal`; CLI/process-global convenience uses `ProcessGlobal`.
* Totals are **sums**; do not label averages as p95.
* Event queues are bounded (`DEFAULT_EVENT_QUEUE_CAP`); drops never block inference.

## Example

```rust
use aurum_core::{BoundedEventSink, Metrics, OpKind, TerminalCategory, TerminalGuard};
use std::sync::Arc;

let metrics = Arc::new(Metrics::engine_local());
let sink = Arc::new(BoundedEventSink::new(64));
metrics.set_event_sink(Some(sink.clone()));

let mut guard = TerminalGuard::start(metrics.clone(), /* request_id */ 1, OpKind::Stt);
// ... work ...
guard.finish(TerminalCategory::Completed, false);

let snap = metrics.snapshot();
assert_eq!(snap.ops_completed, 1);
```

## Privacy canary

`privacy_scan` + `PRIVACY_CANARY_MARKERS` assert secrets/payload markers never
appear in event or snapshot JSON.
