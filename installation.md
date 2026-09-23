# Install FastSkill

FastSkill 0.9.252

Source: https://docs.gofastskill.com/installation

Release revision: 82f73695400b8f8aa19ead9addd7cb78d8a99e2d

Documentation revision: 82f73695400b8f8aa19ead9addd7cb78d8a99e2d



Choose one method, then run `fastskill -V`. The version must match the release
shown on this page. If a package manager is behind, use the release download.
Git sources require system Git. Installing and reading local skills needs no API key.

## Install script (recommended)

On Linux or macOS:

```bash
curl -fsSL https://github.com/gofastskill/fastskill/releases/latest/download/install.sh | sh
```

On Windows (PowerShell):

```powershell
irm https://github.com/gofastskill/fastskill/releases/latest/download/install.ps1 | iex
```

The script picks the archive for your platform, checks it against the release's
`SHA256SUMS`, installs `fastskill` into `~/.local/bin` (`%USERPROFILE%\.local\bin` on
Windows) and adds that directory to PATH. Open a new shell, then run `fastskill -V`.

| Task                                                                                  | Command                                       |
| ------------------------------------------------------------------------------------- | --------------------------------------------- |
| Update to the newest release                                                          | `fastskill cli self update`                   |
| Check without installing                                                              | `fastskill cli self update --check`           |
| Go back to the previous version                                                       | `fastskill cli self rollback`                 |
| Show install location and state                                                       | `fastskill cli self status`                   |
| Remove it                                                                             | `fastskill cli self uninstall`                |
| Install from a downloaded archive (offline; put the release's `SHA256SUMS` beside it) | `fastskill cli self install --from <archive>` |

Environment variables for the script:

| Variable                     | Effect                                                                 |
| ---------------------------- | ---------------------------------------------------------------------- |
| `FASTSKILL_VERSION`          | `stable` (default), `latest`, or an exact version such as `0.9.240`    |
| `FASTSKILL_INSTALL_DIR`      | Install into this directory instead                                    |
| `FASTSKILL_NO_MODIFY_PATH=1` | Leave shell startup files and PATH untouched                           |
| `FASTSKILL_UNMANAGED=1`      | Plain copy for CI and container images: no PATH change, no self-update |

In GitHub Actions:

```yaml
- name: Install FastSkill
  run: |
    curl -fsSL https://github.com/gofastskill/fastskill/releases/latest/download/install.sh | FASTSKILL_UNMANAGED=1 sh
    echo "$HOME/.local/bin" >> "$GITHUB_PATH"
```

### Update notice

After a successful command in an interactive terminal, FastSkill prints one line on stderr when a newer release exists, with the command that upgrades your install (`fastskill cli self update`, or `brew upgrade fastskill` for a Homebrew install). It checks at most once a day. Set `FASTSKILL_NO_UPDATE_CHECK=1` to turn it off; it is also off when `CI` is set, when stderr is not a terminal, and under `fastskill mcp serve`.

## Homebrew

On macOS or Linux with [Homebrew](https://brew.sh/):

```bash
brew install gofastskill/cli/fastskill
fastskill -V
```

Update an existing installation with `brew upgrade fastskill`.

## Scoop

On Windows with [Scoop](https://scoop.sh/):

```powershell
scoop bucket add gofastskill https://github.com/gofastskill/scoop-bucket
scoop install fastskill
fastskill -V
```

Update with `scoop update fastskill`.

## Release downloads

Download an archive from the [latest release](https://github.com/gofastskill/fastskill/releases/latest),
extract it, and put the binary in a directory on your PATH.

| Platform                  | Archive                                       |
| ------------------------- | --------------------------------------------- |
| Linux x86-64, static musl | `fastskill-x86_64-unknown-linux-musl.tar.gz`  |
| Linux ARM64, static musl  | `fastskill-aarch64-unknown-linux-musl.tar.gz` |
| Linux x86-64, glibc       | `fastskill-x86_64-unknown-linux-gnu.tar.gz`   |
| macOS Apple Silicon       | `fastskill-aarch64-apple-darwin.tar.gz`       |
| macOS Intel               | `fastskill-x86_64-apple-darwin.tar.gz`        |
| Windows x86-64            | `fastskill-x86_64-pc-windows-msvc.zip`        |
| Windows ARM64             | `fastskill-aarch64-pc-windows-msvc.zip`       |

Each release also publishes `SHA256SUMS` with the checksum of every archive.

For example, on Linux x86-64:

```bash
curl -fL https://github.com/gofastskill/fastskill/releases/latest/download/fastskill-x86_64-unknown-linux-musl.tar.gz -o fastskill.tar.gz
tar -xzf fastskill.tar.gz
mkdir -p ~/.local/bin
install -m 755 fastskill ~/.local/bin/fastskill
fastskill -V
```

Your shell must include `~/.local/bin` on PATH. On Windows, extract `fastskill.exe`
and add its directory to PATH. Installation does not configure an embedding provider;
that is optional for [semantic search](/cli-reference/reindex-command).

Continue with [your first skill](/quickstart).

