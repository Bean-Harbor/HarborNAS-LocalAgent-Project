# HarborOS Read-Only NAS Files Substrate Pack

## Purpose

This pack defines the smallest HarborOS System Domain file substrate needed to
support later framework-owned multimodal RAG work without letting HarborOS
become the knowledge owner.

The scope here is strictly NAS substrate:

- `files.list`
- `files.stat`
- `files.read_text`

These are helper primitives for inspecting NAS-backed files. They are not a
search, ranking, embedding, retrieval, or answer-generation layer.

For the real HarborOS / TrueNAS interface parity note, see
[`docs/harboros-real-integration-parity-note.md`](harboros-real-integration-parity-note.md).

## Boundary

HarborOS owns the substrate only:

- preview routing inside the HarborOS executor chain
- real live parity only where the connector table marks a capability as `real`
- `midcli` fallback shape for file inspection previews
- workspace and path guardrails
- large-read protection for text reads
- audit-friendly execution metadata

HarborOS does not own:

- query parsing
- ranking
- chunking strategy
- retrieval policy
- multimodal RAG business semantics
- device-native control
- IM transport or delivery semantics

## Safety Rules

- read-only calls must stay under the allowlisted roots
- write destinations are rejected for read-only primitives
- `read_text` is capped by `max_bytes`
- oversized reads fail fast instead of turning into a broad file export path
- no read-only primitive may smuggle copy/move/archive behavior into this pack

## Supported Inputs

The substrate intentionally exposes only a narrow file surface:

- `files.list`
  - `resource.path`, `resource.paths[0]`, `resource.source`, or `resource.src`
  - optional `args.recursive`
- `files.stat`
  - `resource.path`, `resource.paths[0]`, `resource.source`, or `resource.src`
- `files.read_text`
  - `resource.path`, `resource.paths[0]`, `resource.source`, or `resource.src`
  - optional `args.max_bytes`

Text reads are intended for small document bodies, notes, markdown, json, csv,
yaml, log, and similar plain-text NAS artifacts. This pack does not promise OCR,
embedding, semantic ranking, or response synthesis.

## Failure Modes

These are the expected, stable failures:

- path outside the read allowlist
- path under a denied root
- read request includes a destination field
- `read_text` exceeds the configured byte cap
- unsupported `files.*` operation
- malformed or missing path fields
- missing live parity for scaffold-only helpers

## Example Shapes

```json
{
  "domain": "files",
  "operation": "list",
  "resource": {
    "path": "/mnt/library",
    "recursive": true
  },
  "args": {}
}
```

```json
{
  "domain": "files",
  "operation": "stat",
  "resource": {
    "source": "/mnt/library/brief.txt"
  },
  "args": {}
}
```

```json
{
  "domain": "files",
  "operation": "read_text",
  "resource": {
    "path": "/mnt/library/brief.txt"
  },
  "args": {
    "max_bytes": 4096
  }
}
```

Framework callers should treat these failures as substrate-level guardrails, not
as retrieval signals.

## Preview Consumption Pattern

Framework-owned retrieval can use this substrate in a preview-only flow:

1. list candidate paths under an allowlisted root
2. stat the selected file to confirm size and type
3. read a bounded text slice when the artifact is plain text
4. let the framework handle chunking, ranking, citations, and answer writing

HarborOS only guarantees the substrate response shape and guardrails. It does
not choose ranked results or generate citations.

Preview responses carry a stable substrate shape:

- `method` identifies the underlying filesystem primitive
- `context.preview_kind` says whether this is `list`, `stat`, or `read_text`
- `context.normalized_path` is the canonical path after normalization
- `context.read_only` is always `true` for this pack
- `context.max_bytes` appears only for `read_text`

Live parity note:

- `files.list` is part of the real HarborOS parity table today
- `files.stat` and `files.read_text` remain scaffold-only until the connector
  parity table says otherwise

## Smoke Pack

Run these tests to review the boundary:

```bash
cargo test readonly_files_are_supported_by_harbor_executors
cargo test readonly_list_rejects_destination_fields
cargo test readonly_read_text_caps_max_bytes_and_stays_read_only
cargo test readonly_read_text_rejects_out_of_bounds_requests
cargo test readonly_files_normalize_paths_before_execution
cargo test harbor_files_read_only_actions_stay_on_middleware_then_midcli
cargo test planner_keeps_read_only_files_on_harboros_routes
```

Expected result:

- read-only file actions still prefer `Middleware API -> MidCLI`
- Browser and MCP stay fallback-only for non-system domains
- device-native domains are still not claimed by HarborOS executors
- path guardrails reject dangerous write-shaped payloads

## Canary Notes

If canary traffic starts using `browser` or `mcp` for ordinary NAS file
inspection, the HarborOS system-domain boundary has drifted.

If read-only requests begin carrying destination or overwrite fields, pause and
trace the caller back before widening the substrate.

If `read_text` needs larger byte budgets, raise the limit intentionally in one
place and keep the cap explicit. Do not convert this pack into a general file
export service.

## Operator / Config Note

- Treat `HARBOR_KNOWLEDGE_ROOTS` as the upstream framework's allowlisted
  knowledge root list when it is provided by the operator or service wrapper.
- Keep HarborOS read-only calls inside `/mnt` or `/data` unless the framework
  explicitly broadens the allowlist for a controlled environment.
- Preflight should confirm the effective roots before canary, then run one
  `list`, one `stat`, and one bounded `read_text` preview against a known-safe
  root.
- If the demo fails, classify it before touching retrieval logic:
  - root or path failures show up as allowlist/denied-path errors
  - preview-shape failures show up as missing `preview_kind` or `normalized_path`
  - oversize failures show up as `read_text max_bytes` errors
  - ranking or answer differences belong to framework-owned retrieval, not HarborOS
- If canary traffic starts pulling from a wrong root, rollback should remove the
  new read-only file operation from the caller before widening HarborOS.
- Rollback should restore the prior preview path, not turn this substrate into a
  retrieval service or a general export path.

## Handoff Checklist

When a retrieval demo fails, hand off in this order:

1. Check the effective roots first.
2. Run the preview smoke against one known-safe path.
3. Inspect the substrate response shape.
4. Only then escalate to framework retrieval logic.

Classify the failure before changing code:

- allowlist or path rejection means the root config is wrong
- missing `preview_kind` or `normalized_path` means the preview substrate is
  malformed
- `read_text max_bytes` errors mean the preview budget is too small or the
  caller is oversized
- a wrong citation, ranking, or final answer belongs to framework retrieval,
  not HarborOS
