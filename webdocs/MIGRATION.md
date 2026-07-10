# Fumadocs Migration Notes

This directory contains documentation content converted from Mintlify to [Fumadocs](https://fumadocs.dev) MDX format.

## What changed

### Navigation: mint.json → meta.json

Mintlify's central `mint.json` was replaced with per-folder `meta.json` files. Fumadocs derives the sidebar from the folder structure plus `meta.json` ordering. The root `meta.json` uses `---Separator---` entries to recreate the original navigation groups (Getting Started, Configuration, Skill Management, etc.).

### Component mappings

| Mintlify | Fumadocs | Notes |
|----------|----------|-------|
| `<Note>` | `<Callout>` | Default type (info) |
| `<Info>` | `<Callout type="info">` | |
| `<Warning>` | `<Callout type="warn">` | |
| `<Tip>` | `<Callout>` | Mapped to default (info) |
| `<Check>` | `<Callout type="success">` | |
| `<CardGroup>` / `<Card icon="...">` | `<Cards>` / `<Card>` | `icon` props stripped (string names need JSX components in host app) |
| `<AccordionGroup>` / `<Accordion>` | `<Accordions>` / `<Accordion id="..." title="...">` | Slugified `id` added |
| `<Steps>` / `<Step title="X">` | `### X [step]` headings | Requires `remark-steps` plugin in host app |
| `<Tabs>` / `<Tab title="X">` | `<Tabs items=[...]>` / `<Tab value="X">` | |
| `<ParamField path="..." type="...">` | `#### \`field\` (type, required/optional)` | Converted to heading + bullets |
| `<Frame>` | removed | Content (images) kept |

### Other changes

- `TROUBLESHOOTING.md` renamed to `troubleshooting.mdx` for slug consistency.
- Fixed duplicate `</Accordions>` in `skill-management/validation.mdx`.
- Card `icon="..."` props were stripped. Fumadocs requires JSX icon components (e.g. `lucide-react`), not string names. Add icons in the host app via the `icon` handler on `loader()` or per-card.

## What the hosting app needs to do

This migration covers content only. To serve these docs:

1. **Create a fumadocs app** (Next.js recommended): `pnpm create fumadocs-app`
2. **Point content source at this directory**: set `dir: 'content/docs'` (or wherever you place these files) in `source.config.ts` → `defineDocs()`.
3. **Enable remark-steps**: the `[step]` heading markers require the `remark-steps` plugin. Add it to `mdxOptions` in `source.config.ts`.
4. **Place static assets in `public/`**: images referenced as `/images/...` and logos in `/logo/` must live in the host app's `public/` directory.
5. **Configure theme and search**: colors, logo, topbar CTA, and search (Algolia/Orama) are host-app concerns, not content-level config.
6. **Map card icons** (optional): if you want icons on cards, pass an `icon` handler to `loader()` that resolves icon names to `lucide-react` components.

### File structure

```
webdocs/
  meta.json              ← root navigation (groups + ordering)
  index.mdx              ← landing page
  welcome.mdx
  quickstart.mdx
  ...
  cli-reference/
    meta.json            ← folder page ordering
    overview.mdx
    ...
  images/                ← static assets (move to public/ in host app)
  logo/                  ← logo SVGs (move to public/ in host app)
```
