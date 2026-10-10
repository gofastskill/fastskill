//! Managed settings, read only from trusted configuration (ADR-0016 decision 7).
//!
//! Two files hold them, both named `managed.toml`:
//!
//! - **the system file**, which device management deploys at a fixed platform location
//!   ([`system_file_path`]) and FastSkill honors only when the administrator owns it and its
//!   folder and nobody else can write to them;
//! - **the user file**, in FastSkill's configuration directory.
//!
//! A setting in the system file overrides the same setting in the user file. A project's
//! `skill-project.toml`, environment variables and flags never supply them.
//!
//! ```toml
//! source = "https://skills.example.com/state"   # or an absolute path to a state file
//! required = false
//! credential_command = ["example-login", "token"]
//! targets = ["claude", "cursor"]                # replaces agent detection
//!
//! [[keys]]
//! id = "2026-09"
//! public_key = "<base64 of the 32-byte Ed25519 public key>"
//! ```

use super::envelope::decode_required;
use super::ManagedError;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The name of both settings files.
pub const SYSTEM_FILE_NAME: &str = "managed.toml";
/// The name of the user's settings file, in FastSkill's configuration directory.
pub const USER_FILE_NAME: &str = "managed.toml";

/// A key that may sign managed states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedKey {
    pub id: String,
    pub public_key: [u8; 32],
}

/// Where managed states come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedSource {
    Https(url::Url),
    File(PathBuf),
}

impl ManagedSource {
    /// An `https://` URL, or an absolute local path. Any other scheme is refused.
    pub fn parse(value: &str) -> Result<Self, ManagedError> {
        let value = value.trim();
        if value.contains("://") {
            return match url::Url::parse(value) {
                Ok(url) if url.scheme() == "https" && url.host_str().is_some() => {
                    Ok(Self::Https(url))
                }
                _ => Err(ManagedError::Config(format!(
                    "source {value:?} must be an https:// URL or an absolute path"
                ))),
            };
        }
        let path = PathBuf::from(value);
        if value.is_empty() || !path.is_absolute() {
            return Err(ManagedError::Config(format!(
                "source {value:?} must be an https:// URL or an absolute path"
            )));
        }
        Ok(Self::File(path))
    }

    /// The source as configured.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Https(url) => url.as_str(),
            Self::File(path) => path.to_str().unwrap_or_default(),
        }
    }

    /// Whether a state's `source` names this source.
    pub fn matches(&self, value: &str) -> bool {
        match self {
            Self::Https(url) => url::Url::parse(value).is_ok_and(|other| &other == url),
            Self::File(path) => Path::new(value) == path,
        }
    }

    /// Whether `link` is an `https://` URL on this source's origin (scheme, host and port). A
    /// file source has no origin.
    pub fn same_origin(&self, link: &str) -> bool {
        let Self::Https(url) = self else {
            return false;
        };
        url::Url::parse(link)
            .is_ok_and(|other| other.scheme() == "https" && other.origin() == url.origin())
    }
}

/// The managed settings in force, after the system file overrides the user file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedSettings {
    pub source: Option<ManagedSource>,
    pub keys: Vec<PinnedKey>,
    /// The program and its arguments, run without a shell.
    pub credential_command: Option<Vec<String>>,
    /// Refuse to read or change skills without a valid, unexpired state.
    pub required: bool,
    /// Agents to cover instead of detecting them.
    pub targets: Option<Vec<String>>,
    /// Whether the system file set `required = true`.
    pub required_by_system: bool,
    /// Whether the system file names the source, so the first apply may enroll.
    pub source_from_system: bool,
    /// The files the settings came from.
    pub files: Vec<PathBuf>,
}

/// One settings file, as written.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsFile {
    pub source: Option<String>,
    pub keys: Option<Vec<KeyEntry>>,
    pub credential_command: Option<Vec<String>>,
    pub required: Option<bool>,
    pub targets: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEntry {
    pub id: String,
    pub public_key: String,
}

impl ManagedSettings {
    /// Whether a managed source is configured.
    pub fn is_configured(&self) -> bool {
        self.source.is_some()
    }

    /// Read the platform system file and the user file.
    pub fn load() -> Result<Self, ManagedError> {
        Self::load_from(
            system_file_path().as_deref(),
            user_file_path().as_deref(),
            check_system_file,
        )
    }

    /// Read the settings from `system` and `user`, each optional and absent when missing, with
    /// `trust` deciding whether the system file may be honored.
    pub fn load_from(
        system: Option<&Path>,
        user: Option<&Path>,
        trust: impl Fn(&Path) -> Result<(), String>,
    ) -> Result<Self, ManagedError> {
        let mut files = Vec::new();
        let system_settings = match system.filter(|path| path.symlink_metadata().is_ok()) {
            Some(path) => {
                trust(path).map_err(ManagedError::Config)?;
                files.push(path.to_path_buf());
                Some(read_file(path)?)
            }
            None => None,
        };
        let user_settings = match user.filter(|path| path.exists()) {
            Some(path) => {
                files.push(path.to_path_buf());
                Some(read_file(path)?)
            }
            None => None,
        };
        let mut settings = merge(system_settings, user_settings.unwrap_or_default())?;
        settings.files = files;
        Ok(settings)
    }
}

