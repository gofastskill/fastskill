use super::{AddMode, AddOutcome, PreparedSkill};
use crate::core::service::{FastSkillService, ServiceError};

impl PreparedSkill {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn resolved(&self) -> &crate::core::origin::Resolved {
        &self.fetched.resolved
    }

    pub fn dependencies(&self) -> &[crate::core::manifest::SkillEntry] {
        &self.dependencies
    }
}

impl FastSkillService {
    /// Apply a previously prepared candidate without changing desired state or
    /// the Lock. The caller persists one deterministic Lock after the complete
    /// operation succeeds.
    pub async fn apply_prepared_install(
        &self,
        prepared: PreparedSkill,
        groups: Vec<String>,
    ) -> Result<AddOutcome, ServiceError> {
        self.commit(
            prepared.fetched,
            prepared.origin,
            AddMode::Update,
            groups,
            false,
            false,
        )
        .await
    }

    /// Commit prepared content only, preserving the caller's Fresh/Update
    /// conflict policy. Global commands own their separate desired-state file.
    pub async fn apply_prepared_content(
        &self,
        prepared: PreparedSkill,
        mode: AddMode,
        groups: Vec<String>,
    ) -> Result<AddOutcome, ServiceError> {
        self.commit(
            prepared.fetched,
            prepared.origin,
            mode,
            groups,
            false,
            false,
        )
        .await
    }
}
