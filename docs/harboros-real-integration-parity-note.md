# HarborOS Real Integration Parity Note

This note keeps the HarborOS System Domain tied to the real HarborOS / TrueNAS
surface instead of turning HarborOS into a generic product abstraction.

## Scope

HarborOS owns the system-domain connector surface:

- `Middleware API`
- `MidCLI`
- `Browser/MCP fallback` only when the system-domain route cannot be served

HarborOS does not own retrieval ranking, chunking, citation selection, or answer
generation.

## What Maps To Real HarborOS Today

These are the real connector surfaces this repo can map to a HarborOS / TrueNAS
style system today:

- `service.query` through middleware and MidCLI
- `service.control` through middleware and MidCLI
- `files.copy` through middleware and MidCLI
- `files.move` through middleware and MidCLI
- `files.list` through middleware and MidCLI

The current connector parity table lives in
[`src/connectors/harboros.rs`](../src/connectors/harboros.rs).

## What Is Scaffold Only

These are intentionally preview-only helpers for framework consumption:

- `files.stat`
- `files.read_text`

They are safe, bounded substrate helpers for file-tree preview and operator
inspection, but they are not being claimed here as native HarborOS business
surfaces.

If a later layer uses them for retrieval preview, that layer must still own the
retrieval logic. HarborOS only provides the read-only substrate shape.

## What Is Still Missing

Before this is “true parity” rather than scaffold parity, we still need:

- a live HarborOS / TrueNAS integration target to verify against
- concrete middleware or MidCLI support for any missing file preview semantics
- a framework-owned retrieval layer that turns preview data into citations and
  answers
- canary evidence showing real OS responses match the connector parity table

## Operator Preflight

1. Confirm the environment exposes the real middleware and MidCLI binaries or
   endpoints.
2. Run the drift matrix and inspect the HarborOS parity section.
3. Check that the following surfaces are marked `real`:
   - `service.query`
   - `service.control`
   - `files.copy`
   - `files.move`
   - `files.list`
4. Check that `files.stat` and `files.read_text` are still marked
   `scaffold-only`.
5. If preview or allowlist failures occur, classify them as substrate issues.
6. If ranking, citation, or final-answer quality differs, classify that as a
   framework retrieval issue, not a HarborOS connector failure.

## Rollback Expectations

Rollback is boring on purpose:

- leave the real middleware / MidCLI mappings in place
- remove any caller-side dependency on `files.stat` or `files.read_text` if the
  preview surface is the problem
- do not move retrieval logic into HarborOS as a rollback shortcut
- do not change IM delivery, route handling, or AIoT ownership

## Practical Reading

Use the parity table to answer one question quickly:

- “Is this a real HarborOS connector capability, or just a safe preview helper?”

If the answer is not obvious from the table, the connector surface is too vague
and should be tightened before canary.
