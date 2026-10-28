# Local implementation and Codex Cloud review

Repository: https://github.com/MudabbirulSaad/rust-download-manager

Jira: https://rdm-mudabbirulsaad.atlassian.net/jira/software/projects/RDM/boards/1

Use a normal local Codex task for implementation and a **fresh full cloud task**
for review. Neither a Jira assignment alone nor `@codex review` launches this flow.
No polling daemon, automation subscription or additional Jira user is required.

## One-time setup

1. Connect this GitHub repository and the **RDM** Atlassian site to the agents.
   Old-site connector access and local CLI credentials do not establish cloud access.
   Verify repository read/push/PR/merge and RDM read/comment/transition permissions.
2. Use the repository's pinned Rust and Node versions. Run
   `bash scripts/agent-setup.sh --system --drivers` on a disposable Ubuntu environment
   (omit `--system` when native packages are already installed). Source
   `.cache/agent-env.sh` in each task before running checks.
3. In Codex Cloud, create an environment for this repository, use the setup command
   above as its installation instruction and source the environment file in its
   start skill. Run doctor, contract validation and common/http/media checks. Publish
   only after these pass. Republish when toolchain/dependency setup changes.
4. Keep package-manager/upstream tool download access in environment setup. Actual
   transfer regressions use localhost and fake credentials. Configure GitHub/Jira
   service access separately through supported connections or secret-backed access;
   never copy browser cookies or local credential stores into a cloud environment.

## Everyday prompts

Local:

> Use $rdm-implement-ticket to implement RDM-15 and prepare its tested PR.

Cloud:

> Use $rdm-review-ticket to review RDM-15, fix in-scope issues, run required checks,
> then merge and close only when every gate passes.

A batch names explicit ticket keys. Review dependencies first and merge sequentially.
The roles are skills, not imaginary Jira/GitHub accounts. Preserve the real assignee.

## Contracts and readiness

`planning/jira-backlog.json` retains the original 40 tasks and adds `agent` version 1:
Jira/epic keys, semantic requirements hash, actual dependency keys, profiles,
starting points/fixtures, boundaries, implementation notes, local evidence and
readiness. `npm run agent:ticket -- RDM-15` prints the brief without claiming to have
read Jira. Fetch live requirements and compare them; refresh meaningful drift.
Status/comments are not part of the semantic hash, so bookkeeping does not silently
change product requirements. `npm run agent:validate` checks IDs, links, DAG,
criteria, paths and profiles. `python3 scripts/enrich-agent-contracts.py` regenerates
reviewed contracts and existing-issue update payloads; it does not write to Jira.

- `agent-ready`: scoped, unblocked, verification defined. Confirm live prerequisites.
- `needs-refinement`: unresolved product contract, external distribution or oversized work.
- `agent-blocked`: prerequisites/access/validation outstanding; state the exact blocker.
- `agent-review-ready`: completed local handoff, In Review, suitable for cloud selection.

Epics organize work and do not represent independently executable assignments.
RDM-18 separates diagnostics, packaging, and external signing/distribution approval.
Its subtasks are RDM-52, RDM-53 and RDM-54. Their execution contracts live in the
same planning JSON under `subtasks` and are accepted by every ticket command.
Do not close a parent until all its criteria and required child deliverables pass.
Apply existing-issue payloads through field-level Jira edits. The legacy CSV
updater clears epic parents when hierarchy fields are omitted; do not use partial
CSV imports for these updates. Verify preserved parents, priorities and native
dependencies after any bulk operation.

## Checks and evidence

`planning/agent-checks.json` is shared by local work, cloud and CI. Required checks
are the union of ticket profiles and diff impact. A code change cannot silently
avoid a browser/desktop/storage check by choosing a narrower profile.

```bash
npm run agent:doctor
npm run agent:validate
npm run agent:test
npm run agent:check -- --ticket RDM-15 --environment local --base origin/main
npm run agent:check -- --ticket RDM-15 --environment cloud --base origin/main
```

Reports/logs live in ignored `.agent-runs/`. A report records exact HEAD/base,
clean-worktree state, command arguments, exit codes, timestamps, log hashes,
environment versions and required checks. `--allow-dirty` is for provisional local
diagnosis only. A provisional report cannot pass a handoff gate. A missing command
fails; a required external/interactive check stays unrun. No timeout/skip is Passed.

For an interactive browser check add `--interactive` locally and complete the real
Firefox Snap portal grant. `brave-handoff` and `firefox-snap-handoff` cannot be
claimed by a cloud run. The cloud reviewer combines independently rerun cloud
checks with exact-head local evidence linked to a durable GitHub PR/Actions artifact.
Required Docker package checks can be supplied by CI. Same-version reinstall/removal
coverage is labelled that way; it is not falsely labelled a prior-version migration.

Generate and fill a handoff after committing and checking:

```bash
npm run agent:handoff -- init --ticket RDM-15 --report .agent-runs/RUN/checks.json
npm run agent:handoff -- gate --ticket RDM-15 --handoff .agent-runs/RUN/handoff.json
```

The generated template has **not-run** criteria and an unverified live comparison.
Fill each criterion with an explanation and check IDs that actually establish it.
Attach reports/logs to the PR/CI and replace local-only report paths when importing
them in cloud. External report objects include their durable `artifact_url`.
Record limitations and migrations explicitly, including empty lists when applicable.
Use `--phase merge` only after recording independent review of the final revision.

The JSON gates enforce consistency; they are not signatures or authorization and do
not prove that an assertion is truthful. The reviewer must inspect actual logs,
criteria coverage and live services. Review changes to the check definitions too;
do not approve a weakened gate just because the modified gate says green.

## Merge and Jira recovery

Immediately before merging, query live GitHub for current head/base, required CI,
mergeability and findings. Use expected head SHA and normal branch protection.
New code or a changed base needs fresh relevant evidence. The cloud reviewer can
fix within scope; required local evidence then needs the new revision as well.

After merging, verify the merge SHA and required push CI. Then comment once on Jira
with ticket/PR/merge/check links, transition Done and remove transient agent labels.
Never infer that a Jira write succeeded because GitHub merged.

`npm run agent:handoff -- reconcile --state STATE.json` evaluates a normalized live
snapshot containing issue key, PR URL, merged flag, head/expected-head/merge SHAs,
post-merge `{sha,status}`, Jira status and whether the completion comment exists.
It never performs a network mutation. After a Jira outage it proposes only missing
Jira steps, never another merge. An already reconciled completion is a no-op.
Only actual API/tool results establish those snapshot fields.

## Pilot and operational limits

Use RDM-15 for the first local-to-cloud ticket. Workflow regression tests cover a
failed check, stale head/base, changed contracts, unrun criteria, cloud/local evidence
mixing, reviewer fixes, repeated invocation and interrupted finalization.

The cloud environment, service grants and actual pilot must have separate verified
results. Do not mark their setup or the pilot complete from a unit simulation.
Do not install an app build on the host or publish releases as a side effect of merging.
