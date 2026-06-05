# fastskill-core

Core Rust library for FastSkill skill management, discovery, validation, and runtime services.

`fastskill-core` is designed for developers embedding FastSkill capabilities in their own tools, services, or runtimes. It exposes service-layer APIs and re-exports shared eval primitives under `fastskill_core::eval`.

## Install

Add the crate from this workspace:

```toml
[dependencies]
fastskill-core = { path = "../fastskill-core" }
```

## Quick start

```rust
use fastskill_core::{FastSkillService, ServiceConfig};
use std::path::PathBuf;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let config = ServiceConfig {
    skill_storage_path: PathBuf::from("./skills"),
    ..Default::default()
};

let mut service = FastSkillService::new(config).await?;
service.initialize().await?;

let skills = service.skill_manager().list_skills(None).await?;
println!("Loaded {} skills", skills.len());
# Ok(())
# }
```

## Agent integration example (context-aware retrieval)

Use this pattern when your agent needs to pick relevant skills from local folders for each user turn.

### What this flow does

1. Initializes `FastSkillService` with your skills directory.
2. Searches local skills using the current user query/context.
3. Loads `SKILL.md` content for top matches so your planner/executor can use them.

### Example

```rust
use fastskill_core::{
    search::{self, SearchQuery, SearchScope},
    FastSkillService, ServiceConfig, SkillId,
};
use std::path::PathBuf;

pub async fn retrieve_skills_for_turn(
    user_query: &str,
    skills_dir: PathBuf,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    // 1) Boot service and index skills from filesystem
    let mut service = FastSkillService::new(ServiceConfig {
        skill_storage_path: skills_dir,
        ..Default::default()
    })
    .await?;
    service.initialize().await?;

    // 2) Search local skills by current context/query
    let hits = search::execute(
        SearchQuery {
            query: user_query.to_string(),
            scope: SearchScope::Local,
            limit: 5,
            // Keep deterministic text search for simple agent integration.
            embedding: Some(false),
        },
        &service,
    )
    .await?;

    // 3) Load SKILL.md of top matches
    let mut loaded = Vec::new();
    for hit in hits {
        let skill_id = SkillId::new(hit.id.clone())?;
        if let Some(def) = service.skill_manager().get_skill(&skill_id).await? {
            let skill_md = tokio::fs::read_to_string(&def.skill_file).await?;
            loaded.push((hit.id, skill_md));
        }
    }

    Ok(loaded)
}
```

### Notes

- Use `embedding: Some(true)` only when embedding config and `OPENAI_API_KEY` are set.
- Start with a small `limit` (for example `3..7`) to control token usage.
- Build `user_query` from your agent context: task goal, constraints, and recent conversation.

## Feature flags

- `filesystem-storage` (default): local storage backend.
- `registry-publish` (default): registry publishing support.
- `hot-reload`: filesystem watch support for skill changes.

## Core capabilities

- Skill management and metadata parsing.
- Search and vector index integration.
- Validation for skill/project structure and safety checks.
- HTTP server components for service hosting.
- Re-exported evaluation APIs via `fastskill_core::eval`.

## Related documentation

- Workspace overview: [`../../README.md`](../../README.md)
- Workspace contribution guide: [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md)
- Crate contribution guide: [`CONTRIBUTING.md`](CONTRIBUTING.md)
