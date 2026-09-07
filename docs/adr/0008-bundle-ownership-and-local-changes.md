# Bundle lifecycle preserves shared skills and local changes

Status: accepted

Multiple bundles and individual installation may require the same skill. FastSkill shares identical contents under the same skill ID and records all owners. Differing contents cause a conflict before any installation changes. Removing one bundle preserves skills required by another owner, including an explicit individual installation.

Bundle updates and removals stop before changing the installation when they would replace or delete locally modified members. The user explicitly discards the changes or retains a personal override where permitted. Unrelated personal additions remain untouched.

Members are required unless the bundle author explicitly marks them overridable. Every owning bundle must permit a shared skill override. A new release that invalidates an existing override stops with a conflict. This prevents a more permissive bundle from weakening another owner's requirements; these are installer rules, not enforcement over independently launched agents.

An approved override is an explicitly declared personal skill selection with its own origin. Users retain edited contents in that origin and record the replacement; editing an installed file alone does not approve an override. This makes retained customizations visible instead of treating untracked drift as policy-compliant.

We chose explicit ownership and conflict resolution over last-installed-wins replacement. This requires tracking membership and content identity but prevents one bundle from silently changing another bundle's setup or deleting user work.

Bundle installation stages and validates all contents before applying changes, and retains recovery information to restore the previous setup if application fails. Skill files, ownership records, Manifest, and Lock must remain consistent. We accept the staging and recovery cost instead of leaving a partly applied team setup after an I/O failure. Recovery mechanics and visibility to concurrently running agents remain implementation/design questions; this does not claim an atomic snapshot for an independently running agent.
