# Understand FastSkill

FastSkill 0.9.228

Source: https://docs.gofastskill.com/welcome

Release revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d

Documentation revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d



FastSkill installs and manages directories containing `SKILL.md` and supporting
resources. Agents read these instructions when relevant to a task. FastSkill does
not run skills as a sandbox or guarantee that an agent follows their instructions.

## The pieces

| Piece                | Purpose                                                                |
| -------------------- | ---------------------------------------------------------------------- |
| Skill                | Instructions and resources for a task                                  |
| `skill-project.toml` | Desired dependencies, sources, groups, and installation directory      |
| `skills.lock`        | Selected versions, revisions, integrity evidence, and ownership        |
| Installed directory  | Files the agent can discover and read                                  |
| Bundle               | A versioned ZIP containing a team skill set and its dependency closure |

## Choose the operation

* `skill add` declares and installs a skill from a source.
* `project install` restores compatible locked selections and resolves missing coverage.
* `project install --lock` restores the locked selection without updating the lock.
* `skill update` deliberately resolves changes allowed by the recorded constraints.
* `skill remove` removes a direct owner; shared requirements can keep a skill installed.
* `skill list --check` verifies the desired, locked, and installed state.

Start with [your first skill](/quickstart). Continue with
[manifests and scope](/skill-management/manifest-system),
[team bundles](/cli-reference/bundle-command), or [agent access](/integration/agents).

