// Checkpoint system for resumable playbook execution

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::output::errors::NexusError;
use crate::parser::ast::Value;

/// Current checkpoint format version
const CHECKPOINT_VERSION: &str = "1.0";

/// A checkpoint representing the state of playbook execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Checkpoint format version
    pub version: String,
    /// Path to the playbook file
    pub playbook_path: PathBuf,
    /// SHA256 hash of playbook content (to detect changes)
    pub playbook_hash: String,
    /// Path to the inventory file
    pub inventory_path: PathBuf,
    /// Set of completed tasks (host, task_id)
    pub completed_tasks: HashSet<TaskKey>,
    /// Current variables (playbook vars + registered vars)
    pub variables: HashMap<String, Value>,
    /// Registered results per host
    pub registered_results: HashMap<String, HashMap<String, Value>>,
    /// Handler notifications (handler_name -> set of hosts)
    pub handler_notifications: HashMap<String, HashSet<String>>,
    /// When this checkpoint was created
    pub timestamp: DateTime<Utc>,
    /// Last task that was executed (for display)
    pub last_task: Option<String>,
    /// Last host that was executed (for display)
    pub last_host: Option<String>,
}

/// Unique identifier for a task execution on a host
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskKey {
    /// Host name
    pub host: String,
    /// Task identifier (name or index)
    pub task_id: String,
}

impl TaskKey {
    pub fn new(host: impl Into<String>, task_id: impl Into<String>) -> Self {
        TaskKey {
            host: host.into(),
            task_id: task_id.into(),
        }
    }
}

impl Checkpoint {
    /// Create a new checkpoint
    pub fn new(
        playbook_path: PathBuf,
        playbook_content: &str,
        inventory_path: PathBuf,
        variables: HashMap<String, Value>,
    ) -> Self {
        let playbook_hash = calculate_hash(playbook_content);

        Checkpoint {
            version: CHECKPOINT_VERSION.to_string(),
            playbook_path,
            playbook_hash,
            inventory_path,
            completed_tasks: HashSet::new(),
            variables,
            registered_results: HashMap::new(),
            handler_notifications: HashMap::new(),
            timestamp: Utc::now(),
            last_task: None,
            last_host: None,
        }
    }

    /// Mark a task as completed for a specific host
    pub fn mark_completed(&mut self, host: &str, task_id: &str) {
        self.completed_tasks.insert(TaskKey::new(host, task_id));
        self.last_task = Some(task_id.to_string());
        self.last_host = Some(host.to_string());
        self.timestamp = Utc::now();
    }

    /// Check if a task should be skipped (already completed)
    pub fn should_skip(&self, host: &str, task_id: &str) -> bool {
        self.completed_tasks.contains(&TaskKey::new(host, task_id))
    }

    /// Update variables in the checkpoint
    pub fn update_variables(&mut self, variables: HashMap<String, Value>) {
        self.variables = variables;
    }

    /// Store registered result for a host
    pub fn store_registered(&mut self, host: &str, var_name: &str, value: Value) {
        self.registered_results
            .entry(host.to_string())
            .or_default()
            .insert(var_name.to_string(), value);
    }

    /// Notify handler for a host
    pub fn notify_handler(&mut self, handler_name: &str, host: &str) {
        self.handler_notifications
            .entry(handler_name.to_string())
            .or_default()
            .insert(host.to_string());
    }

    /// Verify checkpoint is compatible with current playbook
    pub fn verify(&self, playbook_content: &str, inventory_path: &Path) -> Result<(), NexusError> {
        // Check version compatibility
        if self.version != CHECKPOINT_VERSION {
            return Err(NexusError::Runtime {
                function: None,
                message: format!(
                    "Checkpoint version mismatch: expected {}, found {}",
                    CHECKPOINT_VERSION, self.version
                ),
                suggestion: Some("Delete the checkpoint and run from the beginning".to_string()),
            });
        }

        // Verify playbook hasn't changed
        let current_hash = calculate_hash(playbook_content);
        if self.playbook_hash != current_hash {
            return Err(NexusError::Runtime {
                function: None,
                message: "Playbook has been modified since checkpoint was created".to_string(),
                suggestion: Some("Delete the checkpoint and run from the beginning, or use --force-resume to ignore this warning".to_string()),
            });
        }

        // Verify inventory path matches
        if self.inventory_path != inventory_path {
            return Err(NexusError::Runtime {
                function: None,
                message: format!(
                    "Inventory path mismatch: checkpoint uses {}, current is {}",
                    self.inventory_path.display(),
                    inventory_path.display()
                ),
                suggestion: Some("Use the same inventory file as the checkpoint".to_string()),
            });
        }

        Ok(())
    }
}

