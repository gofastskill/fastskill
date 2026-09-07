# FastSkill owns skill deployment within externally managed environments

Status: accepted

Team presets address repeated manual follow-up when teammates use incorrect or outdated skills. FastSkill owns the desired skill setup and its installation, verification, updates, and rollback. Existing infrastructure tooling owns environment provisioning, agent runtimes, external tools, and credentials.

We chose this boundary over expanding FastSkill into an application and multi-cloud infrastructure deployment platform. It keeps team skill distribution focused and lets existing deployment systems deliver the same approved setup across environments. Installing a skill does not grant execution permissions; identity and authorization remain external as established in ADR-0003.

Initial artifact publishing uses existing CI or storage tooling. FastSkill builds bundles and installs local ZIPs or public HTTPS artifacts; existing authenticated tools obtain private artifacts before local installation. This avoids coupling the bundle format and installer to cloud-specific login and upload mechanisms.

This decision assigns product responsibilities. Managed launch is optional future work. Preset composition, session verification, and fleet rollout mechanisms are not settled by this ADR.
