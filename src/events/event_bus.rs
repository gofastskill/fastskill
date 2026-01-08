//! Event bus for skill lifecycle events

use crate::core::service::ServiceError;
use crate::core::skill_manager::SkillDefinition;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

/// Type alias for event handlers map to reduce complexity
type EventHandlersMap = HashMap<String, Vec<Arc<dyn EventHandler>>>;

/// Skill event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillEvent {
    /// Skill registered
    SkillRegistered {
        skill_id: String,
        skill: Box<SkillDefinition>,
    },

    /// Skill updated
    SkillUpdated {
        skill_id: String,
        changes: SkillUpdate,
    },

    /// Skill unregistered
    SkillUnregistered { skill_id: String },

    /// Skill reloaded
    SkillReloaded {
        skill_id: String,
        success: bool,
        error_message: Option<String>,
    },

    /// Skill validation failed
    SkillValidationFailed {
        skill_id: String,
        errors: Vec<String>,
    },

    /// Hot reload enabled
    HotReloadEnabled { config: HotReloadConfig },

    /// Hot reload disabled
    HotReloadDisabled,

    /// Skill enabled
    SkillEnabled { skill_id: String },

    /// Skill disabled
    SkillDisabled { skill_id: String },

    /// Custom event
    Custom {
        event_type: String,
        data: serde_json::Value,
    },
}

/// Skill update information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub enabled: Option<bool>,
}

/// Hot reload configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotReloadConfig {
    pub watch_paths: Vec<String>,
    pub debounce_ms: u64,
    pub auto_reload: bool,
    pub max_concurrent_reloads: usize,
}

/// Event handler trait
#[async_trait]
pub trait EventHandler: Send + Sync {
    async fn handle_event(&self, event: SkillEvent) -> Result<(), ServiceError>;
}

/// Event bus for managing skill lifecycle events
pub struct EventBus {
    /// Broadcast sender for events
    sender: broadcast::Sender<SkillEvent>,

    /// Event handlers registry
    handlers: Arc<RwLock<EventHandlersMap>>,

    /// Event history for debugging
    event_history: Arc<RwLock<Vec<(SkillEvent, std::time::Instant)>>>,

    /// Maximum history size
    max_history_size: usize,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Create a new event bus
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1000); // Buffer for 1000 events

        Self {
            sender,
            handlers: Arc::new(RwLock::new(HashMap::new())),
            event_history: Arc::new(RwLock::new(Vec::new())),
            max_history_size: 100,
        }
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<SkillEvent> {
        self.sender.subscribe()
    }

    /// Register an event handler for a specific event type
    pub async fn register_handler<H: EventHandler + 'static>(
        &self,
        event_type: &str,
        handler: H,
    ) -> Result<(), ServiceError> {
        let mut handlers = self.handlers.write().await;

        let handler_arc = Arc::new(handler) as Arc<dyn EventHandler>;

        handlers
            .entry(event_type.to_string())
            .or_insert_with(Vec::new)
            .push(handler_arc);

        info!("Registered event handler for event type: {}", event_type);

        Ok(())
    }

    /// Unregister an event handler
    pub async fn unregister_handler(
        &self,
        event_type: &str,
        _handler_id: &str,
    ) -> Result<(), ServiceError> {
        let mut handlers = self.handlers.write().await;

        if let Some(handler_list) = handlers.get_mut(event_type) {
            // For now, just remove all handlers with this event type
            // In a more sophisticated implementation, we'd track handler IDs
            handler_list.clear();
            info!("Unregistered all handlers for event type: {}", event_type);
        }

        Ok(())
    }

    /// Publish an event
    pub async fn publish_event(&self, event: SkillEvent) -> Result<usize, ServiceError> {
        // Add to history
        {
            let mut history = self.event_history.write().await;

            history.push((event.clone(), std::time::Instant::now()));

            // Trim history if too large
            if history.len() > self.max_history_size {
                history.truncate(self.max_history_size);
            }
        }

        // Send to all subscribers
        let subscriber_count = self.sender.send(event.clone()).unwrap_or(0);

        // Notify registered handlers
        self.notify_handlers(&event).await;

        debug!(
            "Published event: subscriber_count={}, handlers_notified",
            subscriber_count
        );

        Ok(subscriber_count)
    }

    /// Notify registered handlers about an event
    async fn notify_handlers(&self, event: &SkillEvent) {
        let handlers = self.handlers.read().await;

        // Determine event type for handler lookup
        let event_type = match event {
            SkillEvent::SkillRegistered { .. } => "skill:registered",
            SkillEvent::SkillUpdated { .. } => "skill:updated",
            SkillEvent::SkillUnregistered { .. } => "skill:unregistered",
            SkillEvent::SkillReloaded { .. } => "skill:reloaded",
            SkillEvent::SkillValidationFailed { .. } => "skill:validation:failed",
            SkillEvent::HotReloadEnabled { .. } => "hot-reload:enabled",
            SkillEvent::HotReloadDisabled => "hot-reload:disabled",
            SkillEvent::SkillEnabled { .. } => "skill:enabled",
            SkillEvent::SkillDisabled { .. } => "skill:disabled",
            SkillEvent::Custom { event_type, .. } => event_type.as_str(),
        }
        .to_string();

        if let Some(event_handlers) = handlers.get(&event_type) {
            for handler in event_handlers {
                match handler.handle_event(event.clone()).await {
                    Ok(_) => {
                        debug!("Event handler processed event successfully");
                    }
                    Err(e) => {
                        warn!("Event handler failed to process event: {}", e);
                    }
                }
            }
        }
    }

    /// Get event history
    pub async fn get_event_history(&self) -> Vec<(SkillEvent, std::time::Instant)> {
        self.event_history.read().await.clone()
    }

    /// Clear event history
    pub async fn clear_event_history(&self) {
        self.event_history.write().await.clear();
    }

    /// Get registered event handlers
    pub async fn get_registered_handlers(&self) -> HashMap<String, usize> {
        let handlers = self.handlers.read().await;
        handlers.iter().map(|(k, v)| (k.clone(), v.len())).collect()
    }
}

