# HarborBeacon Files BatchOps Contract v1

## Purpose
This document defines the executable contract for `files.batch_ops` with route priority:
1. middleware API (when capability exists)
2. midcli (HarborOS CLI fallback)
3. constrained local CLI templates (for non-HarborOS host operations)

## Scope
- Skill: `files.batch_ops`
- Capabilities: copy, move, archive, search
- Security baseline: explicit path policies + command template allowlist

## Canonical Action Model

```json
{
  "domain": "files",
  "operation": "copy|move|archive|search",
  "resource": {
    "paths": ["/mnt/data/a.txt"],
    "target": "/mnt/backup/"
  },
  "args": {
    "recursive": false,
    "overwrite": false,
    "pattern": "*.log"
  },
  "risk_level": "LOW|MEDIUM|HIGH|CRITICAL",
  "dry_run": true
}
```

## Path Policy
- Allowed read roots: `/mnt/**`, `/data/**`
- Allowed write roots: `/mnt/**`, `/data/**`, `/tmp/agent/**`
- Denied roots: `/`, `/etc/**`, `/boot/**`, `/root/**`, `/var/lib/**`
- All input paths must be normalized before policy check.

## Route Mapping Table

| Operation | Primary Route (Middleware API) | Secondary Route (midcli) | Tertiary Route (Constrained CLI) | Risk |
|---|---|---|---|---|
| copy | `filesystem.copy` | `filesystem copy ...` | `cp` template | MEDIUM |
| move | `filesystem.move` | `filesystem move ...` | `mv` template | HIGH |
| archive | `filesystem.archive` | `filesystem archive ...` | `tar` template | MEDIUM |
| search | `filesystem.search` | `filesystem search ...` | `find` template | LOW |

Notes:
- If middleware method is missing in current HarborBeacon build, fallback to `midcli`.
- If both middleware and `midcli` are unavailable for a capability, use constrained CLI templates only when path policy passes.

## Request Contract

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "skill_id": "files.batch_ops",
  "executor": "middleware_api|midcli|cli_template",
  "action": {
    "domain": "files",
    "operation": "copy",
    "resource": {
      "paths": ["/mnt/data/a.txt"],
      "target": "/mnt/backup/"
    },
    "args": {
      "recursive": false,
      "overwrite": false
    },
    "risk_level": "MEDIUM",
    "dry_run": true
  }
}
```

## Response Contract

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "ok": true,
  "executor_used": "midcli",
  "route_fallback_used": true,
  "result": {
    "operation": "copy",
    "items_processed": 1,
    "target": "/mnt/backup/"
  },
  "artifacts": [],
  "metrics": {
    "duration_ms": 88,
    "retries": 0
  },
  "error": null
}
```

## CLI Template Policy (When executor=cli_template)
Only the following normalized templates are allowed:

```text
cp [--recursive] [--no-clobber] <src...> <dst>
mv [--no-clobber] <src...> <dst>
tar -cf <archive> <src...>
find <root> -name <pattern>
```

Hard rules:
- No shell metacharacters in arguments (`;`, `&&`, `||`, backticks, `$()`, redirection).
- No wildcard expansion outside approved `pattern` field for search.
- Argument tokenizer must pass structured args directly (never concatenate free-form command strings).

## Risk Classification
- LOW: read-only search operations.
- MEDIUM: reversible copy/archive operations.
- HIGH: move operations across directories or overwrite-enabled actions.
- CRITICAL: any operation touching protected roots (must be blocked, not approved).

## Error Model

```json
{
  "ok": false,
  "error": {
    "code": "PATH_POLICY_DENIED|METHOD_UNAVAILABLE|VALIDATION_ERROR|TIMEOUT|NON_ZERO_EXIT|APPROVAL_REQUIRED",
    "message": "human-readable summary",
    "details": {
      "operation": "move",
      "path": "/etc/passwd"
    }
  }
}
```

## Compatibility Matrix Template

| Capability | HarborBeacon middleware ref | Upstream middleware ref | API test | midcli test | CLI-template test | Decision |
|---|---|---|---|---|---|---|
| files.copy | develop | master | pass/fail | pass/fail | pass/fail | keep/block |
| files.move | develop | master | pass/fail | pass/fail | pass/fail | keep/block |
| files.archive | develop | master | pass/fail | pass/fail | pass/fail | keep/block |
| files.search | develop | master | pass/fail | pass/fail | pass/fail | keep/block |

## Minimum Test Cases
1. Copy/move/archive/search happy paths within allowed roots.
2. Reject denied roots and path traversal payloads.
3. Dry-run preview for HIGH operations.
4. Fallback chain validation: API failure -> midcli -> CLI template.
5. Non-zero exit and timeout handling for each executor.

## Release Gate
A release is allowed only when:
- path policy tests pass,
- contract and fallback tests pass,
- compatibility matrix has no blocking rows,
- audit payload contains `executor_used`, `route_fallback_used`, and normalized action.
