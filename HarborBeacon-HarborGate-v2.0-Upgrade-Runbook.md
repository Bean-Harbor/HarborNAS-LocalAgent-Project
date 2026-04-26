# HarborBeacon HarborGate v2.0 Upgrade Runbook

## Status

This is the active control pack entry for the HarborBeacon side of the v2.0
upgrade.

Authoritative contract:

- `C:\Users\beanw\OpenSource\HarborGate\HarborBeacon-HarborGate-Agent-Contract-v2.0.md`

The v1.5 documents are historical references only. Do not use them as current
release gates.

## Daily Start

At the start of each session:

1. Read the v2.0 contract.
2. Read this runbook.
3. Check local git status in HarborBeacon and HarborGate.
4. Identify the current phase and the one main line for the day.
5. Confirm whether the next action is local-only or needs live target access.

## Phases

### Phase 1: Control Pack

- Establish v2.0 contract authority.
- Update HarborBeacon docs and tests to point at v2.0.
- Add drift guards that expose remaining v1.5 active paths.
- Do not implement `/api/turns` yet.

### Phase 2: Beacon Turn Core

- Add `TaskTurnEnvelope`.
- Add `POST /api/turns`.
- Normalize turn identity around Beacon-owned `conversation.handle`.
- Introduce `ActiveDialogueFrame` and `ConversationAct`.
- Preserve approvals, artifacts, audit, and media records.

### Phase 3: Gate Turn Client

- Make HarborGate emit v2.0 turn requests.
- Cache only opaque `conversation.handle` and continuation values.
- Remove default `/api/tasks` task-client behavior from active path.
- Keep platform credentials and delivery in Gate.

### Phase 4: Delivery And Live Proof

- Drive Weixin native video/file behavior through v2 `delivery_hints`.
- Run local contract tests in both repos.
- Confirm `.182` using the target registry before live steps.
- Run the Weixin private-DM matrix.

### Phase 5: Closeout

- Write v2.0 cutover evidence.
- Write rollback notes.
- Sync both repos to GitHub.
- Leave exact next steps for the next session.

## Drift Guards

The upgrade must fail fast when active work drifts back to v1.5.

Guard conditions:

- Active path must not use `X-Contract-Version: 1.5`.
- HarborGate active path must not call `/api/tasks`.
- New active code must not emit `args.resume_token`.
- HarborBeacon must not treat `source.session_id` as business conversation
  truth.
- HarborGate must not parse Beacon active-frame business semantics.
- Group chat remains out of scope.

The first control-pack commit may intentionally introduce failing guard tests.
Those failures are the queue for the code-upgrade phases.

## Stop-The-Line Conditions

Stop and ask the user before continuing when any of these appear:

- A new public v2.0 contract field is required.
- Ownership between Beacon and Gate would change.
- A v1.5 compatibility path is requested.
- Group chat is needed for the current path.
- Live target, credential, DNS, or external platform state blocks the work.

## Daily Closeout

Every day ends with:

- completed
- changed files
- tests run
- drift check result
- blockers
- next exact step

Do not report a release-ready state while any drift guard still fails.
