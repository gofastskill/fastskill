//! Proxy server configuration

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Configuration for the FastSkill proxy server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Port for the proxy server (default: 8081)
    pub port: u16,
    /// Target Anthropic API URL (default: https://api.anthropic.com)
    pub target_url: String,
    /// Enable dynamic skill discovery mode
    pub enable_dynamic_injection: bool,
    /// Skills directory path (default: .claude/skills/)
    pub skills_dir: PathBuf,
    /// Maximum number of skills to inject in dynamic mode
    pub max_dynamic_skills: usize,
    /// Request timeout in seconds
    pub request_timeout_secs: u64,
    /// Enable request logging
    pub enable_logging: bool,
    /// Hardcoded skills to always inject
    pub hardcoded_skills: Vec<String>,
    /// Minimum relevance score for dynamic skills
    pub dynamic_min_relevance: f64,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            port: 8081,
            target_url: "https://api.anthropic.com".to_string(),
            enable_dynamic_injection: false,
            skills_dir: PathBuf::from(".claude/skills"),
            max_dynamic_skills: 3,
            request_timeout_secs: 300, // 5 minutes
            enable_logging: true,
            hardcoded_skills: Vec::new(),
            dynamic_min_relevance: 0.7,
        }
    }
}

impl ProxyConfig {
    /// Create a new proxy config with custom values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the proxy port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the target Anthropic API URL
    pub fn with_target_url(mut self, url: String) -> Self {
        self.target_url = url;
        self
    }

    /// Enable or disable dynamic skill injection
    pub fn with_dynamic_injection(mut self, enabled: bool) -> Self {
        self.enable_dynamic_injection = enabled;
        self
    }

    /// Set the skills directory
    pub fn with_skills_dir(mut self, dir: PathBuf) -> Self {
        self.skills_dir = dir;
        self
    }

    /// Set maximum number of skills for dynamic injection
    pub fn with_max_dynamic_skills(mut self, max: usize) -> Self {
        self.max_dynamic_skills = max;
        self
    }

    /// Load proxy configuration from skills.toml manifest
    pub fn from_manifest(manifest: &crate::core::manifest::SkillsManifest) -> Option<Self> {
        manifest.proxy.as_ref().map(|proxy_manifest| {
            let mut config = Self {
                port: proxy_manifest.port,
                target_url: proxy_manifest.target_url.clone(),
                enable_dynamic_injection: proxy_manifest.enable_dynamic,
                ..Default::default()
            };

            // Set hardcoded skills
            if let Some(hardcoded) = &proxy_manifest.hardcoded_skills {
                config.hardcoded_skills = hardcoded.skills.clone();
            }

            // Set dynamic skills config
            if let Some(dynamic) = &proxy_manifest.dynamic_skills {
                config.max_dynamic_skills = dynamic.max_skills;
                config.dynamic_min_relevance = dynamic.min_relevance;
            }

            config
        })
    }

    /// Load proxy config with fallback priority:
    /// 1. skills.toml manifest
    /// 2. Command line arguments
    /// 3. Default values
    pub fn load_with_fallback(
        manifest_path: Option<&Path>,
        cli_args: &ProxyCommandArgs,
    ) -> Result<Self, ConfigError> {
        // Try to load from skills.toml first
        if let Some(path) = manifest_path {
            if path.exists() {
                if let Ok(manifest) = crate::core::manifest::SkillsManifest::load_from_file(path) {
                    if let Some(mut config) = Self::from_manifest(&manifest) {
                        // Set skills_dir from resolved path
                        config.skills_dir = path
                            .parent()
                            .unwrap_or(Path::new("."))
                            .join(".claude/skills");

                        // Override with CLI args if provided
                        return Ok(config.apply_cli_overrides(cli_args));
                    }
                }
            }
        }

        // Fall back to CLI args or defaults
        Self::from_args(cli_args)
    }

    /// Apply CLI argument overrides to config
    fn apply_cli_overrides(mut self, args: &ProxyCommandArgs) -> Self {
        if let Some(port) = args.port {
            self.port = port;
        }
        if !args.target_url.is_empty() {
            self.target_url = args.target_url.clone();
        }
        if let Some(enable_dynamic) = args.enable_dynamic {
            self.enable_dynamic_injection = enable_dynamic;
        }
        if let Some(max_skills) = args.max_dynamic_skills {
            self.max_dynamic_skills = max_skills;
        }
        if let Some(timeout) = args.request_timeout_secs {
            self.request_timeout_secs = timeout;
        }
        if let Some(enable_logging) = args.enable_logging {
            self.enable_logging = enable_logging;
        }
        self
    }

    /// Create config from CLI arguments
    fn from_args(args: &ProxyCommandArgs) -> Result<Self, ConfigError> {
        let mut config = Self::default();

        if let Some(port) = args.port {
            config.port = port;
        }
        if !args.target_url.is_empty() {
            config.target_url = args.target_url.clone();
        }
        if let Some(enable_dynamic) = args.enable_dynamic {
            config.enable_dynamic_injection = enable_dynamic;
        }
        if let Some(max_skills) = args.max_dynamic_skills {
            config.max_dynamic_skills = max_skills;
        }
        if let Some(timeout) = args.request_timeout_secs {
            config.request_timeout_secs = timeout;
        }
        if let Some(enable_logging) = args.enable_logging {
            config.enable_logging = enable_logging;
        }

        Ok(config)
    }
}

/// Arguments structure for proxy command (for config loading)
#[derive(Debug, Clone)]
pub struct ProxyCommandArgs {
    pub port: Option<u16>,
    pub target_url: String,
    pub enable_dynamic: Option<bool>,
    pub max_dynamic_skills: Option<usize>,
    pub request_timeout_secs: Option<u64>,
    pub enable_logging: Option<bool>,
}

/// Configuration error type
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Configuration error: {0}")]
    Invalid(String),
}
