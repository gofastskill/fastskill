# FastSkill

Package manager and operational toolkit for Claude Code-compatible skills. FastSkill enables discovery, installation, versioning, and deployment of skills at scale.

[![Python/Rust package build status](https://github.com/gofastskill/fastskill/actions/workflows/ci.yml/badge.svg)](https://github.com/gofastskill/fastskill/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/gofastskill/fastskill/branch/main/graph/badge.svg)](https://codecov.io/gh/gofastskill/fastskill)

## Add a skill

```bash
fastskill add https://github.com/org/skill-repo
```

Quickly add skills from Git, local folders, or registries to extend your AI agent's capabilities.

## What is FastSkill?

FastSkill is a skill package manager and operational toolkit for the AI agent ecosystem. It builds on Anthropic's standardized Skills format, adding registry services, semantic search, version management, and deployment tooling.

**What are skills?** Skills are reusable instruction sets in SKILL.md that extend an AI agent's capabilities with specialized procedures, tool integrations, and domain knowledge. Examples include creating pull requests, integrating cloud services, automating CI/CD workflows, and domain-specific data processing. FastSkill provides the infrastructure to develop, manage, consume, and deploy skills at scale.

## AI Agentic Skills Standard

FastSkill supports the [AI Agentic Skills Specification](https://agentskills.io/specification).

### Skill Requirements

According to the standard, a skill must have:

- **Required**: `SKILL.md` file with YAML frontmatter
  - `name`: Skill identifier (1-64 chars, lowercase alphanumeric + hyphens)
  - `description`: What the skill does (1-1024 chars)

- **Optional**: `skill-project.toml` for advanced features
  - Recommended for skill authors
  - Provides dependency management
  - Enables version tracking

### Adding Skills Without skill-project.toml

```bash
# Works with standard-compliant skills
fastskill add ./my-skill

# Will use metadata from SKILL.md:
# - Skill ID: from metadata.id or name field
# - Version: from metadata.version or defaults to 1.0.0
# - Warning displayed recommending 'fastskill init'
```

### Migration Guide

For existing skills without `skill-project.toml`:

```bash
# Navigate to skill directory
cd my-skill

# Run fastskill init to add skill-project.toml
fastskill init

# This creates skill-project.toml with:
# [metadata]
# id = "my-skill"  # From SKILL.md name
# version = "1.0.0"  # From SKILL.md metadata.version
```

## Key Capabilities

- **Package Management**: Install, update, and remove skills from multiple sources (Git, local, ZIP)
- **Semantic Search**: Find skills using OpenAI embeddings and natural language queries with high accuracy
- **Registry Services**: Publish, version, and distribute skills via registry
- **Manifest System**: Declarative dependency management with lock files for reproducible installations
- **HTTP API**: RESTful service layer for agent integration (requires authentication for protected endpoints)
- **Web UI**: Browse and manage skills through web interface

## Compatible Agents

Optimized for Claude Code. Skills follow the same SKILL.md format used by Cursor and other AI agents.

## Core Use Cases

- **Skill Authors**: Package, version, and publish skills to registries
- **Agent Developers**: Discover and install skills for agent capabilities
- **Teams**: Share internal skills via private registries with version control
- **CI/CD**: Automate skill packaging, publishing, and deployment pipelines
- **Production Systems**: Manage skill lifecycles in agentic applications

## Benefits Over Baseline

Without FastSkill, teams manually manage skills through ad-hoc scripts, copy-paste workflows, and manual version tracking. FastSkill provides:

- **Standardized Workflows**: Consistent patterns for skill lifecycle management
- **Version Control**: Semantic versioning and dependency resolution
- **Discovery**: Semantic search eliminates manual skill cataloging
- **Reproducibility**: Lock files ensure consistent installations across environments
- **Scalability**: Registry architecture supports hundreds of skills across teams
- **Automation**: CLI and API enable CI/CD integration

## Installation

FastSkill can be installed in several ways depending on your use case:

### CLI Installation

**Quick Install (Recommended)**

Install FastSkill with a single command:

```bash
curl -fsSL https://raw.githubusercontent.com/gofastskill/fastskill/main/scripts/install.sh | bash
```

Or download and run the script manually:

```bash
wget https://raw.githubusercontent.com/gofastskill/fastskill/main/scripts/install.sh
chmod +x install.sh
./install.sh
```

The script automatically:
- Detects your platform
- Downloads the latest version (or specify a version: `./install.sh v0.6.8`)
- Installs to `/usr/local/bin` (or `~/.local/bin` if sudo is unavailable)
- Verifies the installation

**Options:**
- `--user`: Install to `~/.local/bin` instead of system directory
- `--prefix DIR`: Install to a custom directory
- `--force`: Overwrite existing installation
- `--help`: Show all available options

**Homebrew (macOS + Linux)**

Install FastSkill via [Homebrew](https://brew.sh/) on macOS or Linux:

```bash
brew install gofastskill/cli/fastskill
```

For more details, see the [Homebrew tap repository](https://github.com/gofastskill/homebrew-cli).

**Scoop (Windows)**

Install FastSkill via [Scoop](https://scoop.sh/) on Windows:

```powershell
scoop bucket add gofastskill https://github.com/gofastskill/scoop-bucket
scoop install fastskill
```

For more details, see the [Scoop bucket repository](https://github.com/gofastskill/scoop-bucket).

**Manual Installation from GitHub Releases**

Download the pre-built binary for your platform from [GitHub Releases](https://github.com/gofastskill/fastskill/releases).

**macOS:**

Two macOS binaries are available:

| Binary | Hardware |
|--------|----------|
| `fastskill-aarch64-apple-darwin.tar.gz` | Apple Silicon (M1/M2/M3+) |
| `fastskill-x86_64-apple-darwin.tar.gz` | Intel Macs |

**Apple Silicon example:**

```bash
VERSION="0.8.6"  # Replace with latest version
wget https://github.com/gofastskill/fastskill/releases/download/v${VERSION}/fastskill-aarch64-apple-darwin.tar.gz
tar -xzf fastskill-aarch64-apple-darwin.tar.gz
sudo mv fastskill /usr/local/bin/
fastskill --version
```

**Intel macOS example:**

```bash
VERSION="0.8.6"  # Replace with latest version
wget https://github.com/gofastskill/fastskill/releases/download/v${VERSION}/fastskill-x86_64-apple-darwin.tar.gz
tar -xzf fastskill-x86_64-apple-darwin.tar.gz
sudo mv fastskill /usr/local/bin/
fastskill --version
```

**Linux:**

Two Linux binaries are available:

| Binary | Best For | Compatibility |
|--------|----------|---------------|
| `fastskill-x86_64-unknown-linux-musl.tar.gz` | Containers, CI/CD, older distributions | Universal - works on any Linux (Ubuntu 18.04+, RHEL 7+, Alpine, etc.). Note: Built without git-support; use system git for git operations. |
| `fastskill-x86_64-unknown-linux-gnu.tar.gz` | FIPS/compliance, full git2 support | Requires glibc 2.38+ (Ubuntu 24.04+, Fedora 39+). Includes native git integration. |

**Recommended: Use the musl (static) binary for maximum compatibility:**

```bash
VERSION="0.8.6"  # Replace with latest version
wget https://github.com/gofastskill/fastskill/releases/download/v${VERSION}/fastskill-x86_64-unknown-linux-musl.tar.gz
tar -xzf fastskill-x86_64-unknown-linux-musl.tar.gz
sudo mv fastskill /usr/local/bin/
fastskill --version
```

**For FIPS/compliance environments requiring dynamic linking:**

```bash
VERSION="0.8.6"  # Replace with latest version
wget https://github.com/gofastskill/fastskill/releases/download/v${VERSION}/fastskill-x86_64-unknown-linux-gnu.tar.gz
tar -xzf fastskill-x86_64-unknown-linux-gnu.tar.gz
sudo mv fastskill /usr/local/bin/
fastskill --version
```

**From Source:**
```bash
cargo install fastskill
# Or build from source
git clone https://github.com/gofastskill/fastskill.git
cd fastskill
cargo install --path .
```

**Requirements**: Rust nightly (for source builds), OpenAI API key for embedding features

### Kubernetes Deployment (Production)

Deploy FastSkill as a production service in Kubernetes using Helm:

```bash
# Create secrets and install chart
kubectl create namespace fastskill
kubectl create secret generic fastskill-github-token \
  --from-literal=GITHUB_TOKEN=your-token -n fastskill
kubectl create secret generic fastskill-s3-credentials \
  --from-literal=AWS_ACCESS_KEY_ID=your-key \
  --from-literal=AWS_SECRET_ACCESS_KEY=your-secret -n fastskill

helm install fastskill ./tools/fastskill/helm/fastskill \
  --namespace fastskill --create-namespace
```

For detailed Kubernetes deployment instructions, see the [Kubernetes Deployment Guide](/integration/kubernetes-deployment).

## Quick Start

### 1. Configure FastSkill

Create `skill-project.toml` in your project root:

```toml
[metadata]
id = "my-project"
version = "1.0.0"

[dependencies]
# Add skill dependencies here

[tool.fastskill]
skills_directory = ".claude/skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"

[[tool.fastskill.repositories]]
name = "anthropic"
type = "git-marketplace"
url = "https://github.com/anthropics/skills"
priority = 0
```

Or use the init command:

```bash
fastskill init
```

Set your OpenAI API key:

```bash
export OPENAI_API_KEY="your-key-here"
```

### 2. Add Skills

**Source formats**

| Source | Example |
|--------|---------|
| Git URL | `https://github.com/org/skill.git` |
| Tree URL (subdir) | `https://github.com/org/repo/tree/main/path/to/skill` |
| Local path | `./local-skill` |
| Recursive directory | `./skills -r` |
| Editable (dev) | `./local-skill -e` |

**Options**

| Flag | Description |
|------|-------------|
| `-r, --recursive` | Add all skills under directory (local folders only) |
| `-e, --editable` | Install in editable mode for local development |
| `-f, --force` | Force registration even if skill exists |
| `--branch <BRANCH>` | Git branch to checkout (git URLs only) |
| `--tag <TAG>` | Git tag to checkout (git URLs only) |
| `--source-type <TYPE>` | Override source type (registry, github, local) |
| `--group <GROUP>` | Add skill to a specific group |

```bash
# Add skill from git URL
fastskill add https://github.com/org/skill.git

# Add skill from a subdirectory (GitHub tree URL: tree/branch/path/to/skill)
fastskill add "https://github.com/org/repo/tree/main/path/to/skill"

# Add skill from local folder
fastskill add ./local-skill

# Add skill in editable mode (for local development)
fastskill add ./local-skill -e

# Add all skills under a directory (recursive)
fastskill add ./skills -r
```

### 3. Index and Search

```bash
# Index skills for semantic search
fastskill reindex

# Search for skills (semantic by default)
fastskill search "powerpoint presentation"
fastskill search "data processing" --limit 5

# Search using keyword-only (no API key required)
fastskill search --embedding false "powerpoint"
```

## Essential Commands

| Command | Description |
|---------|-------------|
| `fastskill add <source>` | Add skill from Git, local, or registry |
| `fastskill remove <skill-id>` | Remove skill from database |
| `fastskill show` | List installed skills and metadata |
| `fastskill update` | Update skills to latest versions |
| `fastskill search "query"` | Semantic search for skills |
| `fastskill reindex` | Rebuild vector index for search |
| `fastskill serve` | Start HTTP API server |
| `fastskill init` | Initialize skill-project.toml |
| `fastskill package` | Package skills into ZIP artifacts |
| `fastskill analyze matrix` | Show similarity matrix between all skills |

### Skill Management

```bash
fastskill add <source>              # Add skill (git URL, tree URL for subdir, local path, or ZIP). Use -r for recursive folder add
fastskill remove <skill-id>          # Remove skill
fastskill show                       # List installed skills
fastskill update                     # Update skills to latest versions
```

### Search and Discovery

```bash
fastskill search "query"             # Semantic search
fastskill reindex                    # Rebuild search index
```

### Server and API

```bash
fastskill serve                      # Start HTTP API server
fastskill serve --enable-registry    # Enable web UI at /registry
```

### Skill Authoring

```bash
fastskill init                       # Initialize skill metadata
fastskill package                    # Package skills into ZIP artifacts
fastskill package --force --recursive # Package all skills recursively from nested directories
```

### Diagnostic Commands

```bash
fastskill analyze matrix             # Show similarity matrix between all skills
```

- Identify related skills
- Find potential duplicates
- Verify embedding quality
- Export data in JSON format

## Repositories

FastSkill provides a unified repository system for managing all skill storage locations. Repositories can be:

- **Public registries** (HTTP-based index with S3 storage)
- **Private registries** (enterprise/internal registries)
- **Git repositories** (with marketplace.json for skill discovery)
- **ZIP URL sources** (static hosting with marketplace.json)
- **Local folders** (for development)

All repository types are configured in `skill-project.toml` under `[[tool.fastskill.repositories]]`.

For detailed repository setup, usage, and management instructions, see [docs/REGISTRY.md](docs/REGISTRY.md).

## Configuration

FastSkill uses `skill-project.toml` as the unified configuration file for both project-level and skill-level contexts.

### skill-project.toml Structure

```toml
[metadata]
id = "my-skill"
version = "1.0.0"

[dependencies]
# Add your skill dependencies here

[tool.fastskill]
skills_directory = ".claude/skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"

[[tool.fastskill.repositories]]
name = "anthropic"
type = "git-marketplace"
url = "https://github.com/anthropics/skills"
priority = 0
```

### Configuration Resolution

The CLI resolves configuration from `skill-project.toml`:

1. Searches current directory and parents for `skill-project.toml`
2. Extracts `[tool.fastskill]` section for skills directory and embedding config
3. Extracts `[tool.fastskill.repositories]` for repository sources
4. Defaults to `.claude/skills/` if no skills_directory is configured

### Installation Scope

FastSkill is project-scoped. Configuration and skills are managed per project using `skill-project.toml` in the project root. There is no global/user-level mode; each project maintains its own skills directory and dependencies.

### Skill Discovery

FastSkill looks for skills in the following locations:

- **skills_directory** - Configured path in `[tool.fastskill.skills_directory]`
- **Default** - `.claude/skills/` if not specified
- **Repositories** - All sources from `[[tool.fastskill.repositories]]` including:
  - Git marketplace repos with marketplace.json
  - HTTP registries with flat index
  - ZIP URL sources with marketplace.json
  - Local filesystem paths

### Environment Variables

| Variable | Description |
|----------|-------------|
| `OPENAI_API_KEY` | OpenAI API key for semantic search and embeddings (required) |
| `RUST_LOG` | Logging level (e.g., `fastskill=debug`, `fastskill=trace`) |
| `FASTSKILL_API_URL` | Base URL for registry API |
| `FASTSKILL_API_TOKEN` | Authentication token for registry API |
| `FASTSKILL_CONFIG_DIR` | Path to FastSkill configuration directory |
| `FASTSKILL_JWT_SECRET` | Secret for JWT token generation (HTTP API) |
| `FASTSKILL_JWT_ISSUER` | JWT issuer claim (default: `fastskill`) |
| `FASTSKILL_JWT_EXPIRY` | JWT token expiry in seconds |
| `FASTSKILL_STATIC_DIR` | Path to static files for HTTP server |

### Quick Setup

```bash
# Initialize a new project
fastskill init

# Set OpenAI API key for semantic search
export OPENAI_API_KEY="your-key-here"
```

## Troubleshooting

### Configuration Not Found

If you see "Embedding configuration required but not found":

1. Run `fastskill init` to create `skill-project.toml`
2. Add `[tool.fastskill.embedding]` section with embedding configuration
3. Set `OPENAI_API_KEY` environment variable

### API Key Issues

```bash
export OPENAI_API_KEY="your-openai-api-key-here"
```

For persistent setup, add this to your shell profile (`.bashrc`, `.zshrc`, etc.).

### Add fails (git or network)

If `fastskill add` fails with git or network errors:

1. Verify the Git URL is accessible and public (or configured with auth)
2. Check network connectivity and proxy settings
3. For private repos, ensure credentials are configured
4. Use `fastskill add --verbose` for detailed error messages

### Search returns no results

If `fastskill search` returns no results:

1. Run `fastskill reindex` to rebuild the search index
2. Verify `OPENAI_API_KEY` is set and valid
3. Check embedding configuration in `[tool.fastskill.embedding]`
4. Ensure skills are installed (`fastskill show`)

## Documentation

- [Registry Setup](docs/REGISTRY.md) - Detailed registry configuration and management
- [Kubernetes Deployment](integration/kubernetes-deployment) - Production deployment guide
- [Security Policy](SECURITY.md) - Security guidelines and vulnerability reporting
- [GitHub Releases](https://github.com/gofastskill/fastskill/releases) - Latest versions and changelog

## License

Apache-2.0
# CI Test
