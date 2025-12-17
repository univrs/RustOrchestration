//! Deploy command - deploy a new workload.

use clap::Args;
use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::client::ApiClient;
use crate::error::CliError;
use crate::output::{self, print_item};
use crate::OutputFormat;

/// Arguments for the deploy command.
#[derive(Args)]
pub struct DeployArgs {
    /// Workload name
    #[arg(short, long)]
    name: String,

    /// Container image
    #[arg(short, long)]
    image: String,

    /// Number of replicas
    #[arg(short, long, default_value = "1")]
    replicas: u32,

    /// Container port
    #[arg(short, long)]
    port: Option<u16>,

    /// CPU request in millicores
    #[arg(long, default_value = "100")]
    cpu: u64,

    /// Memory request in MB
    #[arg(long, default_value = "128")]
    memory: u64,

    /// Environment variables (KEY=VALUE format, can be repeated)
    #[arg(short, long, value_parser = parse_env_var)]
    env: Vec<(String, String)>,

    /// Labels (key=value format, can be repeated)
    #[arg(short, long, value_parser = parse_env_var)]
    label: Vec<(String, String)>,
}

fn parse_env_var(s: &str) -> std::result::Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid format '{}', expected KEY=VALUE", s));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

/// Create workload request.
#[derive(Debug, Serialize)]
struct CreateWorkloadRequest {
    name: String,
    replicas: u32,
    labels: std::collections::HashMap<String, String>,
    containers: Vec<ContainerRequest>,
}

#[derive(Debug, Serialize)]
struct ContainerRequest {
    name: String,
    image: String,
    ports: Vec<PortRequest>,
    env: std::collections::HashMap<String, String>,
    resources: ResourceRequest,
}

#[derive(Debug, Serialize)]
struct PortRequest {
    container_port: u16,
    protocol: String,
}

#[derive(Debug, Serialize)]
struct ResourceRequest {
    cpu_millicores: u64,
    memory_mb: u64,
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
}

/// Execute the deploy command.
pub async fn execute(args: DeployArgs, api_url: &str, format: OutputFormat) -> anyhow::Result<()> {
    let client = ApiClient::authenticated(api_url).await.map_err(|e| {
        CliError::config_error(format!(
            "Authentication required for deploy. Run 'orch init' first. Error: {}",
            e
        ))
    })?;

    output::info(&format!("Deploying workload '{}'...", args.name));

    // Build the request
    let mut labels = std::collections::HashMap::new();
    for (key, value) in args.label {
        labels.insert(key, value);
    }

    let mut env = std::collections::HashMap::new();
    for (key, value) in args.env {
        env.insert(key, value);
    }

    let ports = if let Some(port) = args.port {
        vec![PortRequest {
            container_port: port,
            protocol: "TCP".to_string(),
        }]
    } else {
        vec![]
    };

    let request = CreateWorkloadRequest {
        name: args.name.clone(),
        replicas: args.replicas,
        labels,
        containers: vec![ContainerRequest {
            name: args.name.clone(),
            image: args.image.clone(),
            ports,
            env,
            resources: ResourceRequest {
                cpu_millicores: args.cpu,
                memory_mb: args.memory,
            },
        }],
    };

    // Send the request
    let response: WorkloadResponse = client.post("/api/v1/workloads", &request).await?;

    output::success(&format!("Workload '{}' deployed successfully!", args.name));
    print_item(&response, format)?;

    output::info(&format!(
        "Scheduling {} replica(s) with image '{}'",
        args.replicas, args.image
    ));
    output::info("Use 'orch status' to check deployment progress");

    Ok(())
}
