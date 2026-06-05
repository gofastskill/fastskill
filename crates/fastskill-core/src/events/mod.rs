//! Event system for skill lifecycle management

pub mod event_bus;

// Re-export main types
pub use event_bus::{
    EventBus, EventHandler, HotReloadConfig, LoggingEventHandler, MetricsEventHandler, SkillEvent,
    SkillUpdate,
};
