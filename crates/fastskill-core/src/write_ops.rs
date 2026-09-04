//! The single definition of FastSkill's mutating operations (ADR-0003 / WRITE-GATE).
//!
//! Every surface that can mutate state is gated off this one table:
//!
//! * `fastskill serve` builds its write-gated Axum router from [`WriteOperation::http_routes`]
//!   (see `crate::http::server`), so a route listed here is automatically behind
//!   the `--enable-write` middleware.
//! * `fastskill mcp serve` derives, from [`WriteOperation::command_path`], the MCP
//!   tool names it hides from `tools/list` and refuses in `tools/call` unless
//!   `--enable-write` was passed.
//!
//! Adding a mutating command or route means adding it *here*. There is
//! deliberately no second list to keep in sync: the MCP surface drifted into
//! exposing every mutator precisely because its gate was maintained separately
//! from the HTTP one.
//!
//! ## What counts as a write
//!
//! An operation belongs here when it changes FastSkill-managed state: the
//! project manifest or lock, the installed skills tree, the search index, the
//! configured repository list, the local cache, or an optimization run.
//! Commands that only render a report to a caller-named path (`eval *`,
//! `analyze *`, `optimize export`) are not gated.

use crate::http::handlers::{manifest, registry, reindex, skills, AppState};
use axum::routing::{delete, post, put, MethodRouter};

/// One mutating HTTP route, together with the method-router that serves it.
///
/// `method` and `path` are documentation and diagnostics; `router` is what the
/// server actually mounts, which is why the two cannot drift apart.
pub struct WriteHttpRoute {
    /// HTTP method, for diagnostics and docs (`"POST"`, `"DELETE"`, ...).
    pub method: &'static str,
    /// Route path relative to the `/api/v1` mount point.
    pub path: &'static str,
    router: fn() -> MethodRouter<AppState>,
}

impl WriteHttpRoute {
    /// The Axum method-router serving this route.
    pub fn method_router(&self) -> MethodRouter<AppState> {
        (self.router)()
    }
}

/// One operation that mutates FastSkill-managed state.
pub struct WriteOperation {
    /// Stable identifier, used in diagnostics.
    pub id: &'static str,
    /// The CLI command path that performs it (`["repos", "add"]`), when there is
    /// one. `None` for operations reachable only over HTTP.
    pub command_path: Option<&'static [&'static str]>,
    /// The HTTP routes that perform it. Empty for operations with no HTTP surface.
    pub http_routes: &'static [WriteHttpRoute],
}

fn route_delete_skill() -> MethodRouter<AppState> {
    delete(skills::delete_skill)
}
fn route_install_skill() -> MethodRouter<AppState> {
    post(skills::install_skill)
}
fn route_update_skills() -> MethodRouter<AppState> {
    post(skills::update_skills)
}
fn route_upgrade_skills() -> MethodRouter<AppState> {
    post(skills::update_skills)
}
fn route_reindex_all() -> MethodRouter<AppState> {
    post(reindex::reindex_all)
}
fn route_reindex_skill() -> MethodRouter<AppState> {
    post(reindex::reindex_skill)
}
fn route_refresh_sources() -> MethodRouter<AppState> {
    post(registry::refresh_sources)
}
fn route_add_skill_to_manifest() -> MethodRouter<AppState> {
    post(manifest::add_skill_to_manifest)
}
fn route_update_skill_in_manifest() -> MethodRouter<AppState> {
    put(manifest::update_skill_in_manifest)
}
fn route_remove_skill_from_manifest() -> MethodRouter<AppState> {
    delete(manifest::remove_skill_from_manifest)
}

