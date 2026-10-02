# Registry credentials: bearer tokens and credential commands

Status: accepted. Date: 2026-10-02. Implemented: 2026-10-02.

Related: [ADR-0005](0005-install-seam-and-origin-model.md) and
[ADR-0016](0016-machines-follow-a-signed-managed-state.md).

## Context

An `http-registry` repository could only be authenticated with `pat`: a token read from an
environment variable and sent as `Authorization: token <value>`. If the variable was unset, the
request went out without credentials. That works for git hosts. It does not work for a registry
that requires a standard OAuth bearer token, and it does not work when the token is short-lived
and has to be obtained from an identity provider CLI or from a CI job's token exchange right
before the request.

ADR-0016 already defines how FastSkill runs a credential command for a managed source and where
that token may go (decisions 6 and 7). Registries need the same, so this ADR reuses those rules
instead of inventing new ones.

## Decision

1. **Two new auth types for `http-registry`.** `bearer` and `command` join `pat`. Both send
   `Authorization: Bearer <token>`. They are valid only on `http-registry` repositories, like
   `pat` is today on the types that accept auth.

   ```toml
   # skill-project.toml or the user repositories.toml
   auth = { type = "bearer", env_var = "REGISTRY_TOKEN" }

   # the user repositories.toml only
   auth = { type = "command", command = ["my-login", "token"] }
   ```

2. **`bearer` reads an environment variable.** `env_var` is required. The variable is read when a
   request is made. It is allowed in a project's `skill-project.toml` and in the user's
   `repositories.toml`, because it names a variable, not a secret.

3. **`command` runs a program, following ADR-0016 decision 6.** `command` is an executable and its
   arguments. It runs directly without a shell, from FastSkill's configuration directory, with a
   30-second timeout after which it is killed. The token is the first line of standard output with
   trailing CR/LF removed. Standard output may be at most 16 KiB, and an empty token is an error.
   Standard error goes to the terminal only when it is one, and is otherwise discarded. When
   standard input or standard error is not a terminal, FastSkill sets `FASTSKILL_INTERACTIVE=0`
   so the command knows it must not prompt.

4. **A command runs at most once per process for each repository.** The token is kept in memory
   only, in a type whose debug output is redacted and which has no display form. FastSkill never
   writes it to disk, logs or error messages. A failed run is not cached, so a later request in the
   same process tries again.

5. **A command may be configured only in user configuration, following ADR-0016 decision 7.** The
   user file is `repositories.toml` in FastSkill's configuration directory
   (`$XDG_CONFIG_HOME/fastskill/` when set, else the platform config directory). A project file
   that names a command, in any form, is refused with an error naming the repository, so cloning a
   repository can never make FastSkill run a program. `fastskill repo add --auth-type command`
   requires `--user`. A user entry replaces a project entry of the same name, and both are used for
   listing, resolution and install.

6. **The token goes only to the registry's origin.** The origin (scheme, host and port) of
   `index_url` is the only one that receives it. Downloads on that origin get it; downloads on any
   other origin get no token and must carry their own authorization, such as a pre-signed URL.
   FastSkill follows redirects itself: a redirect to another origin is followed without the token,
   a redirect from `https` to `http` or to any non-HTTP scheme is refused, and more than ten
   redirects is an error.

7. **Missing credentials fail the request.** An unset or empty `bearer` variable and a command that
   cannot start, times out, exits non-zero, prints nothing or prints too much are errors. They
   never fall back to an unauthenticated request. Errors name the repository, the variable or the
   program and its exit status, never the token.

8. **One place builds the header.** Index fetches, downloads, catalog listing and install all go
   through the registry client's single request path, so the scoping rules above apply to every
   request.

9. **`pat` is unchanged.** It keeps its `token` scheme, its existing redirect handling and its
   behaviour of sending no header when its variable is unset.

10. **Listing shows the type, never the token.** `repo list` shows the auth type; `repo info`
    shows the type with the variable name or the program name. Neither ever shows a token or a
    command's arguments in table output.

## Consequences

- A registry that requires a bearer token works with a plain environment variable, and a registry
  with short-lived tokens works with any CLI that can print one.
- Projects can declare that a registry needs a bearer token without being able to run code on the
  machines that use them. Users who need a credential command configure it once, per machine.
- A credential command that needs to prompt can do so in a terminal. In CI, hooks and other
  non-interactive runs it sees `FASTSKILL_INTERACTIVE=0` and must print a token without prompting
  or fail.
