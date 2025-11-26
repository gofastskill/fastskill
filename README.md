# FastSkill

Package manager and operational toolkit for Claude Code-compatible skills. FastSkill enables discovery, installation, versioning, and deployment of skills at scale.

[![Python/Rust package build status](https://github.com/gofastskill/fastskill/actions/workflows/release.yml/badge.svg)](https://github.com/gofastskill/fastskill/actions/workflows/release.yml)

## What is FastSkill?

FastSkill is a skill package manager and operational toolkit for the AI agent ecosystem. It builds on Anthropic's standardized Skills format, adding registry services, semantic search, version management, and deployment tooling.

Skills are recipes that extend AI Agent capabilities with specialized procedures, tool integrations, and domain knowledge. FastSkill provides the infrastructure to develop, manage, consume, and deploy skills at scale.

## Key Capabilities

- **Package Management**: Install, update, and remove skills from multiple sources (Git, local, ZIP)
- **Semantic Search**: Find skills using OpenAI embeddings and natural language queries
- **Registry Services**: Publish, version, and distribute skills via registry
- **Manifest System**: Declarative dependency management with lock files for reproducible installations
- **HTTP API**: RESTful service layer for agent integration
- **Web UI**: Browse and manage skills through web interface

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

**Quick Install**

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
- Downloads the latest version (or specify a version: `./install.sh v0.7.8`)
- Installs to `/usr/local/bin` (or `~/.local/bin` if sudo is unavailable)
- Verifies the installation

**Options:**

- `--user`: Install to `~/.local/bin` instead of system directory
- `--prefix DIR`: Install to a custom directory
- `--force`: Overwrite existing installation
- `--help`: Show all available options

**Homebrew (Linux)**

Install FastSkill via [Homebrew](https://brew.sh/) on Linux:

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

**Linux:**

```bash
VERSION="0.6.8"  # Replace with latest version
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
cd fastskill/tools/fastskill/rust
cargo install --path .
```

**Requirements**: Rust 1.70+ (for source builds), OpenAI API key for embedding features

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

Create `.fastskill.yaml` in your project root:

```yaml
embedding:
  openai_base_url: "https://api.openai.com/v1"
  embedding_model: "text-embedding-3-small"
```

Set your OpenAI API key:

```bash
export OPENAI_API_KEY="your-key-here"
```

### 2. Add Skills

```bash
# Add skill from git URL
fastskill add https://github.com/org/skill.git

# Add skill from local folder
fastskill add ./local-skill

# Add skill in editable mode (for local development)
fastskill add ./local-skill -e
```

### 3. Index and Search

```bash
# Index skills for semantic search
fastskill reindex

# Search for skills
fastskill search "powerpoint presentation"
fastskill search "data processing" --limit 5
```

## Essential Commands

### Skill Management

```bash
fastskill add <source>              # Add skill (git URL, local path, or ZIP)
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
fastskill proxy                      # Start Claude API proxy server
```

### Skill Authoring

```bash
fastskill init                       # Initialize skill metadata
fastskill package                    # Package skills into ZIP artifacts
fastskill package --force --recursive # Package all skills recursively from nested directories
```

## Repositories

FastSkill provides a unified repository system for managing all skill storage locations. Repositories can be:

- **Public registries** (crates.io-style with Git index + S3 storage)
- **Private registries** (enterprise/internal registries)
- **Git repositories** (with marketplace.json for skill discovery)
- **ZIP URL sources** (static hosting with marketplace.json)
- **Local folders** (for development)

All repository types are configured in `.claude/repositories.toml` (or automatically loaded from `sources.toml` and `registries.toml` for backward compatibility).

For detailed repository setup, usage, and management instructions, see [docs/REGISTRY.md](docs/REGISTRY.md).

## Configuration

Create `.fastskill.yaml` in your project root:

```yaml
embedding:
  openai_base_url: "https://api.openai.com/v1"
  embedding_model: "text-embedding-3-small"

# Optional: Custom skills directory
skills_directory: ".claude/skills"
```

The CLI resolves the skills directory using this priority:

1. `skills_directory` from `.fastskill.yaml`
2. Walk up directory tree to find existing `.claude/skills/`
3. Default to `.claude/skills/` in current directory (doesn't auto-create)

## Troubleshooting

### Configuration Not Found

If you see "Embedding configuration required but not found":

1. Create `.fastskill.yaml` with embedding configuration (see Quick Start section)
2. Set `OPENAI_API_KEY` environment variable

**Note**: The error message may mention `fastskill init`, but that command is for skill authors only. For project setup, manually create `.fastskill.yaml` as shown in Quick Start.

### API Key Issues

```bash
export OPENAI_API_KEY="your-openai-api-key-here"
```

For persistent setup, add this to your shell profile (`.bashrc`, `.zshrc`, etc.).

## License

Apache-2.0
