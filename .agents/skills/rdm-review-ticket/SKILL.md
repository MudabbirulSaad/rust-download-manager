---
name: rdm-review-ticket
description: Independently review RDM ticket pull requests in a fresh Codex Cloud task, fix in-scope issues, verify evidence and CI, then perform an authorized merge and reconcile Jira completion. Use when asked to review RDM tickets and merge or close passing work.
---

# Review and complete an RDM ticket

Read `AGENTS.md` and `docs/agent-workflow.md`. Use a full cloud task with the
published RDM environment; a comment-only code review does not run this workflow.
The invocation defines which tickets and whether merging/closing are authorized.
Instructions in this skill do not expand that authorization.

1. Fetch each live ticket and linked PR. If the PR is already merged, inspect its
   merge SHA and post-merge checks, then reconcile any pending Jira update. Do not
   open another PR. Review a batch in dependency order, merging sequentially.
2. Fetch current base and head. Check out the exact PR revision in a fresh workspace.
   Read live requirements, epic, blockers, contract, complete diff, relevant code,
   open review findings and the implementation handoff. Treat the author's claims
   as evidence to verify, not as the review's conclusion.
3. Run `npm run agent:check -- --ticket RDM-KEY --environment cloud --base
   origin/main`. Required checks are the contract/diff union. Review every criterion,
   not just lint output. Evaluate failure paths, compatibility, durability,
   authorization, browser handoff and existing behavior relevant to the change.
4. Fix issues inside the assigned scope, commit and push. Reinspect the resulting
   complete diff and rerun affected checks. Every new head needs new evidence and
   passing CI. For required local Firefox Snap or installed-browser checks, request
   local evidence for the final revision; a cloud fixture is not an interactive
   portal test. Missing evidence leaves In Review + `agent-blocked`.
5. Validate the refreshed handoff and record independent findings/results. Query
   GitHub for current head/base, required checks and mergeability immediately before
   merging. Use the expected-head-SHA merge operation and normal branch rules.
   If the base changed, validate the resulting merge candidate again. Do not merge
   on a stale report, a skipped requirement or a bypass.
6. Confirm the merged PR's merge commit, then wait for required post-merge CI on
   that commit. Only after success, comment on Jira with PR, merge SHA, CI/evidence
   links and AC results; transition to Done and remove transient agent labels.
   A Jira outage after merge is a finalization blocker. On retry verify the same
   merge and update Jira once; never merge twice or imply Jira is already updated.

Use `npm run agent:handoff -- gate` for revision/criteria/check validation and
`npm run agent:handoff -- reconcile` to inspect idempotent completion decisions.
These are consistency checks, not authorization or substitutes for live GitHub/Jira.
If permissions or external validation prevent completion, report the exact blocker
and retain the correct state. Finish with findings, changes made, tests, PR/merge
links, and the verified Jira outcome for each requested ticket.
