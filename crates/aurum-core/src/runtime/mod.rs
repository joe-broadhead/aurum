//! Runtime lifecycle, concurrency, and resource governance (JOE-1573).
//!
//! Modules:
//! - [`op_context`] — per-operation cancel/deadline/progress
//! - [`governor`] — process/engine admission permits and overload policy
//! - [`singleflight`] — per-key load coalescing
//! - [`registry`] — weighted model residency and eviction
//! - [`lifecycle`] — Running → ShuttingDown → Stopped (for FFI / process)
//!
//! ## Profiles
//!
//! [`GovernorConfig::default`] is a conservative desktop profile. Use
//! [`GovernorConfig::mobile`] or [`GovernorConfig::server`] for known deployments.
//! Process-global defaults live on [`ResourceGovernor::process_global`].

pub mod governor;
pub mod lifecycle;
pub mod op_context;
pub mod registry;
pub mod singleflight;

pub use governor::{GovernorConfig, GovernorStats, PermitKind, ResourceGovernor, ResourcePermit};
pub use lifecycle::{AdmissionError, Lifecycle, LifecycleState, OpAdmission, ShutdownError};
pub use op_context::{OpContext, OpProgress, ProgressSink};
pub use registry::{
    ModelRegistry, RegistryConfig, RegistryEntry, RegistryPin, RegistrySnapshot, ResidencyWeight,
};
pub use singleflight::{LoadKey, Singleflight};
