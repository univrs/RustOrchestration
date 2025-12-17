//! Status command - show cluster and workload status.

use clap::Args;
use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::client::ApiClient;
use crate::output::{self, print_data, section};
use crate::OutputFormat;

/// Arguments for the status command.
#[derive(Args)]
pub struct StatusArgs {
    /// Show detailed status
    #[arg(short, long)]
    detailed: bool,

    /// Filter by workload name
    #[arg(short, long)]
    workload: Option<String>,

    /// Show only nodes
    #[arg(long)]
    nodes_only: bool,

    /// Show only workloads
    #[arg(long)]
    workloads_only: bool,
}

/// Cluster status response from API.
#[derive(Debug, Deserialize)]
struct ClusterStatusResponse {
    node_id: String,
    cluster_size: usize,
    is_leader: bool,
    total_workloads: usize,
    total_instances: usize,
}

/// Node response from API.
#[derive(Debug, Serialize, Deserialize, Tabled)]
struct NodeResponse {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Address")]
    address: String,
    #[tabled(rename = "CPU")]
    available_cpu_millicores: u64,
    #[tabled(rename = "Memory (MB)")]
    available_memory_mb: u64,
}

/// Workload response from API.
#[derive(Debug, Serialize, Deserialize, Tabled)]
struct WorkloadResponse {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Replicas")]
    replicas: u32,
    #[tabled(rename = "Image")]
    #[tabled(display_with = "display_first_image")]
    containers: Vec<ContainerInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContainerInfo {
    name: String,
    image: String,
}

fn display_first_image(containers: &Vec<ContainerInfo>) -> String {
    containers
        .first()
        .map(|c| c.image.clone())
        .unwrap_or_else(|| "-".to_string())
}

/// Workload instance response from API.
#[derive(Debug, Serialize, Deserialize, Tabled)]
struct InstanceResponse {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Workload")]
    workload_id: String,
    #[tabled(rename = "Node")]
    node_id: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Containers")]
    #[tabled(display_with = "display_container_count")]
    container_ids: Vec<String>,
}

fn display_container_count(ids: &Vec<String>) -> String {
    format!("{}", ids.len())
}

/// Execute the status command.
pub async fn execute(args: StatusArgs, api_url: &str, format: OutputFormat) -> anyhow::Result<()> {
    let client = match ApiClient::authenticated(api_url).await {
        Ok(c) => c,
        Err(e) => {
            output::warn(&format!("Could not load identity for authentication: {}", e));
            output::info("Using unauthenticated client (some endpoints may fail)");
            ApiClient::new(api_url)
        }
    };

    // Show cluster status first (unless filtering)
    if !args.nodes_only && !args.workloads_only {
        match client.get::<ClusterStatusResponse>("/api/v1/cluster/status").await {
            Ok(status) => {
                section("Cluster Status");
                println!("  Node ID:     {}", status.node_id);
                println!("  Cluster:     {} nodes", status.cluster_size);
                println!("  Role:        {}", if status.is_leader { "Leader" } else { "Follower" });
                println!("  Workloads:   {}", status.total_workloads);
                println!("  Instances:   {}", status.total_instances);
            }
            Err(e) => {
                output::error(&format!("Failed to get cluster status: {}", e));
            }
        }
    }

    // Show nodes
    if !args.workloads_only {
        section("Nodes");
        match client.get::<Vec<NodeResponse>>("/api/v1/nodes").await {
            Ok(nodes) => {
                print_data(&nodes, format)?;
            }
            Err(e) => {
                output::error(&format!("Failed to get nodes: {}", e));
            }
        }
    }

    // Show workloads
    if !args.nodes_only {
        section("Workloads");
        match client.get::<Vec<WorkloadResponse>>("/api/v1/workloads").await {
            Ok(workloads) => {
                let filtered: Vec<_> = if let Some(ref name) = args.workload {
                    workloads
                        .into_iter()
                        .filter(|w| w.name.contains(name))
                        .collect()
                } else {
                    workloads
                };
                print_data(&filtered, format)?;

                // Show instances if detailed
                if args.detailed && !filtered.is_empty() {
                    section("Instances");
                    for workload in &filtered {
                        let path = format!("/api/v1/workloads/{}/instances", workload.id);
                        match client.get::<Vec<InstanceResponse>>(&path).await {
                            Ok(instances) => {
                                if !instances.is_empty() {
                                    println!("\n  Workload: {} ({})", workload.name, workload.id);
                                    print_data(&instances, format)?;
                                }
                            }
                            Err(e) => {
                                output::error(&format!(
                                    "Failed to get instances for {}: {}",
                                    workload.name, e
                                ));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                output::error(&format!("Failed to get workloads: {}", e));
            }
        }
    }

    Ok(())
}
