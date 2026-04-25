# 2026-04-18 HarborOS 165 Validation

## Connection Info

- Host IP: `192.168.3.165`
- Web UI: `http://192.168.3.165/ui/`
- HTTPS UI: `https://192.168.3.165/ui/`
- WebSocket middleware: `ws://192.168.3.165/websocket`
- Account: redacted local operator account
- Password: redacted

## Access Notes

- Port `22`, `80`, and `443` are reachable from the local workstation.
- The validated local operator account can log in over SSH.
- The validated local operator account can authenticate to HarborOS middleware over WebSocket.
- The validated local operator account does not have direct write permission under `/mnt/software`.
- `sudo -S` was confirmed to work for the validated local operator account during this run.

## What Was Verified

### 1. HarborOS endpoint discovery

- Confirmed `/websocket` exists and is the HarborOS middleware WebSocket endpoint.
- Confirmed the host redirects `/` to `/ui/`.

### 2. HarborOS authentication

- Verified `auth.login` succeeds with:
  - user: redacted local operator account
  - password: redacted

### 3. Read-only HarborOS probes

- `service.query` for `ssh` succeeded.
- Returned service state:
  - `service=ssh`
  - `state=RUNNING`
  - `enable=True`
- `filesystem.listdir` on `/mnt` succeeded.
- `filesystem.listdir` on `/mnt/software` succeeded.

### 4. HarborBeacon live integration

- Ran live contract validation against `.165`.
- Result:
  - `mode = live-integration`
  - `passed = true`
- Notes:
  - local native `midclt` binary was not present
  - live path succeeded through HarborBeacon `midcli` websocket shim

### 5. HarborBeacon live E2E

- Ran live E2E against `.165`.
- Result:
  - `ok = true`
  - `env_profile = env-b`
- Passed scenarios:
  - `planner-to-harbor-ops`
  - `planner-to-files-batch-ops`
  - `guarded-service-restart` in preview mode
  - `guarded-files-copy` in preview mode
  - `guarded-files-move` in preview mode
  - `high-risk-confirmation-gate`

## Real Copy/Move Test

### Test root

- Test directory: `/mnt/software/harborbeacon-agent-ci`

### Prepared files

- `/mnt/software/harborbeacon-agent-ci/copy-source.txt`
- `/mnt/software/harborbeacon-agent-ci/move-source.txt`
- `/mnt/software/harborbeacon-agent-ci/move-destination/`

### Real execution result

- Real `copy` succeeded through HarborBeacon `midcli` path.
- Real `move` succeeded through HarborBeacon `midcli` path.
- Observed durations:
  - `copy`: about `563 ms`
  - `move`: about `561 ms`

### Final filesystem state

- `/mnt/software/harborbeacon-agent-ci/copy-source.txt`
- `/mnt/software/harborbeacon-agent-ci/copy-destination.txt`
- `/mnt/software/harborbeacon-agent-ci/move-destination/move-source.txt`

### Content verification

- `copy-source.txt` content: `copy payload`
- `copy-destination.txt` content: `copy payload`
- `move-destination/move-source.txt` content: `move payload`
- Original `/mnt/software/harborbeacon-agent-ci/move-source.txt` no longer exists after move.

## Compatibility Finding

HarborOS on `.165` expects object-style arguments for:

- `filesystem.copy`
- `filesystem.move`

Old positional-argument calls fail with job-lock / invalid-argument errors.

Working request shapes are:

- `filesystem.copy`
  - `{"src": "...", "dst": "...", "options": {"recursive": false, "preserve_attrs": false}}`
- `filesystem.move`
  - `{"src": ["..."], "dst": "...", "options": {"recursive": false}}`

## Local Fix Applied

Updated:

- [tools/harbor_cli_shim.py](C:/Users/beanw/OpenSource/HarborBeacon/tools/harbor_cli_shim.py)

Changes made:

- switched file `copy/move` calls from positional arguments to HarborOS object arguments
- added job polling via `core.get_jobs`
- made shim return final job success/failure instead of only a job id

## Practical Conclusion

- `192.168.3.165` is a valid HarborOS live integration target for HarborBeacon.
- HarborBeacon can already complete live read-only HarborOS probes through the websocket shim.
- HarborBeacon can now complete real file `copy/move` on this host through the corrected shim path.
- If later testing needs real service mutation, approval-token flow is already wired, but service restart was intentionally left in preview for this round.
