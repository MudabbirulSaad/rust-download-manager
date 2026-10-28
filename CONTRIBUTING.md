# Contributing

Contributions are GPL-3.0-only. Keep dependencies, original assets and license
notices compatible with that license. Never add the proprietary reference
installer, credentials, browser profiles, caches or downloaded user files.

Read `AGENTS.md` and `docs/agent-workflow.md`. Start from a live RDM Jira ticket
with clear acceptance criteria and delivered prerequisites. Use an isolated
branch/worktree and link one focused pull request to the ticket. Preserve existing
SQLite histories, scoped browser credentials and durable download checkpoints.

Run `bash scripts/agent-setup.sh --system --drivers` on Ubuntu, source
`.cache/agent-env.sh`, and use `npm run agent:check -- --ticket RDM-KEY
--environment local --base origin/main`. Attach actual criterion evidence and
logs for the final revision. Required interactive checks must be run locally;
missing checks remain unrun. Do not include real URLs containing secrets or cookies.

A separate cloud reviewer reviews the full diff, may fix within scope, reruns
checks and uses the normal protected merge path. Update Jira to Done only after
the merge commit's required CI passes. Publishing releases and installing builds
on a user's machine require their own explicitly scoped work.

The `v0.2.2` tag preserves the original published baseline. Candidate source
archives come from `git archive HEAD`; build artifacts are linked to exact source
commits and CI runs. Generate third-party notices with
`python3 scripts/notices.py` and verify `dist/SHA256SUMS` after a release build.
