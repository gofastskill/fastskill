#!/usr/bin/env python3
"""Deterministic extractor for the blind doc-walk harness (spec 005, P3).

Pulls fenced code blocks out of a webdocs `.mdx` file **in order, verbatim** — this
module never decides what a block means beyond mechanical classification (command /
file body / prose) and never edits a block's content. The LLM only shows up later, in
`classify.py`, and only to compare a doc's claimed output against what actually ran.

Stdlib only.

Known, deliberate scope limits (see spec 005 section 2 and the module docstring in
`run_docwalk.py` for the full design rationale):

- Only blocks written as fenced code (``` ... ```) are considered "extractable". A
  fence can be indented 2-4 spaces (list items) — a column-0-only scanner misses those
  (see `webdocs/quickstart.mdx`, the "Add standard-compliant skills" bullet).
- Three block kinds: "command" (bash/sh/shell/zsh/console — a runnable candidate),
  "file" (toml/json/yaml/yml — an input written to disk, never executed), and "prose"
  (anything else, including untagged fences — never executed, may serve as an
  expected-output anchor for the preceding command).
- A command block is pre-filtered and flagged "human-review" (never run) if it:
  contains a `$ `-prefixed line (illustrative prompt+output), contains a placeholder
  (`<...>`, `/path/to`, `your-*`), or contains a network installer
  (`curl ... | bash`, `sudo`, `brew/scoop/cargo install`) — banned even in a sandbox.
- Prose-only *sections* (a heading with zero code blocks anywhere under it) are
  reported too, separately from block-level flags — see `find_prose_sections`. This is
  what "no silent caps" means in practice: a doc step that never had code to extract
  must still show up in the report, not vanish.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

COMMAND_LANGS = {"bash", "sh", "shell", "zsh", "console", "shell-session"}
FILE_LANGS = {"toml", "json", "yaml", "yml"}

# Placeholder shapes seen in the real webdocs census (spec 005): angle brackets
# (`<skill-id>`), `/path/to/...`, and the `your-org` / `your-token` / `your-key`
# family. The `your-[a-z-]+` half is intentionally broader than the two examples
# named in the spec — `your-key-here` and `your-gateway-key` are the same shape and
# were found in configuration/init-command.mdx during this build.
PLACEHOLDER_RE = re.compile(r"<[a-zA-Z][\w.-]*>|/path/to\b|\byour-[a-z][a-z-]*\b")

# Network installers stay banned even inside the sandbox container (spec 005, Q8):
# a fresh container is still a real machine with real network egress.
NETWORK_INSTALL_RE = re.compile(
    r"curl[^\n]*\|\s*(bash|sh)\b|\bsudo\b|\b(brew|scoop|cargo)\s+install\b",
    re.IGNORECASE,
)

DOLLAR_PREFIX_RE = re.compile(r"^\$ ")

OUTPUT_COMMENT_RE = re.compile(r"^\s*#\s*output:?\s*(.*)$", re.IGNORECASE)

# "you'll see...", "you should see...", "you should now see..." — prose claims about
# output that live outside any fence (often inside a <Callout>).
PROSE_HINT_RE = re.compile(
    r"you(?:'ll| will| should)(?: now)? see[^.\n]{0,220}[.\n]", re.IGNORECASE
)

# A prose block is only worth pairing as an "expected output" anchor for the
# preceding command if it isn't itself a command/file block in disguise.
PROSE_ANCHOR_LANGS = {"", "text", "output", "console-output", "plaintext", "plain"}


@dataclass
class Anchor:
    kind: str  # "output-comment" | "next-block" | "prose-hint"
    text: str


@dataclass
class Block:
    index: int
    line_start: int
    line_end: int
    lang: str
    filename: str | None
    raw: str
    kind: str  # "command" | "file" | "prose"
    run: bool
    skip_reason: str | None = None
    notes: list[str] = field(default_factory=list)
    anchors: list[Anchor] = field(default_factory=list)
    consumed_as_anchor_for: int | None = None

    # Populated by runner.py for blocks that were actually executed.
    output: str | None = None
    exit_code: int | None = None
    timed_out: bool = False

    @property
    def command_text(self) -> str:
        return self.raw


@dataclass
class ProseSection:
    heading: str
    line: int
    reason: str


@dataclass
class ExtractedDoc:
    path: str
    blocks: list[Block]
    prose_sections: list[ProseSection]


@dataclass
class _RawFence:
    start_line: int
    end_line: int
    indent: str
    info: str
    raw: str


def _parse_fences(text: str) -> list[_RawFence]:
    """Pull every fenced block, in order, tolerating 2-4 space indentation.

    CommonMark's actual closing-fence rule (>= as many backticks, indented no more
    than the opener) is approximated here: we require the same-or-fewer indent and at
    least as many backticks. That is sufficient for the fence shapes real webdocs
    actually use and deliberately does not try to be a full Markdown parser.
    """
    lines = text.splitlines()
    fence_open = re.compile(r"^(?P<indent>[ \t]*)(?P<fence>`{3,})(?P<info>.*)$")
    fences: list[_RawFence] = []
    i, n = 0, len(lines)
    while i < n:
        m = fence_open.match(lines[i])
        if not m:
            i += 1
            continue
        indent, fence_chars, info = m.group("indent"), m.group("fence"), m.group("info").strip()
        start_line = i + 1  # 1-based
        i += 1
        content: list[str] = []
        closed = False
        close_re = re.compile(rf"^[ \t]{{0,{len(indent)}}}`{{{len(fence_chars)},}}\s*$")
        while i < n:
            if close_re.match(lines[i]):
                closed = True
                break
            content.append(lines[i])
            i += 1
        end_line = i + 1 if closed else i  # 1-based; unclosed fence ends at EOF
        dedented = [
            l[len(indent):] if l[: len(indent)] == indent else l.lstrip(" \t")
            for l in content
        ]
        fences.append(_RawFence(start_line, end_line, indent, info, "\n".join(dedented)))
        if closed:
            i += 1  # step past the closing fence line
    return fences


def _classify_lang(info: str) -> tuple[str, str | None]:
    parts = info.split(None, 1)
    lang = parts[0].lower() if parts else ""
    extra = parts[1].strip() if len(parts) > 1 else ""
    return lang, (extra or None)


def _prefilter_command(raw: str) -> str | None:
    """Return a skip reason if this command block must never be run, else None."""
    if any(DOLLAR_PREFIX_RE.match(line.lstrip()) for line in raw.splitlines()):
        return "illustrative prompt+output ($-prefixed line)"
    if PLACEHOLDER_RE.search(raw):
        return "contains a placeholder (<...>, /path/to, or your-*)"
    if NETWORK_INSTALL_RE.search(raw):
        return "network installer / privileged install — banned even in a sandbox"
    return None


def _extract_output_comment_anchors(raw: str) -> list[Anchor]:
    """Pull `# Output: ...` (single- or multi-line) annotations out of a block.

    Both shapes are real (see webdocs/skill-management/reconciliation.mdx):
    `# Output: <one line>` and `# Output:` followed by more `#`-prefixed lines.
    The block still executes with these comments in it — bash ignores `#` lines on
    its own, so nothing needs to be stripped before running.
    """
    lines = raw.splitlines()
    anchors: list[Anchor] = []
    i, n = 0, len(lines)
    while i < n:
        m = OUTPUT_COMMENT_RE.match(lines[i])
        if not m:
            i += 1
            continue
        parts = [m.group(1)] if m.group(1) else []
        i += 1
        while i < n and lines[i].lstrip().startswith("#"):
            parts.append(lines[i].lstrip().lstrip("#").strip())
            i += 1
        text = "\n".join(p for p in parts if p)
        if text:
            anchors.append(Anchor(kind="output-comment", text=text))
    return anchors


def _find_prose_hint_anchors(doc_text: str, blocks: list[Block]) -> None:
    """Attach "you should see..." prose claims to the nearest block, either direction.

    These live outside fences entirely (typically inside a `<Callout>`), so they are
    found by scanning the raw document text, not the fence list. The claim can refer
    either to the example just above it ("...ran fine:\\n```\\n...") or the one just
    below it ("you'll see: ```\\n...```") — validation.mdx uses the latter shape
    ("If validation fails, you'll see specific error messages:" immediately precedes
    the example), so nearest-by-absolute-distance beats nearest-preceding-only.
    """
    for m in PROSE_HINT_RE.finditer(doc_text):
        hint_line = doc_text.count("\n", 0, m.start()) + 1
        best, best_dist = None, None
        for b in blocks:
            if b.line_start <= hint_line <= b.line_end:
                dist = 0
            else:
                dist = min(abs(hint_line - b.line_start), abs(hint_line - b.line_end))
            if best_dist is None or dist < best_dist:
                best, best_dist = b, dist
        if best is not None and best_dist <= 20:
            best.anchors.append(Anchor(kind="prose-hint", text=m.group(0).strip()))


def _find_prose_sections(doc_text: str, blocks: list[Block]) -> list[ProseSection]:
    """Flag headings whose whole section (incl. subsections) has zero code blocks.

    This is the "prose-only step" limit spec 005 calls out explicitly ("open the file
    and check it looks reasonable" doesn't extract into a command and still needs a
    human) — generalized from "one named example" to "every heading section with no
    code at all", found mechanically rather than by hand-picking examples.
    """
    heading_re = re.compile(r"^(#{2,3})[ \t]+(.+?)\s*$", re.MULTILINE)
    headings = [
        (len(m.group(1)), m.group(2).strip(), doc_text.count("\n", 0, m.start()) + 1)
        for m in heading_re.finditer(doc_text)
    ]
    flags: list[ProseSection] = []
    for idx, (level, title, line) in enumerate(headings):
        end_line = len(doc_text.splitlines()) + 1
        for level2, _title2, line2 in headings[idx + 1 :]:
            if level2 <= level:
                end_line = line2
                break
        has_code = any(line < b.line_start < end_line for b in blocks)
        if not has_code:
            flags.append(
                ProseSection(
                    heading=title,
                    line=line,
                    reason="prose-only section, no code block to extract",
                )
            )
    return flags


def extract_doc(path: str) -> ExtractedDoc:
    text = open(path, encoding="utf-8").read()
    fences = _parse_fences(text)

    blocks: list[Block] = []
    for idx, fence in enumerate(fences):
        lang, extra = _classify_lang(fence.info)
        if lang in COMMAND_LANGS:
            kind = "command"
        elif lang in FILE_LANGS:
            kind = "file"
        else:
            kind = "prose"

        block = Block(
            index=idx,
            line_start=fence.start_line,
            line_end=fence.end_line,
            lang=lang,
            filename=extra if kind == "file" else None,
            raw=fence.raw,
            kind=kind,
            run=False,
        )

        if kind == "command":
            reason = _prefilter_command(fence.raw)
            if reason:
                block.skip_reason = reason
            else:
                block.run = True
            block.anchors.extend(_extract_output_comment_anchors(fence.raw))
        elif kind == "file":
            block.skip_reason = "file body (toml/json/yaml) — an input, not a command"
            if NETWORK_INSTALL_RE.search(fence.raw):
                block.notes.append(
                    "contains network-installer text; not executed because it is a "
                    "file body, flagged for visibility only"
                )
        else:  # prose
            block.skip_reason = f"prose/non-command content (lang={lang!r}); needs human review"

        blocks.append(block)

    # Pair a bare prose fence immediately after a command block as its output anchor.
    for i, block in enumerate(blocks):
        if block.kind != "command":
            continue
        if i + 1 >= len(blocks):
            continue
        nxt = blocks[i + 1]
        if (
            nxt.kind == "prose"
            and nxt.lang in PROSE_ANCHOR_LANGS
            and nxt.raw.strip()
            and (nxt.line_start - block.line_end) <= 6
        ):
            block.anchors.append(Anchor(kind="next-block", text=nxt.raw.strip()))
            nxt.consumed_as_anchor_for = block.index
            nxt.skip_reason = f"consumed as expected-output anchor for block #{block.index}"

    _find_prose_hint_anchors(text, blocks)
    prose_sections = _find_prose_sections(text, blocks)

    return ExtractedDoc(path=path, blocks=blocks, prose_sections=prose_sections)