/// Default event handler implementations
/// Logging event handler - logs all events
pub struct LoggingEventHandler;

impl Default for LoggingEventHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl LoggingEventHandler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EventHandler for LoggingEventHandler {
    async fn handle_event(&self, event: SkillEvent) -> Result<(), ServiceError> {
        match event {
            SkillEvent::SkillRegistered { skill_id, skill } => {
                info!("[OK] Skill registered: {} ({})", skill.name, skill_id);
            }
            SkillEvent::SkillUpdated { skill_id, .. } => {
                info!("Skill updated: {}", skill_id);
            }
            SkillEvent::SkillUnregistered { skill_id } => {
                info!("Skill unregistered: {}", skill_id);
            }
            SkillEvent::SkillReloaded {
                skill_id,
                success,
                error_message,
            } => {
                if success {
                    info!("Skill reloaded successfully: {}", skill_id);
                } else {
                    warn!(
                        "[ERROR] Skill reload failed: {} - {:?}",
                        skill_id, error_message
                    );
                }
            }
            SkillEvent::SkillValidationFailed { skill_id, errors } => {
                warn!(
                    "[ERROR] Skill validation failed: {} - {} errors",
                    skill_id,
                    errors.len()
                );
            }
            SkillEvent::HotReloadEnabled { config } => {
                info!(
                    "[INFO] Hot reload enabled for {} paths",
                    config.watch_paths.len()
                );
            }
            SkillEvent::HotReloadDisabled => {
                info!("Hot reload disabled");
            }
            SkillEvent::SkillEnabled { skill_id } => {
                info!("[OK] Skill enabled: {}", skill_id);
            }
            SkillEvent::SkillDisabled { skill_id } => {
                info!("Skill disabled: {}", skill_id);
            }
            SkillEvent::Custom { event_type, data } => {
                debug!("Custom event: {} - {:?}", event_type, data);
            }
        }

        Ok(())
    }
}

/// Metrics event handler - tracks event statistics
pub struct MetricsEventHandler {
    event_counts: Arc<RwLock<HashMap<String, usize>>>,
}

impl Default for MetricsEventHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsEventHandler {
    pub fn new() -> Self {
        Self {
            event_counts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get event statistics
    pub async fn get_event_counts(&self) -> HashMap<String, usize> {
        self.event_counts.read().await.clone()
    }
}

#[async_trait]
impl EventHandler for MetricsEventHandler {
    async fn handle_event(&self, event: SkillEvent) -> Result<(), ServiceError> {
        let event_type = match event {
            SkillEvent::SkillRegistered { .. } => "skill:registered".to_string(),
            SkillEvent::SkillUpdated { .. } => "skill:updated".to_string(),
            SkillEvent::SkillUnregistered { .. } => "skill:unregistered".to_string(),
            SkillEvent::SkillReloaded { .. } => "skill:reloaded".to_string(),
            SkillEvent::SkillValidationFailed { .. } => "skill:validation:failed".to_string(),
            SkillEvent::HotReloadEnabled { .. } => "hot-reload:enabled".to_string(),
            SkillEvent::HotReloadDisabled => "hot-reload:disabled".to_string(),
            SkillEvent::SkillEnabled { .. } => "skill:enabled".to_string(),
            SkillEvent::SkillDisabled { .. } => "skill:disabled".to_string(),
            SkillEvent::Custom { event_type, .. } => event_type.clone(),
        };

        let mut counts = self.event_counts.write().await;
        *counts.entry(event_type).or_insert(0) += 1;

        Ok(())
    }
}

/// Convenience methods for publishing common events
impl EventBus {
    /// Publish skill registered event
    pub async fn publish_skill_registered(
        &self,
        skill_id: String,
        skill: SkillDefinition,
    ) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::SkillRegistered {
            skill_id,
            skill: Box::new(skill),
        })
        .await
    }

    /// Publish skill updated event
    pub async fn publish_skill_updated(
        &self,
        skill_id: String,
        changes: SkillUpdate,
    ) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::SkillUpdated { skill_id, changes }).await
    }

    /// Publish skill unregistered event
    pub async fn publish_skill_unregistered(
        &self,
        skill_id: String,
    ) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::SkillUnregistered { skill_id }).await
    }

    /// Publish skill reloaded event
    pub async fn publish_skill_reloaded(
        &self,
        skill_id: String,
        success: bool,
        error_message: Option<String>,
    ) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::SkillReloaded {
            skill_id,
            success,
            error_message,
        })
        .await
    }

    /// Publish skill validation failed event
    pub async fn publish_skill_validation_failed(
        &self,
        skill_id: String,
        errors: Vec<String>,
    ) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::SkillValidationFailed { skill_id, errors }).await
    }

    /// Publish hot reload enabled event
    pub async fn publish_hot_reload_enabled(
        &self,
        config: HotReloadConfig,
    ) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::HotReloadEnabled { config }).await
    }

    /// Publish hot reload disabled event
    pub async fn publish_hot_reload_disabled(&self) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::HotReloadDisabled).await
    }

    /// Publish skill enabled event
    pub async fn publish_skill_enabled(&self, skill_id: String) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::SkillEnabled { skill_id }).await
    }

    /// Publish skill disabled event
    pub async fn publish_skill_disabled(&self, skill_id: String) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::SkillDisabled { skill_id }).await
    }
}
