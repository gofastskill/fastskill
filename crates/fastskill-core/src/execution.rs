//! Script execution environment with sandboxing support

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Execution failed: {0}")]
    Failed(String),

    #[error("Script not found: {0}")]
    ScriptNotFound(String),

    #[error("Invalid script content: {0}")]
    InvalidScript(String),

    #[error("Execution timeout after {0:?}")]
    Timeout(Duration),

    #[error("Resource limit exceeded: {0}")]
    ResourceExceeded(String),

    #[error("Security violation: {0}")]
    SecurityViolation(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Execution environment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// Default timeout for script execution
    pub default_timeout: Duration,

    /// Maximum memory per execution (MB)
    pub max_memory_mb: usize,

    /// Network access policy
    pub network_policy: NetworkPolicy,

    /// File system access level
    pub filesystem_access: FileSystemAccess,

    /// Allowed commands for shell execution
    pub allowed_commands: Vec<String>,

    /// Allowed script roots - scripts must be under one of these paths if configured
    pub allowed_script_roots: Vec<PathBuf>,

    /// Environment variables to set
    pub environment_variables: HashMap<String, String>,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(30),
            max_memory_mb: 100,
            network_policy: NetworkPolicy::Restricted {
                allowed_domains: vec![],
            },
            filesystem_access: FileSystemAccess::WorkingDirectory,
            allowed_commands: vec![
                "python".to_string(),
                "python3".to_string(),
                "node".to_string(),
            ],
            allowed_script_roots: Vec::new(),
            environment_variables: HashMap::new(),
        }
    }
}

/// Network access policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkPolicy {
    /// No network access allowed
    None,
    /// Only localhost allowed
    Localhost,
    /// Restricted to specific domains
    Restricted { allowed_domains: Vec<String> },
    /// Full network access
    Full,
}

/// File system access levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileSystemAccess {
    /// No file system access
    None,
    /// Only working directory
    WorkingDirectory,
    /// Read-only access to specific paths
    ReadOnly { paths: Vec<PathBuf> },
    /// Full access (dangerous)
    Full,
}

/// Script definition for execution
#[derive(Debug, Clone)]
pub struct ScriptDefinition {
    /// Path to the script file
    pub path: PathBuf,

    /// Script content (if not reading from file)
    pub content: Option<String>,

    /// Script language/interpreter
    pub language: ScriptLanguage,

    /// Execution parameters
    pub parameters: HashMap<String, String>,

    /// Working directory for execution
    pub working_directory: Option<PathBuf>,
}

/// Supported script languages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScriptLanguage {
    Python,
    NodeJS,
    Shell,
    Rust,
}

impl ScriptLanguage {
    #[allow(dead_code)]
    fn get_command(&self) -> &'static str {
        match self {
            ScriptLanguage::Python => "python3",
            ScriptLanguage::NodeJS => "node",
            ScriptLanguage::Shell => "sh",
            ScriptLanguage::Rust => "cargo",
        }
    }
}

/// Execution context
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Skill ID that initiated the execution
    pub skill_id: String,

    /// User ID (if available)
    pub user_id: Option<String>,

    /// Session ID
    pub session_id: String,

    /// Input parameters for the execution
    pub parameters: HashMap<String, String>,

    /// Working directory override
    pub working_directory: Option<PathBuf>,

    /// Environment variables override
    pub environment_variables: HashMap<String, String>,
}

/// Execution result
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Whether execution was successful
    pub success: bool,

    /// Standard output from execution
    pub stdout: String,

    /// Standard error from execution
    pub stderr: String,

    /// Exit code (if applicable)
    pub exit_code: Option<i32>,

    /// Execution time taken
    pub execution_time: Duration,

    /// Resources used (approximate)
    pub resources_used: ResourceUsage,
}

/// Resource usage tracking
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    /// Memory used (MB)
    pub memory_mb: f64,

    /// CPU time used (seconds)
    pub cpu_seconds: f64,
}

/// Security sandbox for script execution
pub struct ExecutionSandbox {
    config: ExecutionConfig,
}

