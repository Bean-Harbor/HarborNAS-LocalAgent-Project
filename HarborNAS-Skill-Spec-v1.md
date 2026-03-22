# HarborNAS Skill Specification v1

## Purpose
This document defines the standard contract for Skills so teams can build, test, and run skills consistently with HarborOS control-plane-first execution. For HarborOS-domain capabilities, execution should bind to `middleware` API first, then `midcli` as fallback.

## Design Principles

1. HarborOS control-plane first: use `middleware` API for first-party operations.
2. CLI fallback: use `midcli` when API route is unavailable.
3. HarborBeacon IM integration: skills are invokable via HarborBeacon IM channels (Feishu, WeCom, Telegram, Discord, DingTalk, Slack, MQTT) through the MCP adapter; skill manifests are auto-converted to MCP tool descriptions.
4. Deterministic I/O: strict input and output schema.
5. Safe by default: explicit permissions and risk metadata; autonomy levels (ReadOnly/Supervised/Full) align with risk.
6. Observable execution: structured logs and trace IDs.
7. Versioned compatibility: semantic versioning with rollback.

## Skill Package Layout

```text
skills/
  <skill-id>/
    skill.yaml
    handler.py (or executable)
    tests/
      contract_test.json
      smoke_test.sh
```

## skill.yaml Schema

```yaml
id: media.video_edit
name: Video Editing Skill
version: 1.0.0
summary: Edit videos using ffmpeg templates
owner: harbor-team

capabilities:
  - video.trim
  - video.concat
  - video.subtitle

executors:
  cli:
    enabled: true
    command: "python handler.py"
  browser:
    enabled: false
  mcp:
    enabled: false

permissions:
  fs_read:
    - "/data/media/**"
  fs_write:
    - "/data/output/**"
  network: false
  process_spawn: true

risk:
  default_level: MEDIUM
  requires_confirmation:
    - HIGH
    - CRITICAL

input_schema:
  type: object
  required: [action, input]
  properties:
    action:
      type: string
      enum: [trim, concat, subtitle]
    input:
      type: object

output_schema:
  type: object
  required: [ok, result, artifacts]
  properties:
    ok: { type: boolean }
    result: { type: object }
    artifacts:
      type: array
      items: { type: string }

timeouts:
  plan_ms: 2000
  exec_ms: 120000

retries:
  max_attempts: 2
  backoff_ms: 1000
```

## Runtime Contract

### Request Envelope

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "skill_id": "media.video_edit",
  "skill_version": "1.0.0",
  "executor": "cli",
  "risk_level": "MEDIUM",
  "dry_run": false,
  "input": {
    "action": "trim",
    "input": {
      "source": "/data/media/a.mp4",
      "start": "00:00:05",
      "end": "00:00:15"
    }
  }
}
```

### Response Envelope

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "ok": true,
  "executor_used": "cli",
  "exit_code": 0,
  "result": {
    "message": "trim completed"
  },
  "artifacts": ["/data/output/a_trimmed.mp4"],
  "metrics": {
    "duration_ms": 842,
    "retries": 0
  },
  "error": null
}
```

## Routing and Fallback Rules

1. Router must attempt API first if `executors.api.enabled=true`.
2. For HarborOS-domain skills, router should bind API execution to `middleware` before any CLI route.
3. CLI route should bind to `midcli` before any generic shell route.
4. Browser route allowed only when API and CLI are unavailable for that capability.
5. MCP route allowed only when API, CLI, and Browser are unavailable.
6. If risk level is HIGH/CRITICAL, execution requires explicit approval token.

## HarborOS API Binding (middleware)

For skills operating HarborOS services/resources, add a `harbor_api` block in `skill.yaml`:

```yaml
harbor_api:
  enabled: true
  provider: middleware
  endpoint_group: service
  allowed_methods: [query, start, stop, restart, update]
  min_version: "v1"
```

Rules:
- `provider` must be `middleware` for HarborOS first-party operations.
- `allowed_methods` must be explicit and least-privilege.
- Request and response payloads must pass schema validation before and after execution.
- API adapter must normalize endpoint-specific fields into common output schema.

## HarborOS CLI Binding (midcli)

For skills operating HarborOS services/resources, add a `harbor_cli` block in `skill.yaml`:

```yaml
harbor_cli:
  enabled: true
  tool: midcli
  command_group: service
  allowed_subcommands: [status, start, stop, restart]
  require_structured_output: true
```

Rules:
- `tool` must be `midcli` for HarborOS first-party operations.
- `allowed_subcommands` must be explicit and least-privilege.
- `require_structured_output=true` when structured output mode is available.
- Free-form shell strings are disallowed when `harbor_cli.enabled=true`.
- CLI route must not bypass API route when `harbor_api.enabled=true` and API is available.

## Risk Levels

- LOW: read-only operations.
- MEDIUM: reversible write operations.
- HIGH: potentially destructive operations.
- CRITICAL: irreversible or security-sensitive operations.

## Security Controls

1. Command policy:
- allowlist templates + argument validation.
- deny dangerous patterns (`rm -rf /`, shell injection patterns).

2. Sandbox:
- isolated working directory.
- restricted env vars.
- resource limits (CPU/mem/time).

3. Audit:
- record requested action, resolved endpoint/command, executor, user, and outcome.
- retain traceability with `task_id` and `trace_id`.

## Testing Requirements

1. Contract test:
- schema validation for input/output.

2. Dry-run test:
- verify preview mode for risky commands.

3. Smoke test:
- minimal successful execution path.

4. Failure test:
- invalid args, timeout, non-zero exit code.

5. Compatibility test:
- validate API schema and fallback behavior across HarborNAS and upstream versions.

A skill is release-ready only if all tests pass.

## Versioning and Compatibility

- Patch: bugfixes without schema changes.
- Minor: backward-compatible capability additions.
- Major: breaking changes in schema or behavior.

Registry should keep at least one rollback version.

## Minimum Built-in Skills (V2)

1. `system.harbor_ops` - service status/start/stop/restart (API via `middleware`, fallback CLI via `midcli`).
2. `files.batch_ops` - copy/move/archive/search (CLI).
3. `media.video_edit` - trim/concat/subtitle (CLI via ffmpeg).
4. `browser.web_automate` - browser fallback automation.
5. `planner.task_decompose` - task to step plan generation.

## Implementation Checklist

- [ ] skill.yaml validated by schema.
- [ ] API execution path implemented for HarborOS skills.
- [ ] CLI execution path implemented.
- [ ] permission and risk metadata configured.
- [ ] contract and smoke tests added.
- [ ] compatibility matrix checks pass.
- [ ] audit fields present in response.
- [ ] registry entry published.
