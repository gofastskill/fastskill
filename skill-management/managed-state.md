# Managed state

FastSkill 0.9.276

Source: https://docs.gofastskill.com/skill-management/managed-state

Release revision: e9817985823c0f9bea00f7791f51c011bc9afb70

Documentation revision: e9817985823c0f9bea00f7791f51c011bc9afb70



# Managed state

A **managed state** is a signed, resolved list of the skills one user's agents on one machine
should have. Something outside FastSkill decides it; FastSkill verifies it, applies it and never
decides it. This page is the format reference. The commands are under
[`managed`](/cli-reference/managed-command), and what an administrator pins is in
[Hardening managed machines](/security/managed-machines).

The envelope, the payload and the [report](/cli-reference/managed-command#the-report) are a
compatibility surface, like the [reconciliation statuses](/skill-management/reconciliation): each
carries a format version, and within a version fields are only added, never changed or removed.

## The envelope

A state travels in a [DSSE](https://github.com/secure-systems-lab/dsse) envelope: one JSON object.

```json
{
  "payloadType": "application/vnd.fastskill.managed-state+json",
  "payload": "eyJmb3JtYXRfdmVyc2lvbiI6MSwgLi4ufQ",
  "signatures": [
    { "keyid": "k1", "sig": "base64 Ed25519 signature" }
  ]
}
```

* `payloadType` is exactly `application/vnd.fastskill.managed-state+json`.
* `payload` is the payload's exact bytes in base64: standard or URL-safe alphabet, padded or not.
* Each signature is Ed25519 over DSSE's pre-authentication encoding of the payload type and the
  payload bytes, `DSSEv1 <length of type> <type> <length of payload> <payload>`, with the lengths
  in ASCII decimal. No JSON is canonicalized: the signer signs the bytes it puts in `payload`.
* `keyid` names a pinned key. FastSkill accepts the envelope when at least one signature names a
  pinned key id and verifies against that key. Signatures that name no pinned key are ignored.
* An envelope larger than 8 MiB is refused.

Keys are pinned in [trusted settings](/cli-reference/managed-command#settings) only; they are
never fetched from the source. Several keys can be pinned at once, so pin a new key before the
source starts signing with it, and unpin a key to revoke it. FastSkill verifies the cached state
every time it uses it, so a state signed only by an unpinned key stops counting at once.

## The payload

The payload is a JSON object. Its `format_version` is `1`; a state with any other value is refused
with a message to update FastSkill, before anything else is read.

| Field               | Required | Meaning                                                                                                                                                     |
| ------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `format_version`    | yes      | `1`.                                                                                                                                                        |
| `issued_at`         | yes      | When the state was issued, RFC 3339.                                                                                                                        |
| `expires_at`        | yes      | When it expires, RFC 3339.                                                                                                                                  |
| `source`            | yes      | The managed source it was issued for: the configured `source`, exactly.                                                                                     |
| `subject`           | yes      | An opaque label. FastSkill displays it and compares it for equality, never interprets it.                                                                   |
| `skills`            | no       | The skills to install. Each entry has an `id`, a `digest` and an `artifact`.                                                                                |
| `allowed`           | yes      | `listed`: only digests in `skills` or `also_allowed` may be installed. `any`: anything that isn't blocked.                                                  |
| `also_allowed`      | no       | Digests that may be installed but that the state doesn't install.                                                                                           |
| `blocked`           | no       | Digests that must not be present, each with an optional `message` for the user.                                                                             |
| `exclusive_targets` | no       | With `allowed = listed`, `true` makes apply quarantine content in an agent folder whose digest isn't allowed, instead of leaving it alone. Default `false`. |
| `report_url`        | no       | Where each apply sends its report.                                                                                                                          |
| `editable`          | no       | `blocked-only` (default) or `refused`: whether editable local skills skip the allow-list.                                                                   |
| `request_url`       | no       | A link template with one `{digest}`, shown when an install is refused.                                                                                      |

`report_url`, `editable` and `request_url` take effect only with an `https://` source; see
[the fields only an `https://` source honors](/cli-reference/managed-command#fields-only-an-https-source-honors).

A `skills` entry has:

* `id`: the skill's id, one safe path component. It must equal the identity of the content the
  archive holds.
* `digest`: the skill's content digest in the current `sha256-tree-v2:` form. It must equal the
  digest of the extracted archive.
* `artifact`: where the skill's ZIP archive is. An `https://` URL; for a file source, also a path
  next to the state or an absolute path.

Entries are resolved content, like a Lock, not intent, like a Manifest. Applying a state never
resolves versions, reads repositories or runs dependency resolution: `managed apply` installs
exactly `skills`. A skill whose declared dependencies aren't in the state is still installed, and
apply reports the missing dependencies.

### Example

```json
{
  "format_version": 1,
  "issued_at": "2026-10-10T12:00:00Z",
  "expires_at": "2026-10-17T12:00:00Z",
  "source": "https://skills.example.com/state",
  "subject": "team-a",
  "skills": [
    {
      "id": "code-review",
      "digest": "sha256-tree-v2:3f0c2b7d9e41a6c8d5f0b2e7c1a94d6f8e3b5a7c9d1e2f4a6b8c0d2e4f6a8b1c",
      "artifact": "https://skills.example.com/archives/code-review-3f0c2b7d.zip"
    }
  ],
  "allowed": "any",
  "also_allowed": [],
  "blocked": [
    {
      "digest": "sha256-tree-v2:9a1e4c7b2d5f8a0c3e6b9d1f4a7c0e2b5d8f1a3c6e9b2d4f7a0c3e5b8d1f4a6c",
      "message": "Withdrawn on 2026-10-09. Install code-review instead."
    }
  ],
  "exclusive_targets": false,
  "report_url": "https://skills.example.com/reports",
  "request_url": "https://skills.example.com/request?digest={digest}"
}
```

### What FastSkill refuses

The whole state is refused when any of these is true:

* its format version isn't `1`
* a digest is in `blocked` and also in `skills` or `also_allowed`
* two `skills` entries share an id
* an id isn't a safe skill id: one path component, under the same rules as any skill id
* a digest isn't in the current content digest form; the older bare-hex form is never accepted
  here, because two different trees could share it
* an `artifact`, `report_url` or `request_url` isn't `https://` (a file source may use local
  paths for artifacts)
* `editable` has a value other than `blocked-only` or `refused`

When it installs a skill, apply also requires that the downloaded content's digest equals the
entry's `digest` and that the content's own identity equals the entry's `id`.

### Bound to its source and subject, and never backwards

* **Source.** `source` must equal the configured managed source. A state issued for another source
  is refused, even when a pinned key signed it.
* **Subject.** FastSkill records the `subject` of the first state it accepts after enrollment.
  Later states with a different subject are refused until `fastskill managed enroll` runs again,
  so a state issued for one user can't be swapped in for another's.
* **No rollback.** FastSkill records the highest `issued_at` it has accepted and refuses any state
  issued earlier. Receiving the same state again is fine.
* **Clock skew.** A state whose `issued_at` is more than five minutes ahead of the local clock is
  refused.

### Expiry

FastSkill keeps the last valid state and uses it offline until it expires. After expiry, commands
that add or change skill content are refused, and `managed apply` adds, changes and removes
nothing, though blocked content is still quarantined. Nothing is removed because of expiry alone,
and expiry is checked against the local clock. The next accepted state ends the condition.

## Producing a state

FastSkill ships no signer and serves no managed state. Any tool that writes JSON and signs with
an Ed25519 key can produce one:

1. Resolve the skills: for each, its id, its content digest, and a ZIP archive of its folder at
   that digest, placed where `artifact` will point.
2. Write the payload with `issued_at` now, an `expires_at` that leaves machines time to fetch the
   next state, and the `source` the machines are configured with.
3. Sign the pre-authentication encoding of the payload type and the payload bytes with the private
   key, and wrap the payload and the signature in the envelope under the key's pinned id.
4. Publish the envelope at the source, next to the archives for a file source, and pin the public
   key in the machines' trusted settings before any machine fetches it.

A signed state in a git repository or a storage bucket is a complete managed source. Nothing
about it depends on the transport.