impl ExecutionSandbox {
    /// Create a new execution sandbox with the given configuration
    pub fn new(config: ExecutionConfig) -> Result<Self, ExecutionError> {
        Ok(Self { config })
    }

    /// Execute a script with the provided context
    pub async fn execute_script(
        &self,
        script: ScriptDefinition,
        context: ExecutionContext,
    ) -> Result<ExecutionResult, ExecutionError> {
        // Validate script for security
        self.validate_script(&script)?;

        let start_time = std::time::Instant::now();

        // For now, fallback to user's environment execution
        // In the future, this would use proper sandboxing
        let result = self.execute_in_user_environment(script, context).await?;

        let execution_time = start_time.elapsed();

        Ok(ExecutionResult {
            success: result.exit_code.unwrap_or(0) == 0,
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
            execution_time,
            resources_used: ResourceUsage::default(), // Would track actual usage in sandboxed version
        })
    }

    /// Validate script for security issues
    fn validate_script(&self, script: &ScriptDefinition) -> Result<(), ExecutionError> {
        // Basic validation - check for dangerous patterns
        if let Some(content) = &script.content {
            // Check for dangerous imports or system calls
            let dangerous_patterns = [
                "import os",
                "import subprocess",
                "import sys",
                "exec(",
                "eval(",
                "system(",
                "popen(",
                "rm -rf",
                "sudo",
                "chmod 777",
                "chown",
            ];

            for pattern in &dangerous_patterns {
                if content.contains(pattern) {
                    return Err(ExecutionError::SecurityViolation(format!(
                        "Dangerous pattern detected: {}",
                        pattern
                    )));
                }
            }
        }

        // Validate file paths if accessing files
        if script.path.exists() {
            // Check path traversal - ensure script is under allowed roots if configured
            if !self.config.allowed_script_roots.is_empty() {
                let canonical_path = script.path.canonicalize().map_err(|e| {
                    ExecutionError::SecurityViolation(format!(
                        "Failed to canonicalize script path: {}",
                        e
                    ))
                })?;

                let is_allowed = self.config.allowed_script_roots.iter().any(|root| {
                    if let Ok(canonical_root) = root.canonicalize() {
                        canonical_path.starts_with(&canonical_root)
                    } else {
                        false
                    }
                });

                if !is_allowed {
                    return Err(ExecutionError::SecurityViolation(format!(
                        "Script path '{}' is outside allowed roots: {:?}",
                        script.path.display(),
                        self.config.allowed_script_roots
                    )));
                }
            }
        }

        Ok(())
    }

