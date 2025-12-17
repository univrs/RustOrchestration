//! Observability stack for AI-Native Container Orchestration.
//!
//! This crate provides comprehensive observability capabilities:
//!
//! - **Tracing**: Structured logging with spans for distributed tracing
//! - **Metrics**: Prometheus-compatible metrics for monitoring
//! - **Health Endpoints**: HTTP endpoints for health checks and readiness probes
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                  Observability Layer                     │
//! ├─────────────────┬─────────────────┬─────────────────────┤
//! │    Tracing      │     Metrics     │   Health Server     │
//! │  (tracing +     │   (metrics +    │      (axum)         │
//! │   subscriber)   │   prometheus)   │                     │
//! ├─────────────────┴─────────────────┴─────────────────────┤
//! │                 Orchestrator Components                  │
//! └─────────────────────────────────────────────────────────┘
//! ```

pub mod tracing_setup;
pub mod metrics;
pub mod health;
pub mod server;

pub use tracing_setup::{init_tracing, TracingConfig};
pub use metrics::{OrchestratorMetrics, MetricsRegistry};
pub use health::{HealthChecker, HealthStatus, ComponentHealth};
pub use server::{ObservabilityServer, ObservabilityConfig};

/// Re-export tracing macros for convenience
pub use tracing::{debug, error, info, instrument, trace, warn, span, Level};
