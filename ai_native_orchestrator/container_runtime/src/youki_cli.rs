//! CLI-based Youki container runtime.
//!
//! This module provides a container runtime implementation that uses the
//! `youki` CLI binary to manage OCI containers.
//!
//! # Requirements
//!
//! - `youki` binary must be installed and in PATH
//! - Linux with cgroups v2
//! - Root privileges (or appropriate capabilities)
//!
//! # Log Collection
//!
//! Container stdout/stderr is captured to log files stored at:
//! `{state_root}/{container_id}/container.log`
//!
//! The log file contains both stdout and stderr interleaved with timestamps.
//! Use `get_logs()` or `stream_logs()` to access container logs.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use container_runtime_interface::{ContainerRuntime, ContainerStatus, CreateContainerOptions};
use orchestrator_shared_types::{ContainerConfig, ContainerId, NodeId, OrchestrationError, Result};

use crate::image::ImageManager;
use crate::oci_bundle::OciBundleBuilder;

/// Errors specific to Youki CLI operations.
#[derive(Debug, thiserror::Error)]
pub enum YoukiCliError {
    #[error("Youki binary not found: {0}")]
    BinaryNotFound(String),

    #[error("Youki command failed: {command} - {message}")]
    CommandFailed { command: String, message: String },

    #[error("Container not found: {0}")]
    ContainerNotFound(String),

    #[error("Invalid state output: {0}")]
    InvalidState(String),

    #[error("Image error: {0}")]
    Image(#[from] crate::image::ImageError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Log error: {0}")]
    LogError(String),
}

/// A single log entry from a container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Timestamp of the log entry (RFC3339 format)
    pub timestamp: String,
    /// Stream source: "stdout" or "stderr"
    pub stream: String,
    /// The log message
    pub message: String,
}

/// Options for retrieving container logs.
#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    /// Return only the last N lines
    pub tail: Option<usize>,
    /// Include timestamps in output
    pub timestamps: bool,
    /// Only return logs since this timestamp (RFC3339)
    pub since: Option<String>,
    /// Only return logs until this timestamp (RFC3339)
    pub until: Option<String>,
    /// Follow log output (like tail -f)
    pub follow: bool,
}

/// Log stream receiver for follow mode.
pub type LogReceiver = broadcast::Receiver<LogEntry>;

/// Internal handle for active log streams.
struct LogStreamHandle {
    sender: broadcast::Sender<LogEntry>,
    /// Task handle for the log watcher
    _watcher: tokio::task::JoinHandle<()>,
}

impl From<YoukiCliError> for OrchestrationError {
    fn from(err: YoukiCliError) -> Self {
        OrchestrationError::RuntimeError(err.to_string())
    }
}

/// Container state from `youki state` command.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct YoukiState {
    #[serde(rename = "ociVersion")]
    pub oci_version: String,
    pub id: String,
    pub status: String,
    pub pid: Option<i32>,
    pub bundle: String,
    #[serde(default)]
    pub annotations: HashMap<String, String>,
    #[serde(rename = "created", default)]
    pub created: Option<String>,
}

/// Internal container state tracking.
#[derive(Debug, Clone)]
pub struct ContainerState {
    pub id: ContainerId,
    pub node_id: NodeId,
    pub bundle_path: PathBuf,
    pub status: String,
    pub pid: Option<i32>,
}

/// Configuration for YoukiCliRuntime.
#[derive(Debug, Clone)]
pub struct YoukiCliConfig {
    /// Path to youki binary (default: "youki")
    pub youki_binary: PathBuf,
    /// Base directory for container bundles
    pub bundle_root: PathBuf,
    /// Root directory for youki state
    pub state_root: PathBuf,
    /// Timeout for commands (default: 30s)
    pub command_timeout: Duration,
    /// Timeout before SIGKILL (default: 10s)
    pub stop_timeout: Duration,
}

