# Harbor Daily Sync Template

## Purpose

Use this template for the end-of-day GitHub sync across the `harbor-*` lanes.

Recommended helper skill:

- `$harbor-daily-report`

Quick trigger examples:

- `收工日报`
- `写今天日报`
- `harbor-framework 收工同步`
- `补 architect closeout`

The goal is:

- each lane owner leaves behind a consistent GitHub-ready daily summary
- `harbor-architect` can quickly decide what is merge-ready, what must wait,
  and what creates boundary or release risk

## Default File Placement

Create or update the daily note at:

- `docs/daily/YYYY-MM-DD.md`

Use one shared daily file per day. Each lane writes its own section. At the
end, `harbor-architect` appends the integration closeout section.

## Workflow

1. Each lane owner finishes code, tests, commit/push/PR hygiene, then fills in
   one lane section.
2. `harbor-framework` reports HarborNAS shared-runtime and core-platform work.
3. `harbor-im-gateway` reports IM Gateway repo work.
4. `harbor-hos-control` reports HarborOS System Domain work.
5. `harbor-aiot` reports Home Device Domain / camera / AIoT work.
6. `harbor-architect` reads the four lane sections and writes the final
   architecture closeout.

## Lane Owner Template

Copy one block per lane:

```md
## Lane Sync - <harbor-lane> - YYYY-MM-DD

### Repo / Branch / PR

- Repo:
- Branch:
- PR:

### Today's Scope

- 

### Completed

- 

### Validation

- Ran:
- Result:

### GitHub Status

- Commit/push status:
- PR status:

### Cross-Lane Impact

- Touched seam:
- Required collaborators:
- Frozen boundary affected: yes / no

### Risks / Blockers

- 

### Rollback Notes

- 

### Next Step

- 
```

## Architect Closeout Template

Append this after all lane sections:

```md
## Architect Closeout - YYYY-MM-DD

### Merge-Ready

- 

### Pending

- 

### Blocked

- 

### Boundary Check

- Frozen contract changed: yes / no
- Cross-lane routing change introduced: yes / no
- New rollback risk introduced: yes / no

### Release / Cutover View

- Safe to merge today: yes / no
- Safe to cut over today: yes / no
- Highest current risk:

### Next-Day Order

1. 
2. 
3. 
```

## Minimum Quality Bar

Every lane section should include:

- repo, branch, and PR or push state
- what actually landed today
- what was validated
- whether a frozen seam was touched
- the next concrete step

Every architect closeout should include:

- merge-ready list
- pending list
- blocked list
- explicit boundary/risk judgment

## Usage Notes

- Keep one file per day instead of one file per lane.
- If a lane has no meaningful change that day, write a short section anyway and
  say `No code change; investigation/design only`.
- If a lane touched a frozen interface, call it out explicitly instead of
  burying it in the summary.
- If tests were not run, say so directly.
- If a PR is not ready to merge, state the blocker in one sentence.
