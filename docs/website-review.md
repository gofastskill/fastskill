# FastSkill website review and preview

Website positioning and onboarding proposal, reviewed on 2026-09-10.

## Recommendation

Lead with reproducible skill setups:

> The same skills. On every machine.

Supporting message:

> Install the skills your AI agent needs. Lock their versions. Share the exact
> setup with your team, from a developer’s laptop to CI.

The strongest product story is the progression from installing one useful skill
to sharing a tested, versioned setup. Lockfiles and self-contained bundles make
that story concrete. Evaluations add evidence before a team adopts an update.

This is a positioning recommendation grounded in the product, not a claim that
competitors lack these features. Identical skill files do not guarantee identical
model responses or configure every agent automatically.

## Assessment of the original page

The green-and-lime identity, clear typography, source link, and lightweight static
implementation are good foundations. The biggest opportunity is conversion:
help a visitor understand the benefit and complete a useful first install.

| Priority | Original page | Proposed change |
| --- | --- | --- |
| 1 | The hero lists manifests, lockfiles, bundles, search, and evals. | State the outcome first; explain the mechanism below. |
| 1 | “Start in five minutes” sends visitors away before showing installation. | Put OS and method tabs, copyable commands, and verification in the hero. |
| 1 | Terminal uses obsolete root commands and assumes an existing skill configuration. | Use a complete walkthrough verified with the released CLI. |
| 2 | Six features receive equal prominence. | Lead with install, reproduce, share; introduce advanced features afterward. |
| 2 | “Keep every agent in sync” can imply automatic agent configuration. | Explain that FastSkill manages files in the chosen directory; the agent reads them. |
| 2 | A large blue bundle section competes with the green brand. | Use deep green for installation and bundles; reserve lime for emphasis and selection. |
| 3 | A “verified” badge appears on an illustrative manifest. | Label illustrations as examples and describe the actual integrity mechanism. |

## Color and layout

Keep the recognizable brand rather than introduce a new palette.

| Role | Color | Reason |
| --- | --- | --- |
| Background | #f7f9f5 | A cleaner neutral close to the existing page. |
| Text | #142b20 | Strong contrast with the light surface. |
| Supporting text | #53645b | Readable without competing with headings. |
| Brand green | #246a45 | Links and selected headline emphasis. |
| Dark surfaces | #122a1f | A consistent terminal and bundle treatment. |
| Accent | #c9f75b | Preserve identity; dark text on lime, not lime on white. |

The preview replaces the tilted illustrative terminal with a functional installer,
reduces decorative badges, and places the quick start before advanced features.
It uses a single column on small screens, wrapping commands, keyboard tabs,
visible focus, copy feedback, and a no-JavaScript installation fallback.

## Installation design

Choose the OS first, then a valid method. Desktop OS detection is a suggestion;
tabs remain manually selectable.

| OS | Default | Alternatives |
| --- | --- | --- |
| macOS | Homebrew | Direct archive with curl; Apple Silicon or Intel archive download. |
| Linux x86_64 | Shell installer via curl | Homebrew; musl or GNU archive download. |
| Windows x86_64 | Scoop | ZIP download, extraction, verification, and PATH guidance. |

Releases provide .tar.gz for macOS/Linux and .zip for Windows. Do not label every
download “ZIP” or imply that a native Linux ARM64 release exists.
Each path includes verification and prerequisites. Package manager bootstrap
instructions link to the manager's official site.

The first-skill example installs the official gofastskill/skill repository,
which teaches an agent to use FastSkill. It includes a new demo folder,
initialization, installation, listing, and the next agent prompt.
The Cursor note shows how to change the installation directory.

## Evidence and onboarding issues

Checked against fetched origin/main at 92cf946 and installed FastSkill 0.9.225.
The public release API and live Homebrew/Scoop manifests also reported 0.9.225
on 2026-09-10.

- Current commands are “project init”, “project install”, and “skill add/list”;
  the original website's “fastskill init” and “fastskill install” are obsolete.
- The shell installer calls “grep -P” to extract the latest version. Stock macOS
  grep lacks that option. The preview uses Homebrew or direct archive download
  on macOS. The installer itself was not changed.
- Installing the Anthropic frontend-design skill using its GitHub tree/main URL
  failed in the released CLI: “Failed to resolve ref” and “repository ... not
  found.” Origin inference parses a subdirectory but retains the input tree URL.
  The preview uses the working official FastSkill skill repository instead.
- Initialization derives an ID from the current directory and rejects names
  containing a dot. The walkthrough creates a known-valid fastskill-demo folder.
- The official skill installed successfully as fastskill version 2.0.0.
  Listing and “fastskill project install --lock” succeeded, with indexing
  skipped because no embedding provider was configured.

Sources: [release](https://github.com/gofastskill/fastskill/releases/tag/v0.9.225),
[Homebrew formula](https://github.com/gofastskill/homebrew-cli/blob/main/Formula/fastskill.rb),
[Scoop manifest](https://github.com/gofastskill/scoop-bucket/blob/main/bucket/fastskill.json),
[official skill](https://github.com/gofastskill/skill),
[installer source](../scripts/install.sh),
[origin inference](../crates/fastskill-core/src/core/origin_infer.rs),
[project initialization](../crates/fastskill-cli/src/commands/init.rs).

## What would strengthen the advantage next

1. Publish a maintained starter bundle for a specific job, with a useful example
   task and a recorded evaluation result. “Install this setup and do this task”
   is stronger evidence than another feature list.
2. Repair GitHub subdirectory installs and the macOS shell installer before
   promoting them in acquisition content.
3. Try the page with first-time users: can they explain the benefit, choose their
   platform, install a skill, and use it without extra help?
4. Consider a short real demo of installing a bundle on a second machine.
   Avoid invented adoption figures or speed claims.

For discussion: should team consistency remain the lead benefit, or is the
audience primarily individual skill authors? For the latter, an alternative is
“Test your skills. Ship what works.” The preview recommends team consistency
because it connects the first install to a reason to keep using FastSkill.

## Validation and limits

- Linux first-skill walkthrough and locked restore passed with the released CLI.
- Release assets and package manifests checked; Windows download returned HTTP 200.
- HTML anchors, local asset references, unique IDs, CSS parsing, JavaScript syntax,
  and Git whitespace checks passed.
- The page and its assets served successfully during local preview.
- macOS and Windows installers were not executed on those operating systems.
- Responsive and keyboard behavior are implemented but have not been browser-tested.

The website keeps its CSS and JavaScript inline in index.html, with no build step,
tracking, or new dependencies. Review notes live outside the served website directory.
