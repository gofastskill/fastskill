//! Event bus for skill lifecycle events

use crate::core::service::ServiceError;
use crate::core::skill_manager::SkillDefinition;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
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

    /// Event history for debugging (ring buffer: newest kept, oldest evicted)
    event_history: Arc<RwLock<VecDeque<(SkillEvent, std::time::Instant)>>>,

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
            event_history: Arc::new(RwLock::new(VecDeque::new())),
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
        // Add to history (ring buffer: keep the NEWEST `max_history_size`
        // events, evicting the oldest from the front when over capacity).
        {
            let mut history = self.event_history.write().await;

            history.push_back((event.clone(), std::time::Instant::now()));

            while history.len() > self.max_history_size {
                history.pop_front();
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

    /// Notify registered handlers about an event.
    ///
    /// The handler `Arc`s are cloned under a short read-lock, which is then
    /// dropped *before* any handler is awaited. Awaiting handlers while holding
    /// the lock would deadlock the bus if a handler (re)registers or unregisters
    /// a handler (those take a write-lock, and tokio's `RwLock` is
    /// write-preferring / non-reentrant), and would serialize all handler
    /// execution under a lock held across arbitrary user async code.
    async fn notify_handlers(&self, event: &SkillEvent) {
        // Determine event type for handler lookup
        let event_type = match event {
            SkillEvent::SkillRegistered { .. } => "skill:registered",
            SkillEvent::SkillUpdated { .. } => "skill:updated",
            SkillEvent::SkillUnregistered { .. } => "skill:unregistered",
            SkillEvent::SkillReloaded { .. } => "skill:reloaded",
            SkillEvent::SkillValidationFailed { .. } => "skill:validation:failed",
            SkillEvent::HotReloadEnabled { .. } => "hot-reload:enabled",
            SkillEvent::HotReloadDisabled => "hot-reload:disabled",
            SkillEvent::Custom { event_type, .. } => event_type.as_str(),
        }
        .to_string();

        // Clone the handler Arcs under a short read-lock, then release it.
        let handlers_for_event: Vec<Arc<dyn EventHandler>> = {
            let handlers = self.handlers.read().await;
            match handlers.get(&event_type) {
                Some(list) => list.clone(),
                None => Vec::new(),
            }
        };

        // Await handlers OUTSIDE the lock so a handler that (un)registers can't
        // deadlock, and handlers aren't serialized under the lock.
        for handler in handlers_for_event {
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

    /// Get event history (oldest first, newest last)
    pub async fn get_event_history(&self) -> Vec<(SkillEvent, std::time::Instant)> {
        self.event_history.read().await.iter().cloned().collect()
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
        self.publish_event(SkillEvent::SkillUpdated { skill_id, changes })
            .await
    }

    /// Publish skill unregistered event
    pub async fn publish_skill_unregistered(
        &self,
        skill_id: String,
    ) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::SkillUnregistered { skill_id })
            .await
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
        self.publish_event(SkillEvent::SkillValidationFailed { skill_id, errors })
            .await
    }

    /// Publish hot reload enabled event
    pub async fn publish_hot_reload_enabled(
        &self,
        config: HotReloadConfig,
    ) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::HotReloadEnabled { config })
            .await
    }

    /// Publish hot reload disabled event
    pub async fn publish_hot_reload_disabled(&self) -> Result<usize, ServiceError> {
        self.publish_event(SkillEvent::HotReloadDisabled).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn custom_index(event: &SkillEvent) -> u64 {
        match event {
            SkillEvent::Custom { data, .. } => data["i"].as_u64().expect("index should be present"),
            other => panic!("expected Custom event, got {:?}", other),
        }
    }

    /// BUG-13: after overflow, history must keep the NEWEST `max_history_size`
    /// events (drop the oldest), not freeze on the first N ever published.
    #[tokio::test]
    async fn test_event_history_keeps_newest_after_overflow() {
        let bus = EventBus::new();

        // Publish 150 events into a 100-entry ring buffer.
        for i in 0..150u64 {
            bus.publish_event(SkillEvent::Custom {
                event_type: "test".to_string(),
                data: serde_json::json!({ "i": i }),
            })
            .await
            .unwrap();
        }

        let history = bus.get_event_history().await;
        assert_eq!(history.len(), 100, "history is capped at max_history_size");

        // Oldest retained is i=50, newest is i=149.
        assert_eq!(custom_index(&history.first().unwrap().0), 50);
        assert_eq!(custom_index(&history.last().unwrap().0), 149);
    }

    #[tokio::test]
    async fn test_event_history_under_capacity_keeps_all() {
        let bus = EventBus::new();
        for i in 0..5u64 {
            bus.publish_event(SkillEvent::Custom {
                event_type: "test".to_string(),
                data: serde_json::json!({ "i": i }),
            })
            .await
            .unwrap();
        }
        let history = bus.get_event_history().await;
        assert_eq!(history.len(), 5);
        assert_eq!(custom_index(&history.first().unwrap().0), 0);
        assert_eq!(custom_index(&history.last().unwrap().0), 4);
    }

    /// A handler that (re)registers a handler while an event is being dispatched.
    /// This takes the handlers write-lock; if dispatch held a read-lock across
    /// the await it would deadlock (BUG-14).
    struct ReRegisteringHandler {
        bus: Arc<EventBus>,
    }

    #[async_trait]
    impl EventHandler for ReRegisteringHandler {
        async fn handle_event(&self, _event: SkillEvent) -> Result<(), ServiceError> {
            self.bus
                .register_handler("skill:updated", LoggingEventHandler::new())
                .await?;
            Ok(())
        }
    }

    /// BUG-14: dispatch must not hold the handlers read-lock across handler
    /// awaits, so a handler that registers another handler cannot deadlock.
    #[tokio::test]
    async fn test_handler_registering_during_dispatch_does_not_deadlock() {
        let bus = Arc::new(EventBus::new());
        bus.register_handler(
            "skill:unregistered",
            ReRegisteringHandler { bus: bus.clone() },
        )
        .await
        .unwrap();

        let fut = bus.publish_skill_unregistered("skill-1".to_string());
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), fut).await;

        assert!(
            result.is_ok(),
            "publish_event deadlocked while a handler re-registered"
        );
        result.unwrap().unwrap();

        // The nested registration actually took effect.
        let registered = bus.get_registered_handlers().await;
        assert_eq!(registered.get("skill:updated").copied(), Some(1));
    }

    // ---- helpers -------------------------------------------------------------

    use crate::core::service::SkillId;
    use crate::core::skill_manager::SkillDefinition;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sample_skill() -> SkillDefinition {
        let id = SkillId::new("sample-skill".to_string()).unwrap();
        SkillDefinition::new(
            id,
            "Sample".to_string(),
            "A sample skill".to_string(),
            "1.0.0".to_string(),
        )
    }

    /// Handler that counts invocations.
    struct CountingHandler {
        count: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl EventHandler for CountingHandler {
        async fn handle_event(&self, _event: SkillEvent) -> Result<(), ServiceError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Handler that always fails (exercises the warn/error path in notify_handlers).
    struct FailingHandler;
    #[async_trait]
    impl EventHandler for FailingHandler {
        async fn handle_event(&self, _event: SkillEvent) -> Result<(), ServiceError> {
            Err(ServiceError::Custom("boom".to_string()))
        }
    }

    // ---- subscribe -----------------------------------------------------------

    #[tokio::test]
    async fn test_subscribe_receives_published_event() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.publish_skill_unregistered("s1".to_string())
            .await
            .unwrap();
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, SkillEvent::SkillUnregistered { skill_id } if skill_id == "s1"));
    }

    // ---- register / notify / unregister -------------------------------------

    #[tokio::test]
    async fn test_registered_handler_is_notified() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        bus.register_handler(
            "skill:unregistered",
            CountingHandler {
                count: count.clone(),
            },
        )
        .await
        .unwrap();

        bus.publish_skill_unregistered("s1".to_string())
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // A handler registered for a different type is not called.
        bus.publish_skill_reloaded("s2".to_string(), true, None)
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_failing_handler_does_not_break_publish() {
        let bus = EventBus::new();
        bus.register_handler("skill:unregistered", FailingHandler)
            .await
            .unwrap();
        // Publish still succeeds even though the handler errored.
        let subs = bus
            .publish_skill_unregistered("s1".to_string())
            .await
            .unwrap();
        assert_eq!(subs, 0, "no broadcast subscribers");
    }

    #[tokio::test]
    async fn test_unregister_clears_handlers() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        bus.register_handler(
            "skill:updated",
            CountingHandler {
                count: count.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            bus.get_registered_handlers().await.get("skill:updated"),
            Some(&1)
        );

        bus.unregister_handler("skill:updated", "ignored-id")
            .await
            .unwrap();
        assert_eq!(
            bus.get_registered_handlers().await.get("skill:updated"),
            Some(&0)
        );

        // Publishing now invokes nothing.
        bus.publish_skill_updated(
            "s".to_string(),
            SkillUpdate {
                name: None,
                description: None,
                version: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_unregister_unknown_type_is_noop() {
        let bus = EventBus::new();
        // No panic / error when the event type was never registered.
        bus.unregister_handler("never-registered", "id")
            .await
            .unwrap();
    }

    // ---- every SkillEvent variant flows through notify_handlers --------------

    #[tokio::test]
    async fn test_publish_all_variants_via_convenience_methods() {
        let bus = EventBus::new();

        bus.publish_skill_registered("s".to_string(), sample_skill())
            .await
            .unwrap();
        bus.publish_skill_updated(
            "s".to_string(),
            SkillUpdate {
                name: Some("n".to_string()),
                description: Some("d".to_string()),
                version: Some("2.0.0".to_string()),
            },
        )
        .await
        .unwrap();
        bus.publish_skill_unregistered("s".to_string())
            .await
            .unwrap();
        bus.publish_skill_reloaded("s".to_string(), true, None)
            .await
            .unwrap();
        bus.publish_skill_reloaded("s".to_string(), false, Some("err".to_string()))
            .await
            .unwrap();
        bus.publish_skill_validation_failed("s".to_string(), vec!["e1".to_string()])
            .await
            .unwrap();
        bus.publish_hot_reload_enabled(HotReloadConfig {
            watch_paths: vec!["p".to_string()],
            debounce_ms: 10,
            auto_reload: true,
            max_concurrent_reloads: 1,
        })
        .await
        .unwrap();
        bus.publish_hot_reload_disabled().await.unwrap();
        bus.publish_event(SkillEvent::Custom {
            event_type: "custom:thing".to_string(),
            data: serde_json::json!({"k": "v"}),
        })
        .await
        .unwrap();

        // All nine events are in history.
        assert_eq!(bus.get_event_history().await.len(), 9);
    }

    // ---- history management --------------------------------------------------

    #[tokio::test]
    async fn test_clear_event_history() {
        let bus = EventBus::new();
        bus.publish_skill_unregistered("s".to_string())
            .await
            .unwrap();
        assert!(!bus.get_event_history().await.is_empty());
        bus.clear_event_history().await;
        assert!(bus.get_event_history().await.is_empty());
    }

    #[tokio::test]
    #[allow(clippy::default_constructed_unit_structs)]
    async fn test_default_constructs() {
        let _bus = EventBus::default();
        let _logging = LoggingEventHandler::default();
        let _metrics = MetricsEventHandler::default();
    }

    // ---- LoggingEventHandler covers every variant ---------------------------

    #[tokio::test]
    async fn test_logging_handler_all_variants() {
        let h = LoggingEventHandler::new();
        let events = vec![
            SkillEvent::SkillRegistered {
                skill_id: "s".to_string(),
                skill: Box::new(sample_skill()),
            },
            SkillEvent::SkillUpdated {
                skill_id: "s".to_string(),
                changes: SkillUpdate {
                    name: None,
                    description: None,
                    version: None,
                },
            },
            SkillEvent::SkillUnregistered {
                skill_id: "s".to_string(),
            },
            SkillEvent::SkillReloaded {
                skill_id: "s".to_string(),
                success: true,
                error_message: None,
            },
            SkillEvent::SkillReloaded {
                skill_id: "s".to_string(),
                success: false,
                error_message: Some("e".to_string()),
            },
            SkillEvent::SkillValidationFailed {
                skill_id: "s".to_string(),
                errors: vec!["e".to_string()],
            },
            SkillEvent::HotReloadEnabled {
                config: HotReloadConfig {
                    watch_paths: vec!["p".to_string()],
                    debounce_ms: 1,
                    auto_reload: false,
                    max_concurrent_reloads: 1,
                },
            },
            SkillEvent::HotReloadDisabled,
            SkillEvent::Custom {
                event_type: "c".to_string(),
                data: serde_json::json!({}),
            },
        ];
        for e in events {
            h.handle_event(e).await.unwrap();
        }
    }

    // ---- MetricsEventHandler tallies event types ----------------------------

    #[tokio::test]
    async fn test_metrics_handler_counts_event_types() {
        let metrics = MetricsEventHandler::new();
        metrics
            .handle_event(SkillEvent::SkillUnregistered {
                skill_id: "a".to_string(),
            })
            .await
            .unwrap();
        metrics
            .handle_event(SkillEvent::SkillUnregistered {
                skill_id: "b".to_string(),
            })
            .await
            .unwrap();
        metrics
            .handle_event(SkillEvent::HotReloadDisabled)
            .await
            .unwrap();
        metrics
            .handle_event(SkillEvent::Custom {
                event_type: "custom:x".to_string(),
                data: serde_json::json!({}),
            })
            .await
            .unwrap();

        let counts = metrics.get_event_counts().await;
        assert_eq!(counts.get("skill:unregistered"), Some(&2));
        assert_eq!(counts.get("hot-reload:disabled"), Some(&1));
        assert_eq!(counts.get("custom:x"), Some(&1));
    }

    #[tokio::test]
    async fn test_metrics_handler_all_remaining_variants() {
        let metrics = MetricsEventHandler::new();
        let events = vec![
            SkillEvent::SkillRegistered {
                skill_id: "s".to_string(),
                skill: Box::new(sample_skill()),
            },
            SkillEvent::SkillUpdated {
                skill_id: "s".to_string(),
                changes: SkillUpdate {
                    name: None,
                    description: None,
                    version: None,
                },
            },
            SkillEvent::SkillReloaded {
                skill_id: "s".to_string(),
                success: true,
                error_message: None,
            },
            SkillEvent::SkillValidationFailed {
                skill_id: "s".to_string(),
                errors: vec![],
            },
            SkillEvent::HotReloadEnabled {
                config: HotReloadConfig {
                    watch_paths: vec![],
                    debounce_ms: 0,
                    auto_reload: false,
                    max_concurrent_reloads: 0,
                },
            },
        ];
        for e in events {
            metrics.handle_event(e).await.unwrap();
        }
        let counts = metrics.get_event_counts().await;
        assert_eq!(counts.get("skill:registered"), Some(&1));
        assert_eq!(counts.get("skill:updated"), Some(&1));
        assert_eq!(counts.get("skill:reloaded"), Some(&1));
        assert_eq!(counts.get("skill:validation:failed"), Some(&1));
        assert_eq!(counts.get("hot-reload:enabled"), Some(&1));
    }
}