/// Manager for checkpoint persistence and loading
pub struct CheckpointManager {
    /// Directory for storing checkpoints
    checkpoint_dir: PathBuf,
}

impl CheckpointManager {
    /// Create a new checkpoint manager
    pub fn new() -> Result<Self, NexusError> {
        let checkpoint_dir = Self::default_checkpoint_dir()?;

        // Create checkpoint directory if it doesn't exist
        if !checkpoint_dir.exists() {
            fs::create_dir_all(&checkpoint_dir).map_err(|e| NexusError::Io {
                message: format!("Failed to create checkpoint directory: {}", e),
                path: Some(checkpoint_dir.clone()),
            })?;
        }

        Ok(CheckpointManager { checkpoint_dir })
    }

    /// Create checkpoint manager with custom directory
    pub fn with_dir(checkpoint_dir: PathBuf) -> Result<Self, NexusError> {
        // Create checkpoint directory if it doesn't exist
        if !checkpoint_dir.exists() {
            fs::create_dir_all(&checkpoint_dir).map_err(|e| NexusError::Io {
                message: format!("Failed to create checkpoint directory: {}", e),
                path: Some(checkpoint_dir.clone()),
            })?;
        }

        Ok(CheckpointManager { checkpoint_dir })
    }

    /// Get the default checkpoint directory (.nexus/checkpoints)
    fn default_checkpoint_dir() -> Result<PathBuf, NexusError> {
        let cwd = std::env::current_dir().map_err(|e| NexusError::Io {
            message: format!("Failed to get current directory: {}", e),
            path: None,
        })?;

        Ok(cwd.join(".nexus").join("checkpoints"))
    }

    /// Get the checkpoint file path for a playbook
    pub fn checkpoint_path(&self, playbook: &Path) -> PathBuf {
        let hash = calculate_hash(&playbook.to_string_lossy());
        let filename = format!("{}.json", &hash[..16]);
        self.checkpoint_dir.join(filename)
    }

    /// Save a checkpoint to disk
    pub fn save(&self, checkpoint: &Checkpoint) -> Result<PathBuf, NexusError> {
        let path = self.checkpoint_path(&checkpoint.playbook_path);

        let json = serde_json::to_string_pretty(checkpoint).map_err(|e| NexusError::Runtime {
            function: None,
            message: format!("Failed to serialize checkpoint: {}", e),
            suggestion: None,
        })?;

        fs::write(&path, json).map_err(|e| NexusError::Io {
            message: format!("Failed to write checkpoint: {}", e),
            path: Some(path.clone()),
        })?;

        Ok(path)
    }

