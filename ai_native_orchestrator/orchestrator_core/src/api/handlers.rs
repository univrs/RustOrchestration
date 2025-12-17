//! API request handlers.

use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use orchestrator_shared_types::{
    ContainerConfig, Node, NodeResources, NodeStatus, PortMapping,
    WorkloadDefinition, WorkloadInstance, WorkloadInstanceStatus,
};

use super::error::{ApiError, ApiResult};
use super::state::ApiState;

// ============================================================================
// Request/Response Types
// ============================================================================

/// Request to create a new workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkloadRequest {
    /// User-friendly name for the workload.
    pub name: String,
    /// Container configurations.
    pub containers: Vec<ContainerConfigRequest>,
    /// Desired number of replicas.
    pub replicas: u32,
    /// Optional labels for scheduling and selection.
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Container configuration in API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfigRequest {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
    #[serde(default)]
    pub ports: Vec<PortMappingRequest>,
    #[serde(default)]
    pub resource_requests: ResourceRequestsRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMappingRequest {
    pub container_port: u16,
    pub host_port: Option<u16>,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "tcp".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceRequestsRequest {
    #[serde(default)]
    pub cpu_cores: f32,
    #[serde(default)]
    pub memory_mb: u64,
    #[serde(default)]
    pub disk_mb: u64,
}

/// Response for workload operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadResponse {
    pub id: Uuid,
    pub name: String,
    pub replicas: u32,
    pub labels: HashMap<String, String>,
    pub containers: Vec<ContainerConfigResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfigResponse {
    pub name: String,
    pub image: String,
    pub command: Option<Vec<String>>,
    pub args: Option<Vec<String>>,
    pub env_vars: HashMap<String, String>,
    pub ports: Vec<PortMappingResponse>,
    pub resource_requests: ResourceRequestsResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMappingResponse {
    pub container_port: u16,
    pub host_port: Option<u16>,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequestsResponse {
    pub cpu_cores: f32,
    pub memory_mb: u64,
    pub disk_mb: u64,
}

/// Response for list operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    pub count: usize,
}

/// Node response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResponse {
    pub id: Uuid,
    pub address: String,
    pub status: String,
    pub labels: HashMap<String, String>,
    pub resources_capacity: ResourceRequestsResponse,
    pub resources_allocatable: ResourceRequestsResponse,
}

/// Workload instance response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceResponse {
    pub id: Uuid,
    pub workload_id: Uuid,
    pub node_id: Uuid,
    pub container_ids: Vec<String>,
    pub status: String,
}

/// Cluster status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatusResponse {
    pub total_nodes: usize,
    pub ready_nodes: usize,
    pub not_ready_nodes: usize,
    pub total_workloads: usize,
    pub total_instances: usize,
    pub running_instances: usize,
    pub pending_instances: usize,
    pub failed_instances: usize,
    pub total_cpu_capacity: f32,
    pub total_memory_mb: u64,
    pub total_cpu_allocatable: f32,
    pub total_memory_allocatable_mb: u64,
}

// ============================================================================
// Conversion Helpers
// ============================================================================

impl From<CreateWorkloadRequest> for WorkloadDefinition {
    fn from(req: CreateWorkloadRequest) -> Self {
        WorkloadDefinition {
            id: Uuid::new_v4(),
            name: req.name,
            containers: req.containers.into_iter().map(Into::into).collect(),
            replicas: req.replicas,
            labels: req.labels,
        }
    }
}

impl From<ContainerConfigRequest> for ContainerConfig {
    fn from(req: ContainerConfigRequest) -> Self {
        ContainerConfig {
            name: req.name,
            image: req.image,
            command: req.command,
            args: req.args,
            env_vars: req.env_vars,
            ports: req.ports.into_iter().map(Into::into).collect(),
            resource_requests: req.resource_requests.into(),
        }
    }
}

