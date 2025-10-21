// Orchestrator API Server Library
//
// This library provides the HTTP API server for the RustOrchestration platform.
// It exposes REST endpoints for workload management, node inspection, and
// instance monitoring.

pub mod handlers;
pub mod routes;
pub mod server;
pub mod state;

pub use server::ApiServer;