fn read_file(path: &Path) -> Result<SettingsFile, ManagedError> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| ManagedError::Config(format!("can't read {}: {error}", path.display())))?;
    toml::from_str(&content)
        .map_err(|error| ManagedError::Config(format!("{}: {error}", path.display())))
}

/// The system file's settings override the user's, setting by setting.
fn merge(
    system: Option<SettingsFile>,
    user: SettingsFile,
) -> Result<ManagedSettings, ManagedError> {
    let system = system.unwrap_or_default();
    let required_by_system = system.required == Some(true);
    let source_from_system = system.source.is_some();
    let source = system.source.or(user.source);
    let keys = system.keys.or(user.keys).unwrap_or_default();
    let credential_command = system.credential_command.or(user.credential_command);
    let settings = ManagedSettings {
        source: source.as_deref().map(ManagedSource::parse).transpose()?,
        keys: parse_keys(&keys)?,
        credential_command,
        required: system.required.or(user.required).unwrap_or(false),
        targets: system.targets.or(user.targets),
        required_by_system,
        source_from_system,
        files: Vec::new(),
    };
    if let Some(command) = &settings.credential_command {
        if command
            .first()
            .is_none_or(|program| program.trim().is_empty())
        {
            return Err(ManagedError::Config(
                "credential_command names no program; set credential_command = [\"program\", \
                 \"arg\", ...]"
                    .to_string(),
            ));
        }
    }
    if settings.source.is_some() && settings.keys.is_empty() {
        return Err(ManagedError::Config(
            "a source is configured but no keys are pinned; add a [[keys]] entry".to_string(),
        ));
    }
    if settings.required && settings.source.is_none() {
        return Err(ManagedError::Config(
            "required = true needs a source".to_string(),
        ));
    }
    Ok(settings)
}

fn parse_keys(entries: &[KeyEntry]) -> Result<Vec<PinnedKey>, ManagedError> {
    let mut seen = HashSet::new();
    entries
        .iter()
        .map(|entry| {
            let id = entry.id.trim();
            if id.is_empty() {
                return Err(ManagedError::Config(
                    "a pinned key has an empty id".to_string(),
                ));
            }
            if !seen.insert(id.to_string()) {
                return Err(ManagedError::Config(format!(
                    "the key id {id:?} is pinned twice"
                )));
            }
            let bytes = decode_required(&entry.public_key, &format!("the public key of {id:?}"))
                .map_err(ManagedError::Config)?;
            let public_key: [u8; 32] = bytes.try_into().map_err(|_| {
                ManagedError::Config(format!(
                    "the public key of {id:?} isn't a 32-byte Ed25519 key"
                ))
            })?;
            Ok(PinnedKey {
                id: id.to_string(),
                public_key,
            })
        })
        .collect()
}

/// The user's managed settings file, in FastSkill's configuration directory.
pub fn user_file_path() -> Option<PathBuf> {
    crate::core::repository::user_config::user_config_dir().map(|dir| dir.join(USER_FILE_NAME))
}

/// Where device management deploys the system file on this platform.
pub fn system_file_path() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        Some(PathBuf::from("/etc/fastskill").join(SYSTEM_FILE_NAME))
    } else if cfg!(target_os = "macos") {
        Some(PathBuf::from("/Library/Application Support/FastSkill").join(SYSTEM_FILE_NAME))
    } else if cfg!(windows) {
        std::env::var_os("ProgramData")
            .map(|dir| PathBuf::from(dir).join("FastSkill").join(SYSTEM_FILE_NAME))
    } else {
        None
    }
}

/// Honor the system file only when the administrator owns it and its folder, it is a regular
/// file, and nobody else can write to either.
#[cfg(unix)]
pub fn check_system_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let problem = |what: String| {
        Err(format!(
            "refusing the managed settings file {}: {what}. Device management must deploy it \
             owned by root and writable by nobody else",
            path.display()
        ))
    };
    let file = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !file.file_type().is_file() {
        return problem("it isn't a regular file".to_string());
    }
    let folder = path.parent().unwrap_or(Path::new("/"));
    let dir = std::fs::symlink_metadata(folder).map_err(|error| error.to_string())?;
    for (what, metadata) in [("it", &file), ("its folder", &dir)] {
        if metadata.uid() != 0 {
            return problem(format!("{what} isn't owned by root"));
        }
        if metadata.mode() & 0o022 != 0 {
            return problem(format!("{what} is writable by a group or other users"));
        }
    }
    Ok(())
}

/// Windows ownership and ACL checks aren't implemented yet, so a system file there is refused
/// rather than honored unchecked.
#[cfg(not(unix))]
pub fn check_system_file(path: &Path) -> Result<(), String> {
    Err(format!(
        "refusing the managed settings file {}: checking that only administrators can change \
         it isn't supported on this platform yet",
        path.display()
    ))
}

/// The error for a project file that carries managed settings.
pub fn project_settings_refused(file: &str) -> String {
    format!(
        "{file} contains managed settings. A project file can't set them, because anyone who \
         can change it could redirect where this machine's skills come from or choose a \
         command to run. Set them in the system managed.toml or in your user managed.toml."
    )
}