impl From<PortMappingRequest> for PortMapping {
    fn from(req: PortMappingRequest) -> Self {
        PortMapping {
            container_port: req.container_port,
            host_port: req.host_port,
            protocol: req.protocol,
        }
    }
}

impl From<ResourceRequestsRequest> for NodeResources {
    fn from(req: ResourceRequestsRequest) -> Self {
        NodeResources {
            cpu_cores: req.cpu_cores,
            memory_mb: req.memory_mb,
            disk_mb: req.disk_mb,
        }
    }
}

impl From<WorkloadDefinition> for WorkloadResponse {
    fn from(def: WorkloadDefinition) -> Self {
        WorkloadResponse {
            id: def.id,
            name: def.name,
            replicas: def.replicas,
            labels: def.labels,
            containers: def.containers.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ContainerConfig> for ContainerConfigResponse {
    fn from(cfg: ContainerConfig) -> Self {
        ContainerConfigResponse {
            name: cfg.name,
            image: cfg.image,
            command: cfg.command,
            args: cfg.args,
            env_vars: cfg.env_vars,
            ports: cfg.ports.into_iter().map(Into::into).collect(),
            resource_requests: cfg.resource_requests.into(),
        }
    }
}

impl From<PortMapping> for PortMappingResponse {
    fn from(pm: PortMapping) -> Self {
        PortMappingResponse {
            container_port: pm.container_port,
            host_port: pm.host_port,
            protocol: pm.protocol,
        }
    }
}

impl From<NodeResources> for ResourceRequestsResponse {
    fn from(res: NodeResources) -> Self {
        ResourceRequestsResponse {
            cpu_cores: res.cpu_cores,
            memory_mb: res.memory_mb,
            disk_mb: res.disk_mb,
        }
    }
}

impl From<Node> for NodeResponse {
    fn from(node: Node) -> Self {
        NodeResponse {
            id: node.id,
            address: node.address,
            status: format!("{:?}", node.status),
            labels: node.labels,
            resources_capacity: node.resources_capacity.into(),
            resources_allocatable: node.resources_allocatable.into(),
        }
    }
}

impl From<WorkloadInstance> for InstanceResponse {
    fn from(inst: WorkloadInstance) -> Self {
        InstanceResponse {
            id: inst.id,
            workload_id: inst.workload_id,
            node_id: inst.node_id,
            container_ids: inst.container_ids,
            status: format!("{:?}", inst.status),
        }
    }
}

// ============================================================================
// Workload Handlers
// ============================================================================

/// Create a new workload.
pub async fn create_workload(
    State(state): State<ApiState>,
    Json(request): Json<CreateWorkloadRequest>,
) -> ApiResult<impl IntoResponse> {
    // Validate request
    if request.name.is_empty() {
        return Err(ApiError::validation_error("Workload name cannot be empty"));
    }

    if request.containers.is_empty() {
        return Err(ApiError::validation_error("Workload must have at least one container"));
    }

    if request.replicas == 0 {
        return Err(ApiError::validation_error("Replicas must be at least 1"));
    }

    // Convert to workload definition
    let workload: WorkloadDefinition = request.into();

    // Store workload
    state
        .state_store
        .put_workload(workload.clone())
        .await
        .map_err(ApiError::from)?;

    // Send to orchestrator for scheduling
    state
        .workload_tx
        .send(workload.clone())
        .await
        .map_err(|_| ApiError::internal_error("Failed to submit workload to orchestrator"))?;

    let response: WorkloadResponse = workload.into();
    Ok((StatusCode::CREATED, Json(response)))
}

/// List all workloads.
pub async fn list_workloads(
    State(state): State<ApiState>,
) -> ApiResult<impl IntoResponse> {
    let workloads = state
        .state_store
        .list_workloads()
        .await
        .map_err(ApiError::from)?;

    let items: Vec<WorkloadResponse> = workloads.into_iter().map(Into::into).collect();
    let count = items.len();

    Ok(Json(ListResponse { items, count }))
}

/// Get a workload by ID.
pub async fn get_workload(
    State(state): State<ApiState>,
    Path(workload_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let workload = state
        .state_store
        .get_workload(&workload_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found("Workload", &workload_id.to_string()))?;

    let response: WorkloadResponse = workload.into();
    Ok(Json(response))
}

/// Update a workload.
pub async fn update_workload(
    State(state): State<ApiState>,
    Path(workload_id): Path<Uuid>,
    Json(request): Json<CreateWorkloadRequest>,
) -> ApiResult<impl IntoResponse> {
    // Check workload exists
    let _ = state
        .state_store
        .get_workload(&workload_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found("Workload", &workload_id.to_string()))?;

    // Validate request
    if request.name.is_empty() {
        return Err(ApiError::validation_error("Workload name cannot be empty"));
    }

    if request.containers.is_empty() {
        return Err(ApiError::validation_error("Workload must have at least one container"));
    }

    // Create updated workload with same ID
    let workload = WorkloadDefinition {
        id: workload_id,
        name: request.name,
        containers: request.containers.into_iter().map(Into::into).collect(),
        replicas: request.replicas,
        labels: request.labels,
    };

    // Store updated workload
    state
        .state_store
        .put_workload(workload.clone())
        .await
        .map_err(ApiError::from)?;

    // Send to orchestrator for re-reconciliation
    state
        .workload_tx
        .send(workload.clone())
        .await
        .map_err(|_| ApiError::internal_error("Failed to submit workload update to orchestrator"))?;

    let response: WorkloadResponse = workload.into();
    Ok(Json(response))
}

/// Delete a workload.
pub async fn delete_workload(
    State(state): State<ApiState>,
    Path(workload_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    // Check workload exists
    let _ = state
        .state_store
        .get_workload(&workload_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found("Workload", &workload_id.to_string()))?;

    // Delete instances first
    state
        .state_store
        .delete_instances_for_workload(&workload_id)
        .await
        .map_err(ApiError::from)?;

    // Delete workload
    state
        .state_store
        .delete_workload(&workload_id)
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}

/// List instances for a workload.
pub async fn list_workload_instances(
    State(state): State<ApiState>,
    Path(workload_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    // Check workload exists
    let _ = state
        .state_store
        .get_workload(&workload_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found("Workload", &workload_id.to_string()))?;

    let instances = state
        .state_store
        .list_instances_for_workload(&workload_id)
        .await
        .map_err(ApiError::from)?;

    let items: Vec<InstanceResponse> = instances.into_iter().map(Into::into).collect();
    let count = items.len();

    Ok(Json(ListResponse { items, count }))
}

// ============================================================================
// Node Handlers
// ============================================================================

/// List all nodes.
pub async fn list_nodes(
    State(state): State<ApiState>,
) -> ApiResult<impl IntoResponse> {
    let nodes = state
        .state_store
        .list_nodes()
        .await
        .map_err(ApiError::from)?;

    let items: Vec<NodeResponse> = nodes.into_iter().map(Into::into).collect();
    let count = items.len();

    Ok(Json(ListResponse { items, count }))
}

/// Get a node by ID.
pub async fn get_node(
    State(state): State<ApiState>,
    Path(node_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let node = state
        .state_store
        .get_node(&node_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found("Node", &node_id.to_string()))?;

    let response: NodeResponse = node.into();
    Ok(Json(response))
}

// ============================================================================
// Cluster Handlers
// ============================================================================

/// Get cluster status.
pub async fn get_cluster_status(
    State(state): State<ApiState>,
) -> ApiResult<impl IntoResponse> {
    // Get nodes
    let nodes = state
        .state_store
        .list_nodes()
        .await
        .map_err(ApiError::from)?;

    let total_nodes = nodes.len();
    let ready_nodes = nodes.iter().filter(|n| n.status == NodeStatus::Ready).count();
    let not_ready_nodes = total_nodes - ready_nodes;

    // Calculate resource totals
    let (total_cpu_capacity, total_memory_mb) = nodes.iter().fold((0.0f32, 0u64), |acc, n| {
        (acc.0 + n.resources_capacity.cpu_cores, acc.1 + n.resources_capacity.memory_mb)
    });

    let (total_cpu_allocatable, total_memory_allocatable_mb) = nodes.iter().fold((0.0f32, 0u64), |acc, n| {
        (acc.0 + n.resources_allocatable.cpu_cores, acc.1 + n.resources_allocatable.memory_mb)
    });

    // Get workloads
    let workloads = state
        .state_store
        .list_workloads()
        .await
        .map_err(ApiError::from)?;

    let total_workloads = workloads.len();

    // Get all instances
    let instances = state
        .state_store
        .list_all_instances()
        .await
        .map_err(ApiError::from)?;

    let total_instances = instances.len();
    let running_instances = instances
        .iter()
        .filter(|i| i.status == WorkloadInstanceStatus::Running)
        .count();
    let pending_instances = instances
        .iter()
        .filter(|i| i.status == WorkloadInstanceStatus::Pending)
        .count();
    let failed_instances = instances
        .iter()
        .filter(|i| i.status == WorkloadInstanceStatus::Failed)
        .count();

    Ok(Json(ClusterStatusResponse {
        total_nodes,
        ready_nodes,
        not_ready_nodes,
        total_workloads,
        total_instances,
        running_instances,
        pending_instances,
        failed_instances,
        total_cpu_capacity,
        total_memory_mb,
        total_cpu_allocatable,
        total_memory_allocatable_mb,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_workload_request_conversion() {
        let request = CreateWorkloadRequest {
            name: "test-workload".to_string(),
            containers: vec![ContainerConfigRequest {
                name: "nginx".to_string(),
                image: "nginx:latest".to_string(),
                command: None,
                args: None,
                env_vars: HashMap::new(),
                ports: vec![PortMappingRequest {
                    container_port: 80,
                    host_port: Some(8080),
                    protocol: "tcp".to_string(),
                }],
                resource_requests: ResourceRequestsRequest {
                    cpu_cores: 0.5,
                    memory_mb: 512,
                    disk_mb: 1024,
                },
            }],
            replicas: 3,
            labels: HashMap::new(),
        };

        let workload: WorkloadDefinition = request.into();
        assert_eq!(workload.name, "test-workload");
        assert_eq!(workload.replicas, 3);
        assert_eq!(workload.containers.len(), 1);
        assert_eq!(workload.containers[0].image, "nginx:latest");
    }

    #[test]
    fn test_node_response_conversion() {
        let node = Node {
            id: Uuid::new_v4(),
            address: "10.0.0.1:8080".to_string(),
            status: NodeStatus::Ready,
            labels: HashMap::new(),
            resources_capacity: NodeResources {
                cpu_cores: 4.0,
                memory_mb: 8192,
                disk_mb: 102400,
            },
            resources_allocatable: NodeResources {
                cpu_cores: 3.6,
                memory_mb: 7372,
                disk_mb: 92160,
            },
        };

        let response: NodeResponse = node.clone().into();
        assert_eq!(response.id, node.id);
        assert_eq!(response.status, "Ready");
        assert_eq!(response.resources_capacity.cpu_cores, 4.0);
    }

    #[test]
    fn test_instance_response_conversion() {
        let instance = WorkloadInstance {
            id: Uuid::new_v4(),
            workload_id: Uuid::new_v4(),
            node_id: Uuid::new_v4(),
            container_ids: vec!["container-1".to_string()],
            status: WorkloadInstanceStatus::Running,
        };

        let response: InstanceResponse = instance.clone().into();
        assert_eq!(response.id, instance.id);
        assert_eq!(response.status, "Running");
    }
}