impl Default for YoukiCliConfig {
    fn default() -> Self {
        Self {
            youki_binary: PathBuf::from("youki"),
            bundle_root: PathBuf::from("/var/lib/orchestrator/bundles"),
            state_root: PathBuf::from("/run/youki"),
            command_timeout: Duration::from_secs(30),
            stop_timeout: Duration::from_secs(10),
        }
    }
}

/// CLI-based Youki container runtime.
pub struct YoukiCliRuntime {
    config: YoukiCliConfig,
    image_manager: ImageManager,
    containers: Arc<RwLock<HashMap<String, ContainerState>>>,
    containers_by_node: Arc<RwLock<HashMap<NodeId, Vec<ContainerId>>>>,
    /// Active log streams for follow mode
    log_streams: Arc<RwLock<HashMap<String, LogStreamHandle>>>,
}

impl YoukiCliRuntime {
    /// Create a new YoukiCliRuntime with default configuration.
    pub async fn new() -> std::result::Result<Self, YoukiCliError> {
        Self::with_config(YoukiCliConfig::default()).await
    }

    /// Create with custom configuration.
    pub async fn with_config(config: YoukiCliConfig) -> std::result::Result<Self, YoukiCliError> {
        // Verify youki binary exists
        Self::verify_binary(&config.youki_binary).await?;

        // Create directories
        tokio::fs::create_dir_all(&config.bundle_root).await?;
        tokio::fs::create_dir_all(&config.state_root).await?;

        // Initialize image manager
        let image_cache = config.bundle_root.parent()
            .unwrap_or(Path::new("/var/lib/orchestrator"))
            .join("images");
        tokio::fs::create_dir_all(&image_cache).await?;
        let image_manager = ImageManager::new(&image_cache)?;

        info!("YoukiCliRuntime initialized with binary: {:?}", config.youki_binary);

        Ok(Self {
            config,
            image_manager,
            containers: Arc::new(RwLock::new(HashMap::new())),
            containers_by_node: Arc::new(RwLock::new(HashMap::new())),
            log_streams: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Verify youki binary exists and is executable.
    async fn verify_binary(binary: &Path) -> std::result::Result<(), YoukiCliError> {
        let output = Command::new(binary)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| YoukiCliError::BinaryNotFound(format!("{:?}: {}", binary, e)))?;

        if !output.status.success() {
            return Err(YoukiCliError::BinaryNotFound(format!(
                "{:?} returned non-zero exit code",
                binary
            )));
        }

        let version = String::from_utf8_lossy(&output.stdout);
        info!("Youki version: {}", version.trim());
        Ok(())
    }

    /// Get bundle path for a container.
    fn bundle_path(&self, node_id: &NodeId, container_id: &str) -> PathBuf {
        self.config.bundle_root
            .join(node_id.to_string())
            .join(container_id)
    }

    // ==================== Youki CLI Helper Methods ====================

    /// Execute youki command with timeout.
    async fn exec_youki(&self, args: &[&str]) -> std::result::Result<std::process::Output, YoukiCliError> {
        let cmd_str = format!("youki {}", args.join(" "));
        debug!("Executing: {}", cmd_str);

        let result = tokio::time::timeout(
            self.config.command_timeout,
            Command::new(&self.config.youki_binary)
                .args(args)
                .arg("--root")
                .arg(&self.config.state_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await
        .map_err(|_| YoukiCliError::Timeout(cmd_str.clone()))?
        .map_err(YoukiCliError::Io)?;

        Ok(result)
    }

    /// youki create <id> --bundle <path>
    pub async fn youki_create(&self, id: &str, bundle_path: &Path) -> std::result::Result<(), YoukiCliError> {
        let bundle_str = bundle_path.to_string_lossy();
        let output = self.exec_youki(&["create", id, "--bundle", &bundle_str]).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(YoukiCliError::CommandFailed {
                command: "create".to_string(),
                message: stderr.to_string(),
            });
        }

        debug!("Container {} created", id);
        Ok(())
    }

    /// youki start <id>
    pub async fn youki_start(&self, id: &str) -> std::result::Result<(), YoukiCliError> {
        let output = self.exec_youki(&["start", id]).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(YoukiCliError::CommandFailed {
                command: "start".to_string(),
                message: stderr.to_string(),
            });
        }

        debug!("Container {} started", id);
        Ok(())
    }

    /// youki kill <id> <signal>
    pub async fn youki_kill(&self, id: &str, signal: &str) -> std::result::Result<(), YoukiCliError> {
        let output = self.exec_youki(&["kill", id, signal]).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't error if already stopped
            if !stderr.contains("not running") && !stderr.contains("no such process") {
                return Err(YoukiCliError::CommandFailed {
                    command: format!("kill {}", signal),
                    message: stderr.to_string(),
                });
            }
        }

        debug!("Signal {} sent to container {}", signal, id);
        Ok(())
    }

    /// youki delete <id> [--force]
    pub async fn youki_delete(&self, id: &str, force: bool) -> std::result::Result<(), YoukiCliError> {
        let args = if force {
            vec!["delete", "--force", id]
        } else {
            vec!["delete", id]
        };

        let output = self.exec_youki(&args).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("not exist") && !stderr.contains("not found") {
                return Err(YoukiCliError::CommandFailed {
                    command: "delete".to_string(),
                    message: stderr.to_string(),
                });
            }
        }

        debug!("Container {} deleted", id);
        Ok(())
    }

