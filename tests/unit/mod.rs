//! Unit tests for the FastSkill service

pub mod service;
pub mod skill_manager;
pub mod metadata;
pub mod loading;
pub mod tool_calling;
pub mod routing;

#[cfg(test)]
pub mod core {
    pub mod registry {
        pub mod index_manager_test;
    }
}
