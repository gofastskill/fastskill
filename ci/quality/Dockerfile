# Ephemeral image for the nightly quality tier (spec 005, P4).
#
# Mandatory per spec 005 Q8, not merely prudent: the doc-walk suite this image runs
# executes commands *verbatim* out of the shipped docs, and the docs contain `curl |
# bash`, `sudo`, and `~/.claude*` writes (those specific patterns are pre-filtered by
# the extractor and never run — see specs/005, "Extractor rules" — but the container
# is still the blast-radius boundary in case that filter is ever wrong). $HOME is
# thrown away with the container on every run; nothing here is expected to persist.
#
# fastskill is built from HEAD *inside* this image every run (spec 005 Q1) so the
# tier always tests the binary the branch under test actually produces, not a stale
# release. rust-toolchain.toml pins the nightly channel; rustup (bundled in the base
# image) resolves and installs it automatically from that file.

# ---- build stage: compile the fastskill binary ----
FROM rust:bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --bin fastskill

# ---- runtime stage: fastskill binary + Python quality harness ----
# Stdlib-only Python (see ci/quality/error_quality/judge.py) so no pip install is
# needed here; if a future suite needs third-party packages it must vendor or pip
# install them explicitly in its own directory, not assume this image has network
# access to PyPI at run time (it does, over the Tailnet's default route, but the
# quality tier itself should not depend on that).
FROM python:3.12-slim-bookworm AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/fastskill /usr/local/bin/fastskill
COPY ci/quality /app/ci/quality
# The doc-walk suite executes the commands in these pages verbatim (ci/quality/docwalk/docs.json
# resolves them relative to /app). Without this copy the suite dies on FileNotFoundError and the
# whole tier is reported as INFRA FAILURE.
COPY webdocs /app/webdocs
RUN chmod +x /app/ci/quality/entrypoint.sh

# Non-root, throwaway $HOME. Doc-walk cases that write to ~/.claude* land here and
# vanish with the container; nothing under /app is meant to survive between runs
# either, only what the entrypoint copies out to the bind-mounted /out.
RUN useradd --create-home --home-dir /home/quality --shell /bin/bash quality
ENV HOME=/home/quality
USER quality
WORKDIR /app

# LLM_GATEWAY_URL / LLM_GATEWAY_KEY / LLM_GATEWAY_MODEL are injected at `docker run`
# time from repo secrets (see .github/workflows/quality.yml) — never baked in here.
ENTRYPOINT ["/app/ci/quality/entrypoint.sh"]
