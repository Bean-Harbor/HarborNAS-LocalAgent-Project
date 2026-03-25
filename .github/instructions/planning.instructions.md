---
description: "Use when creating or revising implementation plans, roadmaps, technical designs, phased delivery plans, or research-backed execution plans that must stay synchronized with /memories/session/plan.md."
---

# Planning Instructions

## Plan File Role

- Treat `/memories/session/plan.md` as the current conversation's single persistent plan.
- Planning output shown to the user should match the current version stored in `/memories/session/plan.md`.

## Revise vs Rewrite

- Revise the existing plan when changes are incremental, such as adding validation steps, reordering tasks, clarifying dependencies, or narrowing scope.
- Rewrite the plan when the core objective, target platform, architecture, or phase model changes materially.

## Open Questions

- Do not keep major assumptions implicit.
- Record unresolved questions, assumptions, and risks in the plan when they affect sequencing, scope, or delivery confidence.

## Scope Control

- Explicitly separate included work from excluded work when the user request can expand.
- If the user changes priorities or cuts scope, reflect that in the plan file instead of only mentioning it in chat.

## Consistency Rule

- Before finishing a planning response, make sure the memory plan and the user-facing plan are consistent.
- If the plan changes mid-conversation, update the memory plan in the same turn.