    /// Load a checkpoint from a specific path
    pub fn load(&self, path: &Path) -> Result<Checkpoint, NexusError> {
        let json = fs::read_to_string(path).map_err(|e| NexusError::Io {
            message: format!("Failed to read checkpoint: {}", e),
            path: Some(path.to_path_buf()),
        })?;

        let checkpoint: Checkpoint =
            serde_json::from_str(&json).map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Failed to parse checkpoint: {}", e),
                suggestion: Some("The checkpoint file may be corrupted".to_string()),
            })?;

        Ok(checkpoint)
    }

    /// Load the latest checkpoint for a playbook
    pub fn load_latest(&self, playbook: &Path) -> Result<Option<Checkpoint>, NexusError> {
        let path = self.checkpoint_path(playbook);

        if !path.exists() {
            return Ok(None);
        }

        self.load(&path).map(Some)
    }

    /// Delete checkpoint for a playbook
    pub fn cleanup(&self, playbook: &Path) -> Result<(), NexusError> {
        let path = self.checkpoint_path(playbook);

        if path.exists() {
            fs::remove_file(&path).map_err(|e| NexusError::Io {
                message: format!("Failed to delete checkpoint: {}", e),
                path: Some(path),
            })?;
        }

        Ok(())
    }

    /// List all checkpoints
    pub fn list_all(&self) -> Result<Vec<CheckpointInfo>, NexusError> {
        let mut checkpoints = Vec::new();

        if !self.checkpoint_dir.exists() {
            return Ok(checkpoints);
        }

        let entries = fs::read_dir(&self.checkpoint_dir).map_err(|e| NexusError::Io {
            message: format!("Failed to read checkpoint directory: {}", e),
            path: Some(self.checkpoint_dir.clone()),
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| NexusError::Io {
                message: format!("Failed to read directory entry: {}", e),
                path: Some(self.checkpoint_dir.clone()),
            })?;

            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                match self.load(&path) {
                    Ok(checkpoint) => {
                        checkpoints.push(CheckpointInfo {
                            path: path.clone(),
                            playbook_path: checkpoint.playbook_path.clone(),
                            timestamp: checkpoint.timestamp,
                            completed_tasks: checkpoint.completed_tasks.len(),
                            last_task: checkpoint.last_task.clone(),
                            last_host: checkpoint.last_host.clone(),
                        });
                    }
                    Err(_) => {
                        // Skip invalid checkpoints
                        continue;
                    }
                }
            }
        }

        // Sort by timestamp (newest first)
        checkpoints.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(checkpoints)
    }

    /// Clean old checkpoints (older than N days)
    pub fn clean_old(&self, days: u64) -> Result<usize, NexusError> {
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);
        let checkpoints = self.list_all()?;
        let mut cleaned = 0;

        for info in checkpoints {
            if info.timestamp < cutoff {
                fs::remove_file(&info.path).ok();
                cleaned += 1;
            }
        }

        Ok(cleaned)
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new().expect("Failed to create checkpoint manager")
    }
}

/// Information about a checkpoint (for listing)
#[derive(Debug, Clone)]
pub struct CheckpointInfo {
    pub path: PathBuf,
    pub playbook_path: PathBuf,
    pub timestamp: DateTime<Utc>,
    pub completed_tasks: usize,
    pub last_task: Option<String>,
    pub last_host: Option<String>,
}

/// Calculate SHA256 hash of a string
fn calculate_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_checkpoint_creation() {
        let checkpoint = Checkpoint::new(
            PathBuf::from("/tmp/test.yml"),
            "test content",
            PathBuf::from("/tmp/inventory.yml"),
            HashMap::new(),
        );

        assert_eq!(checkpoint.version, CHECKPOINT_VERSION);
        assert_eq!(checkpoint.completed_tasks.len(), 0);
    }

    #[test]
    fn test_task_completion() {
        let mut checkpoint = Checkpoint::new(
            PathBuf::from("/tmp/test.yml"),
            "test content",
            PathBuf::from("/tmp/inventory.yml"),
            HashMap::new(),
        );

        checkpoint.mark_completed("host1", "task1");
        assert!(checkpoint.should_skip("host1", "task1"));
        assert!(!checkpoint.should_skip("host1", "task2"));
        assert!(!checkpoint.should_skip("host2", "task1"));
    }

    #[test]
    fn test_checkpoint_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let manager = CheckpointManager::with_dir(temp_dir.path().to_path_buf()).unwrap();

        let mut checkpoint = Checkpoint::new(
            PathBuf::from("/tmp/test.yml"),
            "test content",
            PathBuf::from("/tmp/inventory.yml"),
            HashMap::new(),
        );

        checkpoint.mark_completed("host1", "task1");

        let path = manager.save(&checkpoint).unwrap();
        assert!(path.exists());

        let loaded = manager.load(&path).unwrap();
        assert_eq!(loaded.completed_tasks.len(), 1);
        assert!(loaded.should_skip("host1", "task1"));
    }

    #[test]
    fn test_hash_calculation() {
        let hash1 = calculate_hash("test content");
        let hash2 = calculate_hash("test content");
        let hash3 = calculate_hash("different content");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_checkpoint_verification() {
        let content = "test content";
        let checkpoint = Checkpoint::new(
            PathBuf::from("/tmp/test.yml"),
            content,
            PathBuf::from("/tmp/inventory.yml"),
            HashMap::new(),
        );

        // Same content should verify
        assert!(checkpoint
            .verify(content, &PathBuf::from("/tmp/inventory.yml"))
            .is_ok());

        // Different content should fail
        assert!(checkpoint
            .verify("different", &PathBuf::from("/tmp/inventory.yml"))
            .is_err());

        // Different inventory should fail
        assert!(checkpoint
            .verify(content, &PathBuf::from("/tmp/other.yml"))
            .is_err());
    }
}
