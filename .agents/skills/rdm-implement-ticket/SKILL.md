---
name: rdm-implement-ticket
description: Implement one RDM Jira ticket locally, verify its acceptance criteria, and prepare a tested pull request for a separate cloud reviewer. Use for requests such as "Implement RDM-15"; not for reviewing or merging another agent's PR.
---

# Implement an RDM ticket

Read the repository `AGENTS.md` and `docs/agent-workflow.md`. Use repository-relative
paths; do not depend on this machine's home directory or the installed RDM app.

1. Fetch the live RDM issue, epic and blocking links. Run
   `npm run agent:ticket -- RDM-KEY`. Compare current requirements to the committed
   contract and resolve differences before implementation. Confirm prerequisites
   are delivered. An unavailable connection or unresolved product decision is a
   named blocker, not permission to guess a new scope.
2. Reuse the ticket's open PR/branch when resuming. Otherwise create an isolated
   worktree/branch from fresh `origin/main`; use `rdm/rdm-KEY-short-description`.
   Record baseline SHA and baseline failures. Move Jira to In Progress only when
   actual work starts; preserve its assignee, priority, epic and unrelated labels.
3. Implement the contract's behavior with relevant regression tests and docs.
   Inspect surrounding code instead of treating starting-file hints as a hard
   boundary. Keep protocol DTOs and persisted state compatible as applicable.
4. Commit the changes. Run `npm run agent:check -- --ticket RDM-KEY
   --environment local --base origin/main`. If a check fails, fix its cause and
   rerun affected checks at the new revision. Keep failures and unrun checks visible.
5. Create an evidence template with `npm run agent:handoff -- init --ticket
   RDM-KEY --report .agent-runs/RUN/checks.json`. Fill acceptance criteria using
   observed test results, local evidence and explanations. The template deliberately
   starts unverified; it is not a certificate. Add the PR URL and source branch
   before final validation with the handoff command.
6. Push and create/update a PR; attach durable report/logs and the Jira link. Include
   migrations, limitations and exact base/head SHAs. Do not attach cookies or real
   browser/download data. Reuse the existing PR after interruptions.
7. Only once the handoff gate passes, move Jira to In Review and add
   `agent-review-ready`, removing obsolete readiness/blocker labels. Otherwise leave
   it In Progress with the precise outstanding requirement and a draft PR if useful.

Finish with the PR link, criteria/check results, and a ready-to-copy cloud request:
`Use $rdm-review-ticket to review RDM-KEY, fix in-scope issues, run its required
checks, then merge and close only if all gates pass.` Local implementation does
not merge its own PR.
