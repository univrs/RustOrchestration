//! API server state.

use std::sync::Arc;

use tokio::sync::mpsc;

use cluster_manager_interface::ClusterManager;
use orchestrator_shared_types::WorkloadDefinition;
use state_store_interface::StateStore;

use super::auth::AuthConfig;

/// Shared state for the API server.
#[derive(Clone)]
pub struct ApiState {
    /// State store for persistence.
    pub state_store: Arc<dyn StateStore>,
    /// Cluster manager for node information.
    pub cluster_manager: Arc<dyn ClusterManager>,
    /// Channel to submit workloads to the orchestrator.
    pub workload_tx: mpsc::Sender<WorkloadDefinition>,
    /// Authentication configuration.
    pub auth_config: Arc<AuthConfig>,
}

impl ApiState {
    /// Create new API state.
    pub fn new(
        state_store: Arc<dyn StateStore>,
        cluster_manager: Arc<dyn ClusterManager>,
        workload_tx: mpsc::Sender<WorkloadDefinition>,
        auth_config: AuthConfig,
    ) -> Self {
        Self {
            state_store,
            cluster_manager,
            workload_tx,
            auth_config: Arc::new(auth_config),
        }
    }

    /// Create API state with auth disabled.
    pub fn new_without_auth(
        state_store: Arc<dyn StateStore>,
        cluster_manager: Arc<dyn ClusterManager>,
        workload_tx: mpsc::Sender<WorkloadDefinition>,
    ) -> Self {
        Self::new(state_store, cluster_manager, workload_tx, AuthConfig::disabled())
    }
}
