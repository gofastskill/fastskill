//! Python bindings for the FastSkill service layer

use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3::Bound;
use std::collections::HashMap;

use crate::core::service::{FastSkillService, ServiceConfig};

/// Python wrapper for the main service
#[pyclass]
pub struct FastSkillServicePy {
    service: FastSkillService,
}

#[allow(non_local_definitions)]
#[pymethods]
impl FastSkillServicePy {
    #[new]
    fn new() -> PyResult<Self> {
        // For now, create with default config
        let rt = tokio::runtime::Runtime::new().unwrap();
        let service = rt
            .block_on(async { FastSkillService::new(ServiceConfig::default()).await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        Ok(Self { service })
    }

    /// Initialize the service
    fn initialize(&mut self) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async { self.service.initialize().await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;
        Ok(())
    }

    /// Shutdown the service
    fn shutdown(&mut self) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async { self.service.shutdown().await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;
        Ok(())
    }

    /// Get skill manager (returns a Python wrapper)
    #[getter]
    fn skill_manager(&self) -> PyResult<SkillManagerPy> {
        Ok(SkillManagerPy {
            skill_manager: self.service.skill_manager(),
        })
    }

    /// Get metadata service (returns a Python wrapper)
    #[getter]
    fn metadata_service(&self) -> PyResult<MetadataServicePy> {
        Ok(MetadataServicePy {
            metadata_service: self.service.metadata_service(),
        })
    }

    /// Get loading service (returns a Python wrapper)
    #[getter]
    fn loading_service(&self) -> PyResult<LoadingServicePy> {
        Ok(LoadingServicePy {
            loading_service: self.service.loading_service(),
        })
    }

    /// Get tool calling service (returns a Python wrapper)
    #[getter]
    fn tool_service(&self) -> PyResult<ToolCallingServicePy> {
        Ok(ToolCallingServicePy {
            tool_service: self.service.tool_service(),
        })
    }

    /// Get routing service (returns a Python wrapper)
    #[getter]
    fn routing_service(&self) -> PyResult<RoutingServicePy> {
        Ok(RoutingServicePy {
            routing_service: self.service.routing_service(),
        })
    }
}

/// Python wrapper for skill manager
#[pyclass]
pub struct SkillManagerPy {
    skill_manager: std::sync::Arc<dyn crate::core::skill_manager::SkillManagementService>,
}

#[allow(non_local_definitions)]
#[pymethods]
impl SkillManagerPy {
    /// Register a skill from a dictionary
    fn register_skill(&self, skill_dict: &Bound<'_, PyDict>) -> PyResult<String> {
        // Convert Python dict to HashMap
        let mut dict_data = std::collections::HashMap::new();

        // Extract all items from the Python dict
        for (key, value) in skill_dict.iter() {
            let key_str: String = key.extract()?;

            // Handle different value types
            let value_str = if let Ok(s) = value.extract::<String>() {
                s
            } else if let Ok(list) = value.extract::<Vec<String>>() {
                list.join(",")
            } else {
                // For other types, convert to string
                value.str()?.extract()?
            };

            dict_data.insert(key_str, value_str);
        }

        // Create skill from dictionary
        let skill = crate::core::skill_manager::SkillDefinition::from_dict(&dict_data)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e))?;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(async { self.skill_manager.register_skill(skill).await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        Ok(result)
    }

    /// List all skills
    fn list_skills(&self) -> PyResult<Vec<HashMap<String, String>>> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let skills = rt
            .block_on(async { self.skill_manager.list_skills(None).await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        let result: Vec<HashMap<String, String>> = skills
            .into_iter()
            .map(|skill| {
                let mut map = HashMap::new();
                map.insert("id".to_string(), skill.id);
                map.insert("name".to_string(), skill.name);
                map.insert("description".to_string(), skill.description);
                map.insert("version".to_string(), skill.version);
                map.insert("enabled".to_string(), skill.enabled.to_string());
                map.insert("tags".to_string(), skill.tags.join(","));
                map.insert("capabilities".to_string(), skill.capabilities.join(","));
                map
            })
            .collect();

        Ok(result)
    }
}

/// Python wrapper for metadata service
#[pyclass]
pub struct MetadataServicePy {
    metadata_service: std::sync::Arc<dyn crate::core::metadata::MetadataService>,
}

#[allow(non_local_definitions)]
#[pymethods]
impl MetadataServicePy {
    /// Discover skills
    fn discover_skills(&self, query: String) -> PyResult<Vec<HashMap<String, String>>> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let skills = rt
            .block_on(async { self.metadata_service.discover_skills(&query).await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        let result: Vec<HashMap<String, String>> = skills
            .into_iter()
            .map(|skill| {
                let mut map = HashMap::new();
                map.insert("id".to_string(), skill.id);
                map.insert("name".to_string(), skill.name);
                map.insert("description".to_string(), skill.description);
                map.insert("version".to_string(), skill.version);
                map.insert("enabled".to_string(), skill.enabled.to_string());
                map.insert("tags".to_string(), skill.tags.join(","));
                map.insert("capabilities".to_string(), skill.capabilities.join(","));
                map
            })
            .collect();

        Ok(result)
    }

    /// Find skills by capability
    fn find_skills_by_capability(
        &self,
        capability: String,
    ) -> PyResult<Vec<HashMap<String, String>>> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let skills = rt
            .block_on(async {
                self.metadata_service
                    .find_skills_by_capability(&capability)
                    .await
            })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        let result: Vec<HashMap<String, String>> = skills
            .into_iter()
            .map(|skill| {
                let mut map = HashMap::new();
                map.insert("id".to_string(), skill.id);
                map.insert("name".to_string(), skill.name);
                map.insert("description".to_string(), skill.description);
                map.insert("version".to_string(), skill.version);
                map.insert("enabled".to_string(), skill.enabled.to_string());
                map.insert("tags".to_string(), skill.tags.join(","));
                map.insert("capabilities".to_string(), skill.capabilities.join(","));
                map
            })
            .collect();

        Ok(result)
    }

    /// Find skills by tag
    fn find_skills_by_tag(&self, tag: String) -> PyResult<Vec<HashMap<String, String>>> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let skills = rt
            .block_on(async { self.metadata_service.find_skills_by_tag(&tag).await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        let result: Vec<HashMap<String, String>> = skills
            .into_iter()
            .map(|skill| {
                let mut map = HashMap::new();
                map.insert("id".to_string(), skill.id);
                map.insert("name".to_string(), skill.name);
                map.insert("description".to_string(), skill.description);
                map.insert("version".to_string(), skill.version);
                map.insert("enabled".to_string(), skill.enabled.to_string());
                map.insert("tags".to_string(), skill.tags.join(","));
                map.insert("capabilities".to_string(), skill.capabilities.join(","));
                map
            })
            .collect();

        Ok(result)
    }
}

/// Python wrapper for loading service
#[pyclass]
pub struct LoadingServicePy {
    loading_service: std::sync::Arc<dyn crate::core::loading::ProgressiveLoadingService>,
}

#[allow(non_local_definitions)]
#[pymethods]
impl LoadingServicePy {
    /// Load metadata for skills
    fn load_metadata(&self, skill_ids: Vec<String>) -> PyResult<Vec<HashMap<String, String>>> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let skills = rt
            .block_on(async { self.loading_service.load_metadata(&skill_ids).await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        let result: Vec<HashMap<String, String>> = skills
            .into_iter()
            .map(|skill| {
                let mut map = HashMap::new();
                map.insert("id".to_string(), skill.id);
                map.insert("name".to_string(), skill.name);
                map.insert("description".to_string(), skill.description);
                map
            })
            .collect();

        Ok(result)
    }
}

/// Python wrapper for tool calling service
#[pyclass]
pub struct ToolCallingServicePy {
    tool_service: std::sync::Arc<dyn crate::core::tool_calling::ToolCallingService>,
}

#[allow(non_local_definitions)]
#[pymethods]
impl ToolCallingServicePy {
    /// Get available tools
    fn get_available_tools(&self) -> PyResult<Vec<HashMap<String, String>>> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt
            .block_on(async { self.tool_service.get_available_tools().await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        let result: Vec<HashMap<String, String>> = tools
            .into_iter()
            .map(|tool| {
                let mut map = HashMap::new();
                map.insert("name".to_string(), tool.name);
                map.insert("description".to_string(), tool.description);
                map
            })
            .collect();

        Ok(result)
    }
}

/// Python wrapper for routing service
#[pyclass]
pub struct RoutingServicePy {
    routing_service: std::sync::Arc<dyn crate::core::routing::RoutingService>,
}

#[allow(non_local_definitions)]
#[pymethods]
impl RoutingServicePy {
    /// Find relevant skills for a query
    fn find_relevant_skills(&self, query: String) -> PyResult<Vec<HashMap<String, String>>> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let skills = rt
            .block_on(async {
                self.routing_service
                    .find_relevant_skills(&query, None)
                    .await
            })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        let result: Vec<HashMap<String, String>> = skills
            .into_iter()
            .map(|skill| {
                let mut map = HashMap::new();
                map.insert("skill_id".to_string(), skill.skill_id);
                map.insert(
                    "relevance_score".to_string(),
                    skill.relevance_score.to_string(),
                );
                map
            })
            .collect();

        Ok(result)
    }
}

/// Python wrapper for QueryContext
#[pyclass]
pub struct QueryContextPy {
    context: crate::core::routing::QueryContext,
}

#[allow(non_local_definitions)]
#[pymethods]
impl QueryContextPy {
    #[new]
    fn new() -> Self {
        Self {
            context: crate::core::routing::QueryContext {
                available_tokens: None,
                conversation_history: None,
                user_preferences: None,
            },
        }
    }

    #[setter]
    fn set_available_tokens(&mut self, tokens: Option<usize>) {
        self.context.available_tokens = tokens;
    }

    #[setter]
    fn set_conversation_history(&mut self, history: Option<Vec<String>>) {
        self.context.conversation_history = history;
    }

    #[setter]
    fn set_user_preferences(&mut self, prefs: Option<std::collections::HashMap<String, String>>) {
        self.context.user_preferences = prefs;
    }
}

/// Python wrapper for HTTP server control
#[pyclass]
pub struct ServerPy {
    handle: Option<std::thread::JoinHandle<()>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

#[allow(non_local_definitions)]
#[pymethods]
impl ServerPy {
    #[new]
    fn new() -> Self {
        Self {
            handle: None,
            shutdown_tx: None,
        }
    }

    /// Start the HTTP server
    #[pyo3(signature = (host="localhost", port=8080))]
    fn start(&mut self, host: &str, port: u16) -> PyResult<()> {
        if self.handle.is_some() {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Server is already running",
            ));
        }

        // For now, create a basic service for the server
        // In a real implementation, this would accept a service instance
        let rt = tokio::runtime::Runtime::new().unwrap();
        let service = rt
            .block_on(async {
                fastskill::FastSkillService::new(fastskill::ServiceConfig::default()).await
            })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

        let service = std::sync::Arc::new(service);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let server = crate::http::server::FastSkillServer::new(service, host, port);
                tokio::select! {
                    result = server.serve() => {
                        if let Err(e) = result {
                            eprintln!("Server error: {}", e);
                        }
                    }
                    _ = shutdown_rx => {
                        println!("Server shutdown requested");
                    }
                }
            });
        });

        self.handle = Some(handle);
        self.shutdown_tx = Some(shutdown_tx);

        Ok(())
    }

