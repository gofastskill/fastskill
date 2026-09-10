# Documentation development

See [CONTRIBUTING.md](../CONTRIBUTING.md#documentation-site) for the release baseline,
content checks, static build, and publication procedure. This application documents
one current release; it does not contain historical documentation or migration guides.

- Root and topic `.mdx` files are the content source; `meta.json` controls navigation.
- `lib/source.ts` lists content folders explicitly.
- `app/` and `components/` provide the Fumadocs shell and FastSkill theme.
- `lib/markdown-options.ts` preserves component content in agent-readable Markdown.
- `scripts/` validates examples, exercises user journeys, and checks exported assets.