/// Every mutating operation FastSkill exposes, on any surface.
pub static WRITE_OPERATIONS: &[WriteOperation] = &[
    WriteOperation {
        id: "init",
        command_path: Some(&["init"]),
        http_routes: &[],
    },
    WriteOperation {
        id: "install",
        command_path: Some(&["install"]),
        http_routes: &[WriteHttpRoute {
            method: "POST",
            path: "/skills/install",
            router: route_install_skill,
        }],
    },
    WriteOperation {
        id: "update",
        command_path: Some(&["update"]),
        http_routes: &[
            WriteHttpRoute {
                method: "POST",
                path: "/skills/update",
                router: route_update_skills,
            },
            // Back-compat alias for `/skills/update` (spec 003 §2) — same handler.
            WriteHttpRoute {
                method: "POST",
                path: "/skills/upgrade",
                router: route_upgrade_skills,
            },
        ],
    },
    WriteOperation {
        id: "add",
        command_path: Some(&["add"]),
        http_routes: &[WriteHttpRoute {
            method: "POST",
            path: "/manifest/skills",
            router: route_add_skill_to_manifest,
        }],
    },
    WriteOperation {
        id: "remove",
        command_path: Some(&["remove"]),
        http_routes: &[WriteHttpRoute {
            method: "DELETE",
            path: "/skills/{id}",
            router: route_delete_skill,
        }],
    },
    WriteOperation {
        id: "reindex",
        command_path: Some(&["reindex"]),
        http_routes: &[
            WriteHttpRoute {
                method: "POST",
                path: "/reindex",
                router: route_reindex_all,
            },
            WriteHttpRoute {
                method: "POST",
                path: "/reindex/{id}",
                router: route_reindex_skill,
            },
        ],
    },
    WriteOperation {
        id: "repos-add",
        command_path: Some(&["repos", "add"]),
        http_routes: &[],
    },
    WriteOperation {
        id: "repos-remove",
        command_path: Some(&["repos", "remove"]),
        http_routes: &[],
    },
    WriteOperation {
        id: "repos-update",
        command_path: Some(&["repos", "update"]),
        http_routes: &[],
    },
    WriteOperation {
        id: "repos-refresh",
        command_path: Some(&["repos", "refresh"]),
        http_routes: &[WriteHttpRoute {
            method: "POST",
            path: "/registry/refresh",
            router: route_refresh_sources,
        }],
    },
    WriteOperation {
        id: "cache-clean",
        command_path: Some(&["cache", "clean"]),
        http_routes: &[],
    },
    WriteOperation {
        id: "marketplace-create",
        command_path: Some(&["marketplace", "create"]),
        http_routes: &[],
    },
    WriteOperation {
        id: "optimize-run",
        command_path: Some(&["optimize", "run"]),
        http_routes: &[],
    },
    WriteOperation {
        id: "optimize-resume",
        command_path: Some(&["optimize", "resume"]),
        http_routes: &[],
    },
    // Manifest editing has no single CLI equivalent (`add` covers creation only).
    WriteOperation {
        id: "manifest-update",
        command_path: None,
        http_routes: &[WriteHttpRoute {
            method: "PUT",
            path: "/manifest/skills/{id}",
            router: route_update_skill_in_manifest,
        }],
    },
    WriteOperation {
        id: "manifest-remove",
        command_path: None,
        http_routes: &[WriteHttpRoute {
            method: "DELETE",
            path: "/manifest/skills/{id}",
            router: route_remove_skill_from_manifest,
        }],
    },
];

/// The command path of every mutating operation that has one.
pub fn write_command_paths() -> impl Iterator<Item = &'static [&'static str]> {
    WRITE_OPERATIONS.iter().filter_map(|op| op.command_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_has_a_surface() {
        for op in WRITE_OPERATIONS {
            assert!(
                op.command_path.is_some() || !op.http_routes.is_empty(),
                "write operation {} reaches no surface",
                op.id
            );
        }
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = WRITE_OPERATIONS.iter().map(|op| op.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate write-operation id");
    }

    #[test]
    fn command_paths_are_non_empty() {
        for path in write_command_paths() {
            assert!(!path.is_empty(), "empty command path in write operations");
        }
    }
}