    /// Execute script in user's environment (fallback implementation)
    async fn execute_in_user_environment(
        &self,
        script: ScriptDefinition,
        context: ExecutionContext,
    ) -> Result<UserExecutionResult, ExecutionError> {
        let script_path = &script.path;

        if !script_path.exists() {
            return Err(ExecutionError::ScriptNotFound(
                script_path.to_string_lossy().to_string(),
            ));
        }

        // Determine command based on script language
        let command = match script.language {
            ScriptLanguage::Python => "python3",
            ScriptLanguage::NodeJS => "node",
            ScriptLanguage::Shell => "sh",
            ScriptLanguage::Rust => "cargo",
        };

        // Build the command
        let mut cmd = TokioCommand::new(command);

        // Add script path as argument
        cmd.arg(script_path);

        // Add parameters as environment variables
        for (key, value) in &script.parameters {
            cmd.env(format!("PARAM_{}", key), value);
        }

        // Add context environment variables
        for (key, value) in &context.environment_variables {
            cmd.env(key, value);
        }

        // Set working directory
        if let Some(working_dir) = &script.working_directory {
            cmd.current_dir(working_dir);
        }

        // Set timeout
        let timeout_duration = self.config.default_timeout;

        // Execute with timeout
        let result = timeout(timeout_duration, async {
            let output = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await?;

            Ok::<_, std::io::Error>(output)
        })
        .await;

        match result {
            Ok(Ok(output)) => Ok(UserExecutionResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code(),
            }),
            Ok(Err(e)) => Err(ExecutionError::Io(e)),
            Err(_) => Err(ExecutionError::Timeout(timeout_duration)),
        }
    }

    /// Execute a command directly
    pub async fn execute_command(
        &self,
        command: String,
        args: Vec<String>,
        context: ExecutionContext,
    ) -> Result<ExecutionResult, ExecutionError> {
        // Check if command is allowed
        if !self.config.allowed_commands.is_empty() {
            let command_name = command.split('/').next_back().unwrap_or(&command);
            if !self
                .config
                .allowed_commands
                .contains(&command_name.to_string())
            {
                return Err(ExecutionError::SecurityViolation(format!(
                    "Command '{}' is not in allowed commands list",
                    command_name
                )));
            }
        }

        let start_time = std::time::Instant::now();

        // Execute command in user's environment
        let result = self
            .execute_command_in_user_environment(command, args, context)
            .await?;

        let execution_time = start_time.elapsed();

        Ok(ExecutionResult {
            success: result.exit_code.unwrap_or(0) == 0,
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
            execution_time,
            resources_used: ResourceUsage::default(),
        })
    }

    /// Execute command in user's environment (fallback implementation)
    async fn execute_command_in_user_environment(
        &self,
        command: String,
        args: Vec<String>,
        context: ExecutionContext,
    ) -> Result<UserExecutionResult, ExecutionError> {
        let mut cmd = TokioCommand::new(command);
        cmd.args(args);

        // Set working directory
        if let Some(working_dir) = &context.working_directory {
            cmd.current_dir(working_dir);
        }

        // Set timeout
        let timeout_duration = self.config.default_timeout;

        let result = timeout(timeout_duration, async {
            let output = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await?;

            Ok::<_, std::io::Error>(output)
        })
        .await;

        match result {
            Ok(Ok(output)) => Ok(UserExecutionResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code(),
            }),
            Ok(Err(e)) => Err(ExecutionError::Io(e)),
            Err(_) => Err(ExecutionError::Timeout(timeout_duration)),
        }
    }

    /// Get current configuration
    pub fn config(&self) -> &ExecutionConfig {
        &self.config
    }
}

/// Result from user environment execution
#[derive(Debug)]
struct UserExecutionResult {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_validate_script_rejects_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let allowed_root = temp_dir.path().join("allowed");
        std::fs::create_dir_all(&allowed_root).unwrap();

        let config = ExecutionConfig {
            allowed_script_roots: vec![allowed_root.clone()],
            ..Default::default()
        };

        let sandbox = ExecutionSandbox::new(config).unwrap();

        // Create a script outside the allowed root
        let outside_path = temp_dir.path().join("outside").join("script.py");
        std::fs::create_dir_all(outside_path.parent().unwrap()).unwrap();
        std::fs::write(&outside_path, "print('test')").unwrap();

        let script = ScriptDefinition {
            path: outside_path,
            content: None,
            language: ScriptLanguage::Python,
            parameters: std::collections::HashMap::new(),
            working_directory: None,
        };

        let result = sandbox.validate_script(&script);
        assert!(matches!(result, Err(ExecutionError::SecurityViolation(_))));
    }

    #[test]
    fn test_validate_script_accepts_path_inside_allowed_root() {
        let temp_dir = TempDir::new().unwrap();
        let allowed_root = temp_dir.path().join("allowed");
        std::fs::create_dir_all(&allowed_root).unwrap();

        let config = ExecutionConfig {
            allowed_script_roots: vec![allowed_root.clone()],
            ..Default::default()
        };

        let sandbox = ExecutionSandbox::new(config).unwrap();

        // Create a script inside the allowed root
        let inside_path = allowed_root.join("script.py");
        std::fs::write(&inside_path, "print('test')").unwrap();

        let script = ScriptDefinition {
            path: inside_path,
            content: None,
            language: ScriptLanguage::Python,
            parameters: std::collections::HashMap::new(),
            working_directory: None,
        };

        let result = sandbox.validate_script(&script);
        assert!(result.is_ok());
    }
}
