//! Status and root endpoint handlers

use crate::core::service::FastSkillService;
use crate::http::errors::HttpResult;
use crate::http::models::{ApiResponse, StatusResponse};
use axum::{extract::State, response::Html};
use std::sync::Arc;
use std::time::SystemTime;

/// Shared state for HTTP handlers
#[derive(Clone)]
pub struct AppState {
    pub service: Arc<FastSkillService>,
    pub start_time: SystemTime,
    pub project_file_path: std::path::PathBuf,
    pub project_root: std::path::PathBuf,
    pub skills_directory: std::path::PathBuf,
    /// When false, mutating (write) endpoints are gated and return 403.
    pub enable_write: bool,
}

impl AppState {
    pub fn new(service: Arc<FastSkillService>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            service,
            start_time: SystemTime::now(),
            project_file_path: std::path::PathBuf::from("skill-project.toml"),
            project_root: std::path::PathBuf::from("."),
            skills_directory: std::path::PathBuf::from(".claude/skills"),
            enable_write: false,
        })
    }

    /// Enable or disable mutating (write) endpoints. Off by default (read-only).
    pub fn with_enable_write(mut self, enable_write: bool) -> Self {
        self.enable_write = enable_write;
        self
    }

    pub fn with_project_file_path(mut self, path: std::path::PathBuf) -> Self {
        self.project_file_path = path;
        self
    }

    pub fn with_project_config(
        mut self,
        project_root: std::path::PathBuf,
        project_file_path: std::path::PathBuf,
        skills_directory: std::path::PathBuf,
    ) -> Self {
        self.project_root = Self::canonicalize_path(project_root);
        self.project_file_path = Self::canonicalize_path(project_file_path);
        self.skills_directory = Self::canonicalize_path(skills_directory);
        self
    }

    /// Canonicalize a path if it exists, otherwise return as-is
    fn canonicalize_path(path: std::path::PathBuf) -> std::path::PathBuf {
        path.canonicalize().unwrap_or(path)
    }

    pub fn uptime_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(self.start_time)
            .unwrap_or_default()
            .as_secs()
    }
}

/// Minimal HTML-escape for text interpolated into the dashboard page.
/// Escapes the five HTML-significant characters to prevent stored/reflected XSS.
fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

/// GET / - Root endpoint with HTML dashboard
pub async fn root(State(state): State<AppState>) -> Html<String> {
    let skills: Vec<_> = (state.service.skill_manager().list_skills().await).unwrap_or_default();

    let skills_count = skills.len();
    let uptime = state.uptime_seconds();

    let skills_html = if skills.is_empty() {
        "<li>No skills found</li>".to_string()
    } else {
        skills
            .iter()
            .take(10) // Show first 10 skills
            .map(|skill| {
                let name = html_escape(&skill.name);
                let desc = html_escape(&skill.description);
                format!("<li><strong>{}</strong> - {}</li>", name, desc)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>FastSkill Service</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            margin: 0;
            padding: 20px;
            background: #f5f5f5;
        }}
        .container {{
            max-width: 1200px;
            margin: 0 auto;
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
            overflow: hidden;
        }}
        .header {{
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            padding: 30px;
            text-align: center;
        }}
        .content {{
            padding: 30px;
        }}
        .stats {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}
        .stat-card {{
            background: #f8f9fa;
            padding: 20px;
            border-radius: 6px;
            text-align: center;
            border-left: 4px solid #667eea;
        }}
        .stat-number {{
            font-size: 2em;
            font-weight: bold;
            color: #333;
        }}
        .stat-label {{
            color: #666;
            margin-top: 5px;
        }}
        .skills {{
            margin-top: 30px;
        }}
        .skill {{
            background: #f9f9f9;
            padding: 15px;
            margin: 8px 0;
            border-radius: 6px;
            border-left: 4px solid #28a745;
        }}
        .api-links {{
            margin-top: 40px;
            padding: 20px;
            background: #f8f9fa;
            border-radius: 6px;
        }}
        .api-links h3 {{
            margin-top: 0;
            color: #333;
        }}
        .api-links ul {{
            list-style: none;
            padding: 0;
        }}
        .api-links li {{
            margin: 8px 0;
        }}
        .api-links a {{
            color: #667eea;
            text-decoration: none;
            font-weight: 500;
        }}
        .api-links a:hover {{
            text-decoration: underline;
        }}
        .footer {{
            text-align: center;
            padding: 20px;
            background: #f8f9fa;
            color: #666;
            font-size: 0.9em;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>FastSkill Service</h1>
            <p>AI Agent Skills Management Platform</p>
        </div>

        <div class="content">
            <div class="stats">
                <div class="stat-card">
                    <div class="stat-number">{}</div>
                    <div class="stat-label">Skills Indexed</div>
                </div>
                <div class="stat-card">
                    <div class="stat-number">{}</div>
                    <div class="stat-label">Uptime (seconds)</div>
                </div>
                <div class="stat-card">
                    <div class="stat-number">v{}</div>
                    <div class="stat-label">Version</div>
                </div>
            </div>

            <div class="skills">
                <h2>Recent Skills</h2>
                <ul>
                    {}
                </ul>
                {}
            </div>

            <div class="api-links">
                <h3>API Endpoints</h3>
                <ul>
                    <li><a href="/api/v1/skills">GET /api/v1/skills</a> - List all skills</li>
                    <li><a href="/api/v1/status">GET /api/v1/status</a> - Service status</li>
                    <li><a href="/api/v1/search">POST /api/v1/search</a> - Search skills</li>
                    <li><a href="/api/v1/reindex">POST /api/v1/reindex</a> - Reindex skills</li>
                </ul>
            </div>
        </div>
    </div>
</body>
</html>"#,
        skills_count,
        uptime,
        env!("CARGO_PKG_VERSION"),
        skills_html,
        if skills_count > 10 {
            format!("<p>... and {} more skills</p>", skills_count - 10)
        } else {
            "".to_string()
        }
    );

    Html(html)
}

/// GET /api/status - Service status endpoint
pub async fn status(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<StatusResponse>>> {
    let skills = state.service.skill_manager().list_skills().await?;
    let skills_count = skills.len();

    let config = state.service.config();

    let response = StatusResponse {
        status: "running".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        skills_count,
        storage_path: config.skill_storage_path.to_string_lossy().to_string(),
        hot_reload_enabled: config.hot_reload.enabled,
        uptime_seconds: state.uptime_seconds(),
    };

    Ok(axum::Json(ApiResponse::success(response)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::html_escape;

    #[test]
    fn html_escape_neutralizes_script_injection() {
        // SEC-7: skill name/description are interpolated into the dashboard HTML.
        assert_eq!(
            html_escape("<script>alert('x')</script>"),
            "&lt;script&gt;alert(&#x27;x&#x27;)&lt;/script&gt;"
        );
    }

    #[test]
    fn html_escape_covers_all_five_chars() {
        assert_eq!(
            html_escape("a & b \"c\" 'd' <e>"),
            "a &amp; b &quot;c&quot; &#x27;d&#x27; &lt;e&gt;"
        );
    }

    #[test]
    fn html_escape_passes_plain_text() {
        assert_eq!(html_escape("plain text 123"), "plain text 123");
    }
}