    /// youki state <id> -> YoukiState
    pub async fn youki_state(&self, id: &str) -> std::result::Result<YoukiState, YoukiCliError> {
        let output = self.exec_youki(&["state", id]).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("not exist") || stderr.contains("not found") {
                return Err(YoukiCliError::ContainerNotFound(id.to_string()));
            }
            return Err(YoukiCliError::CommandFailed {
                command: "state".to_string(),
                message: stderr.to_string(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let state: YoukiState = serde_json::from_str(&stdout)
            .map_err(|e| YoukiCliError::InvalidState(e.to_string()))?;

        Ok(state)
    }

    // ==================== Helper Methods ====================

    /// Get container logs from state_root.
    pub async fn get_logs(&self, container_id: &str, tail: Option<usize>) -> std::result::Result<String, YoukiCliError> {
        let log_path = self.config.state_root
            .join(container_id)
            .join("container.log");

        if !log_path.exists() {
            return Ok(String::new());
        }

        let content = tokio::fs::read_to_string(&log_path).await?;

        if let Some(n) = tail {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(n);
            Ok(lines[start..].join("\n"))
        } else {
            Ok(content)
        }
    }

    /// Get basic stats from cgroups.
    pub async fn get_stats(&self, container_id: &str) -> std::result::Result<ContainerStats, YoukiCliError> {
        let cgroup_path = PathBuf::from("/sys/fs/cgroup/youki").join(container_id);

        let cpu_usage = tokio::fs::read_to_string(cgroup_path.join("cpu.stat"))
            .await
            .ok()
            .map(|s| parse_cpu_usage(&s))
            .unwrap_or(0);

        let memory_usage = tokio::fs::read_to_string(cgroup_path.join("memory.current"))
            .await
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        Ok(ContainerStats {
            container_id: container_id.to_string(),
            cpu_usage_ns: cpu_usage,
            memory_usage_bytes: memory_usage,
        })
    }

    /// Clean up container bundle.
    async fn cleanup_bundle(&self, bundle_path: &Path) -> std::result::Result<(), YoukiCliError> {
        if bundle_path.exists() {
            tokio::fs::remove_dir_all(bundle_path).await?;
        }
        Ok(())
    }
}

/// Container resource statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub container_id: String,
    pub cpu_usage_ns: u64,
    pub memory_usage_bytes: u64,
}

fn parse_cpu_usage(content: &str) -> u64 {
    for line in content.lines() {
        if line.starts_with("usage_usec") {
            if let Some(val) = line.split_whitespace().nth(1) {
                return val.parse::<u64>().unwrap_or(0) * 1000;
            }
        }
    }
    0
}

#[async_trait]
impl ContainerRuntime for YoukiCliRuntime {
    async fn init_node(&self, node_id: NodeId) -> Result<()> {
        info!("YoukiCliRuntime: Initializing node {}", node_id);

        let bundle_dir = self.config.bundle_root.join(node_id.to_string());
        tokio::fs::create_dir_all(&bundle_dir)
            .await
            .map_err(|e| OrchestrationError::RuntimeError(format!("Failed to create bundle dir: {}", e)))?;

        let mut by_node = self.containers_by_node.write().await;
        by_node.entry(node_id).or_insert_with(Vec::new);

        Ok(())
    }

    async fn create_container(
        &self,
        config: &ContainerConfig,
        options: &CreateContainerOptions,
    ) -> Result<ContainerId> {
        let container_id = format!("{}-{}", config.name, Uuid::new_v4());

        info!(
            "YoukiCliRuntime: Creating container {} on node {}",
            container_id, options.node_id
        );

        let bundle_path = self.bundle_path(&options.node_id, &container_id);

        // Create bundle directory
        tokio::fs::create_dir_all(&bundle_path)
            .await
            .map_err(|e| OrchestrationError::RuntimeError(format!("Failed to create bundle: {}", e)))?;

        // Pull image and get rootfs
        info!("Pulling image: {}", config.image);
        let rootfs_source = self.image_manager.get_rootfs(&config.image)
            .await
            .map_err(|e| OrchestrationError::RuntimeError(format!("Failed to pull image: {}", e)))?;

        // Link rootfs to bundle
        let rootfs_dest = bundle_path.join("rootfs");
        if rootfs_dest.exists() {
            tokio::fs::remove_dir_all(&rootfs_dest).await.ok();
        }

        #[cfg(unix)]
        tokio::fs::symlink(&rootfs_source, &rootfs_dest)
            .await
            .map_err(|e| OrchestrationError::RuntimeError(format!("Failed to link rootfs: {}", e)))?;

        // Build OCI bundle (generates config.json)
        let mut builder = OciBundleBuilder::new(&bundle_path)
            .with_container_config(config)
            .skip_rootfs_setup();

        // Apply resource limits
        if config.resource_requests.cpu_cores > 0.0 {
            builder = builder.with_cpu_limit(config.resource_requests.cpu_cores);
        }
        if config.resource_requests.memory_mb > 0 {
            builder = builder.with_memory_limit(config.resource_requests.memory_mb);
        }

        builder.build()
            .map_err(|e| OrchestrationError::RuntimeError(format!("Failed to build bundle: {}", e)))?;

        // youki create
        self.youki_create(&container_id, &bundle_path)
            .await
            .map_err(|e| OrchestrationError::RuntimeError(format!("youki create failed: {}", e)))?;

        // youki start
        self.youki_start(&container_id)
            .await
            .map_err(|e| OrchestrationError::RuntimeError(format!("youki start failed: {}", e)))?;

        // Track container
        let state = ContainerState {
            id: container_id.clone(),
            node_id: options.node_id,
            bundle_path,
            status: "running".to_string(),
            pid: None,
        };

        self.containers.write().await.insert(container_id.clone(), state);
        self.containers_by_node.write().await
            .entry(options.node_id)
            .or_default()
            .push(container_id.clone());

        info!("Container {} created and started", container_id);
        Ok(container_id)
    }

    async fn stop_container(&self, container_id: &ContainerId) -> Result<()> {
        info!("YoukiCliRuntime: Stopping container {}", container_id);

        // Send SIGTERM
        if let Err(e) = self.youki_kill(container_id, "SIGTERM").await {
            warn!("SIGTERM failed: {}", e);
        }

        // Wait for stop or timeout
        let deadline = tokio::time::Instant::now() + self.config.stop_timeout;
        loop {
            match self.youki_state(container_id).await {
                Ok(state) if state.status == "stopped" => break,
                Ok(_) if tokio::time::Instant::now() >= deadline => {
                    warn!("Container {} didn't stop, sending SIGKILL", container_id);
                    self.youki_kill(container_id, "SIGKILL").await.ok();
                    break;
                }
                Ok(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                Err(YoukiCliError::ContainerNotFound(_)) => break,
                Err(e) => return Err(OrchestrationError::RuntimeError(e.to_string())),
            }
        }

        // Update state
        if let Some(state) = self.containers.write().await.get_mut(container_id) {
            state.status = "stopped".to_string();
        }

        Ok(())
    }

    async fn remove_container(&self, container_id: &ContainerId) -> Result<()> {
        info!("YoukiCliRuntime: Removing container {}", container_id);

        // youki delete --force
        self.youki_delete(container_id, true)
            .await
            .map_err(|e| OrchestrationError::RuntimeError(e.to_string()))?;

        // Cleanup bundle
        if let Some(state) = self.containers.write().await.remove(container_id) {
            self.cleanup_bundle(&state.bundle_path).await.ok();

            let mut by_node = self.containers_by_node.write().await;
            if let Some(list) = by_node.get_mut(&state.node_id) {
                list.retain(|id| id != container_id);
            }
        }

        Ok(())
    }

    async fn get_container_status(&self, container_id: &ContainerId) -> Result<ContainerStatus> {
        debug!("YoukiCliRuntime: Getting status for {}", container_id);

        match self.youki_state(container_id).await {
            Ok(state) => Ok(ContainerStatus {
                id: container_id.clone(),
                state: state.status,
                exit_code: None,
                error_message: None,
            }),
            Err(YoukiCliError::ContainerNotFound(_)) => {
                let containers = self.containers.read().await;
                if let Some(state) = containers.get(container_id) {
                    Ok(ContainerStatus {
                        id: container_id.clone(),
                        state: state.status.clone(),
                        exit_code: None,
                        error_message: None,
                    })
                } else {
                    Err(OrchestrationError::RuntimeError(format!(
                        "Container not found: {}", container_id
                    )))
                }
            }
            Err(e) => Ok(ContainerStatus {
                id: container_id.clone(),
                state: "unknown".to_string(),
                exit_code: None,
                error_message: Some(e.to_string()),
            }),
        }
    }

    async fn list_containers(&self, node_id: NodeId) -> Result<Vec<ContainerStatus>> {
        debug!("YoukiCliRuntime: Listing containers for node {}", node_id);

        let by_node = self.containers_by_node.read().await;
        let ids = by_node.get(&node_id).cloned().unwrap_or_default();
        drop(by_node);

        let mut statuses = Vec::new();
        for id in ids {
            match self.get_container_status(&id).await {
                Ok(status) => statuses.push(status),
                Err(e) => {
                    warn!("Failed to get status for {}: {}", id, e);
                    statuses.push(ContainerStatus {
                        id,
                        state: "unknown".to_string(),
                        exit_code: None,
                        error_message: Some(e.to_string()),
                    });
                }
            }
        }

        Ok(statuses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = YoukiCliConfig::default();
        assert_eq!(config.youki_binary, PathBuf::from("youki"));
        assert_eq!(config.command_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_parse_cpu_usage() {
        let content = "usage_usec 12345\nuser_usec 10000\n";
        assert_eq!(parse_cpu_usage(content), 12345000);
    }

    #[test]
    fn test_youki_state_deserialize() {
        let json = r#"{
            "ociVersion": "1.0.2",
            "id": "test",
            "status": "running",
            "pid": 1234,
            "bundle": "/path/to/bundle"
        }"#;

        let state: YoukiState = serde_json::from_str(json).unwrap();
        assert_eq!(state.id, "test");
        assert_eq!(state.status, "running");
        assert_eq!(state.pid, Some(1234));
    }

    #[test]
    fn test_container_stats() {
        let stats = ContainerStats {
            container_id: "test".to_string(),
            cpu_usage_ns: 1000000,
            memory_usage_bytes: 1048576,
        };
        assert_eq!(stats.memory_usage_bytes, 1024 * 1024);
    }
}
