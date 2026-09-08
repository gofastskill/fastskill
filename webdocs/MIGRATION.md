# Fumadocs site

`webdocs/` is a self-contained Fumadocs application. It serves the existing MDX files at their
current root paths, so `/quickstart` and `/cli-reference/bundle-command` remain stable.

## Run locally

```shell
cd webdocs
pnpm install --frozen-lockfile
pnpm dev
```

Open <http://localhost:3000>. Run `pnpm build`, `pnpm lint`, and `pnpm typecheck` before changing
the app shell, theme, or MDX components. The build exports a static site to `webdocs/out/`.

## Structure

- Root and topic-folder `.mdx` files are the documentation source. `lib/source.ts` lists the
  included folders explicitly so application code and dependencies are never treated as content.
- `app/` contains the static Next.js routes and FastSkill theme.
- `components/` contains the shared brand, search, MDX, and documentation-hero components.
- `public/` contains images, logos, and the favicon.
- Folder `meta.json` files control navigation order.

## Visual system

The theme in `app/global.css` shares the marketing site's palette and visual language:

| Token | Value | Use |
|---|---|---|
| Ink | `#14251d` | Primary text, actions, terminal surfaces |
| Paper | `#f7f8f2` | Page background |
| Lime | `#c9f75b` | Focus, status, and brand accents |
| Green | `#1f7a4d` | Links, labels, and navigation emphasis |
| Blue | `#264fdd` | Team-bundle emphasis |

Keep Fumadocs color variables and custom components aligned with `website/index.html` when the
brand changes. Preserve readable contrast and the reduced-motion fallback in both themes.
