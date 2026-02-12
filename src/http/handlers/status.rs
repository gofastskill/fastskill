//! Status and root endpoint handlers

use crate::core::service::FastSkillService;
use crate::http::auth::jwt::JwtService;
use crate::http::errors::HttpResult;
use crate::http::models::{ApiResponse, StatusResponse};
use axum::{extract::State, response::Html};
use std::sync::Arc;
use std::time::SystemTime;

/// Shared state for HTTP handlers
#[derive(Clone)]
pub struct AppState {
    pub service: Arc<FastSkillService>,
    pub jwt_service: Arc<JwtService>,
    pub start_time: SystemTime,
    pub project_file_path: std::path::PathBuf,
    pub project_root: std::path::PathBuf,
    pub skills_directory: std::path::PathBuf,
}

impl AppState {
    pub fn new(service: Arc<FastSkillService>) -> Result<Self, Box<dyn std::error::Error>> {
        let jwt_service = Arc::new(JwtService::from_env()?);
        Ok(Self {
            service,
            jwt_service,
            start_time: SystemTime::now(),
            project_file_path: std::path::PathBuf::from("skill-project.toml"),
            project_root: std::path::PathBuf::from("."),
            skills_directory: std::path::PathBuf::from(".claude/skills"),
        })
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

/// GET / - Root endpoint with HTML dashboard
pub async fn root(State(state): State<AppState>) -> Html<String> {
    let skills: Vec<_> =
        (state.service.skill_manager().list_skills(None).await).unwrap_or_default();

    let skills_count = skills.len();
    let uptime = state.uptime_seconds();

    let skills_html = if skills.is_empty() {
        "<li>No skills found</li>".to_string()
    } else {
        skills
            .iter()
            .take(10) // Show first 10 skills
            .map(|skill| {
                let name = &skill.name;
                let desc = &skill.description;
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
                    <li><a href="/api/skills">GET /api/skills</a> - List all skills</li>
                    <li><a href="/api/status">GET /api/status</a> - Service status</li>
                    <li><a href="/api/search">POST /api/search</a> - Search skills</li>
                    <li><a href="/api/reindex">POST /api/reindex</a> - Reindex skills</li>
                    <li><a href="/auth/token">POST /auth/token</a> - Get auth token (dev)</li>
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
    let skills = state.service.skill_manager().list_skills(None).await?;
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
