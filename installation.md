# Install FastSkill

FastSkill 0.9.229

Source: https://docs.gofastskill.com/installation

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



Choose one method, then run `fastskill -V`. The version must match the release
shown on this page. If a package manager is behind, use the release download.
Git sources require system Git. Installing and reading local skills needs no API key.

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

| Platform                  | Archive                                      |
| ------------------------- | -------------------------------------------- |
| Linux x86-64, static musl | `fastskill-x86_64-unknown-linux-musl.tar.gz` |
| Linux x86-64, glibc       | `fastskill-x86_64-unknown-linux-gnu.tar.gz`  |
| macOS Apple Silicon       | `fastskill-aarch64-apple-darwin.tar.gz`      |
| macOS Intel               | `fastskill-x86_64-apple-darwin.tar.gz`       |
| Windows x86-64            | `fastskill-x86_64-pc-windows-msvc.zip`       |

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

