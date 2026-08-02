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
//! Process-global defaults live on [`ResourceGovernor::process_global`] for
//! convenience constructors only. **Engine-bound** remote providers must use the
//! engine-local governor from [`crate::provider_platform::ProviderBuildContext`]
//! (JOE-1975) so concurrent engines do not share remote admission.

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
pub use singleflight::{BeginLoad, LeaderGuard, LoadKey, Singleflight};
