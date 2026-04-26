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
- Route `general.message` through active-frame policy before ordinary
  conversation acts whenever a pending frame exists.
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
- Active frames must persist across no-tool conversation acts until explicit
  resolve, cancel, or superseding tool intent.
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

## 2026-04-26 Closeout

- Completed: v2.0 turn core and active `/api/turns` ingress are implemented
  locally; assistant task API tests now use turn envelopes; active
  Beacon/Gate defaults now use contract `2.0`; clarification feedback keeps a
  pending `conversation.clarify` active frame; clip-confirmation feedback now
  goes through frame-first policy, so no-tool turns preserve
  `camera.clip_confirmation` until explicit playback, cancel, or superseding
  tool intent; `.197` built
  `harbor-release-20260426-v20-frame-first-r1.tar.gz`; `.182` is deployed on
  that bundle and passes the direct v2 `/api/turns` frame-first matrix through
  native-video playback hints.
- Changed files: `src/runtime/task_api.rs`,
  `src/bin/assistant_task_api.rs`, contract default sources/templates/tests,
  v2.0 observability docs, and
  `worklogs/2026-04-26-v20-upgrade.md`.
- Tests run: `python -m pytest tests/contracts/test_im_v20_control_pack.py -q`,
  release-packaging contract tests, `cargo test --bin assistant-task-api`,
  targeted general.message turn tests, targeted clip-confirmation frame tests,
  `cargo test`, `python -m pytest -q`, `git diff --check`, `gh pr checks 3`,
  and the `.182` direct `/api/turns` frame-first matrix after the
  clip-confirmation persistence fix.
- Drift check: Beacon v2.0 guard passed; active assistant task API no longer
  exposes the v1.5 task ingress; packaged env uses
  `IM_AGENT_CONTRACT_VERSION=2.0`; Gateway accepts contract `2.0` and rejects
  `1.5`.
- Blockers: `cargo fmt` is unavailable because `rustfmt` is not installed;
  real Weixin private-DM evidence still requires a user-side message run.
- Next exact step: run the Weixin private-DM v2.0 matrix before merge
  readiness.
