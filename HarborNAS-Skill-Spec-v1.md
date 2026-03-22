# HarborNAS Skill Specification v1

## Purpose
This document defines the standard contract for Skills so teams can build, test, and run skills consistently with CLI-first execution.

## Design Principles

1. CLI first: every skill should expose CLI capability when possible.
2. Deterministic I/O: strict input and output schema.
3. Safe by default: explicit permissions and risk metadata.
4. Observable execution: structured logs and trace IDs.
5. Versioned compatibility: semantic versioning with rollback.

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

1. Router must attempt CLI first if `executors.cli.enabled=true`.
2. Browser route allowed only when CLI is unavailable for that capability.
3. MCP route allowed only when both CLI and Browser are unavailable.
4. If risk level is HIGH/CRITICAL, execution requires explicit approval token.

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
- record requested command, normalized command, executor, user, and outcome.
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

A skill is release-ready only if all tests pass.

## Versioning and Compatibility

- Patch: bugfixes without schema changes.
- Minor: backward-compatible capability additions.
- Major: breaking changes in schema or behavior.

Registry should keep at least one rollback version.

## Minimum Built-in Skills (V2)

1. `system.harbor_ops` - service status/start/stop/restart (CLI).
2. `files.batch_ops` - copy/move/archive/search (CLI).
3. `media.video_edit` - trim/concat/subtitle (CLI via ffmpeg).
4. `browser.web_automate` - browser fallback automation.
5. `planner.task_decompose` - task to step plan generation.

## Implementation Checklist

- [ ] skill.yaml validated by schema.
- [ ] CLI execution path implemented.
- [ ] permission and risk metadata configured.
- [ ] contract and smoke tests added.
- [ ] audit fields present in response.
- [ ] registry entry published.