    /// Stop the HTTP server
    fn stop(&mut self) -> PyResult<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }

        Ok(())
    }

    /// Check if server is running
    fn is_running(&self) -> bool {
        self.handle.is_some()
    }
}

/// Free function to start server (alternative to class-based approach)
#[pyfunction]
#[pyo3(signature = (host="localhost", port=8080))]
fn start_server(host: &str, port: u16) -> PyResult<()> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let service = rt
        .block_on(async {
            fastskill::FastSkillService::new(fastskill::ServiceConfig::default()).await
        })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    let service = std::sync::Arc::new(service);

    rt.block_on(async {
        let server = crate::http::server::FastSkillServer::new(service, host, port);
        server.serve().await
    })
    .map_err(|e| {
        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Server error: {}", e))
    })?;

    Ok(())
}

/// Python module initialization
#[pymodule]
pub fn _fastskill(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<FastSkillServicePy>()?;
    m.add_class::<SkillManagerPy>()?;
    m.add_class::<MetadataServicePy>()?;
    m.add_class::<LoadingServicePy>()?;
    m.add_class::<ToolCallingServicePy>()?;
    m.add_class::<RoutingServicePy>()?;
    m.add_class::<QueryContextPy>()?;
    m.add_class::<ServerPy>()?;
    m.add_function(wrap_pyfunction!(start_server, m)?)?;
    Ok(())
}
