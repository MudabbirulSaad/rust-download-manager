import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {loadProject, findTicket, validateContracts, requirementsHash, requiredChecks, validateHandoff, reconcileCompletion} from '../scripts/agent-workflow.mjs';

const project = loadProject();
const clone = value => structuredClone(value);
const hash = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');

test('build-specific license notices preserve clean source across different Cargo caches', () => {
  const work = mkdtempSync(join(tmpdir(), 'rdm-notices-'));
  try {
    const root = join(work, 'repo'), cargo = join(work, 'cargo');
    mkdirSync(join(root, 'scripts'), {recursive: true});
    copyFileSync(new URL('../scripts/notices.py', import.meta.url), join(root, 'scripts/notices.py'));
    writeFileSync(join(root, '.gitignore'), '/dist/\n');
    writeFileSync(join(root, 'THIRD_PARTY_NOTICES.md'), 'Committed source notice\n');
    writeFileSync(join(root, 'Cargo.lock'), 'version = 4\n[[package]]\nname = "fixture"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n');
    writeFileSync(join(root, 'package-lock.json'), '{"packages":{}}');
    const git = args => execFileSync('git', args, {cwd: root, encoding: 'utf8'});
    git(['init', '-q']); git(['add', '.']);
    git(['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgSign=false', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'fixture']);
    const pkg = join(cargo, 'registry/src/test/fixture-1.0.0');
    mkdirSync(pkg, {recursive: true});
    writeFileSync(join(pkg, 'Cargo.toml'), '[package]\nname="fixture"\nversion="1.0.0"\nlicense="MIT"\n');
    writeFileSync(join(pkg, 'LICENSE'), 'Fixture license text\n');
    const generate = cache => execFileSync('python3', ['scripts/notices.py'], {cwd: root, env: {...process.env, CARGO_HOME: cache}});
    generate(cargo);
    assert.equal(readFileSync(join(root, 'THIRD_PARTY_NOTICES.md'), 'utf8'), 'Committed source notice\n');
    assert.match(readFileSync(join(root, 'dist/THIRD_PARTY_NOTICES.md'), 'utf8'), /fixture 1\.0\.0: MIT/);
    assert.equal(readFileSync(join(root, 'dist/licenses/rust-fixture-1.0.0/LICENSE'), 'utf8'), 'Fixture license text\n');
    assert.equal(git(['status', '--porcelain']), '');
    generate(join(work, 'empty-cargo-cache'));
    assert.doesNotMatch(readFileSync(join(root, 'dist/THIRD_PARTY_NOTICES.md'), 'utf8'), /fixture 1\.0\.0/);
    assert.equal(readFileSync(join(root, 'THIRD_PARTY_NOTICES.md'), 'utf8'), 'Committed source notice\n');
    assert.equal(git(['status', '--porcelain']), '');
  } finally { rmSync(work, {recursive: true, force: true}); }
});
function fixture(code = 'B04', environment = 'local') {
  const ticket = clone(project.plan.tickets.find(t => t.code === code));
  const registry = project.registry;
  const required = requiredChecks(ticket, [], registry).ids;
  const headSha = 'a'.repeat(40), baseSha = 'b'.repeat(40);
  const report = {schema_version: 1, issue_key: ticket.agent.key, clean: true,
    head_sha: headSha, base_sha: baseSha, environment, registry_sha256: hash(registry),
    checks: required.map(id => ({id, command: registry.checks[id].command,
      status: 'passed', exit_code: 0, log_sha256: 'd'.repeat(64),
      interactive: registry.checks[id].interactive ?? false}))};
  const handoff = {schema_version: 1, issue_key: ticket.agent.key,
    requirements_sha256: requirementsHash(ticket), head_sha: headSha, base_sha: baseSha,
    environment, branch: 'rdm/rdm-15-content-validation', pr_url: 'https://github.com/MudabbirulSaad/rust-download-manager/pull/2',
    live_requirements: {compared: true, checked_at: '2026-10-28T00:00:00Z',
      issue_url: `https://rdm-mudabbirulsaad.atlassian.net/browse/${ticket.agent.key}`},
    acceptance: ticket.agent.acceptance_ids.map(id => ({id, status: 'passed',
      explanation: 'Controlled fixture exercised the criterion.', check_ids: ['rust-tests']})),
    limitations: [], migrations: [], independent_review: {status: 'passed', head_sha: headSha, open_findings: 0, summary: 'Final diff and regressions reviewed.'}};
  return {handoff, args: {ticket, registry, headSha, baseSha, required, reports: [report]}};
}

test('all original identities, 40 parent contracts and 66 native dependencies are retained', () => {
  assert.equal(validateContracts(project).dependencies, 66);
  assert.equal(validateContracts(project).tickets, 40);
});
test('focused subtasks are assignable by Jira key and retain their parent acceptance mapping', () => {
  assert.equal(validateContracts(project).subtasks, 3);
  assert.equal(findTicket(project, 'RDM-52').parent_key, 'RDM-18');
  assert.deepEqual(findTicket(project, 'RDM-54').agent.dependencies, ['RDM-53']);
  const copy = clone(project); copy.plan.subtasks[0].parent_key = 'RDM-999';
  assert.throws(() => validateContracts(copy), /Invalid subtask parent/);
  const other = clone(project); other.plan.subtasks[0].agent.covers_parent_criteria = ['AC-999'];
  assert.throws(() => validateContracts(other), /Invalid parent acceptance/);
});
test('requirements changes invalidate a contract; readiness changes do not', () => {
  const t = clone(project.plan.tickets[0]); const original = requirementsHash(t);
  t.agent.readiness.state = 'agent-blocked'; assert.equal(requirementsHash(t), original);
  t.criteria[0] += ' A changed product requirement.'; assert.notEqual(requirementsHash(t), original);
  const copy = clone(project); copy.plan.tickets[0] = t;
  assert.throws(() => validateContracts(copy), /Stale requirements hash/);
});
test('cyclic blockers fail even when native dependency counts are unchanged', () => {
  const copy = clone(project); const t = copy.plan.tickets.find(t => t.code === 'B02');
  t.depends = ['B05']; t.agent.dependencies = [copy.manifest.issue_keys.B05];
  t.agent.requirements_sha256 = requirementsHash(t);
  assert.throws(() => validateContracts(copy), /cycle/);
});
test('diff impact adds checks that a narrow ticket profile cannot omit', () => {
  const t = clone(project.plan.tickets.find(t => t.code === 'B40'));
  const checks = requiredChecks(t, ['extensions/src/shared.ts', 'src/App.tsx'], project.registry);
  assert(checks.ids.includes('firefox-snap-handoff'));
  assert(checks.ids.includes('desktop-webdriver'));
});
test('complete same-revision evidence is eligible for an independent merge review', () => {
  const {handoff, args} = fixture('B04', 'cloud'); args.phase = 'merge';
  assert.equal(validateHandoff(handoff, args).eligible, true);
});
test('a review claim cannot substitute local-only results for independent cloud execution', () => {
  const {handoff, args} = fixture(); args.phase = 'merge';
  assert.throws(() => validateHandoff(handoff, args), /Independent cloud rerun missing/);
});
test('handoffs require a source branch and the actual repository PR identity', () => {
  for (const kind of ['branch', 'pr']) {
    const {handoff, args} = fixture();
    if (kind === 'branch') handoff.branch = '';
    else handoff.pr_url = 'https://github.com/someone/other/pull/2';
    assert.throws(() => validateHandoff(handoff, args), /source branch|pull request URL/);
  }
});
test('unrun acceptance criteria and missing required checks prevent readiness', () => {
  const first = fixture(); first.handoff.acceptance[0].status = 'not-run';
  assert.throws(() => validateHandoff(first.handoff, first.args), /Acceptance not established/);
  const second = fixture(); second.args.reports[0].checks.pop();
  assert.throws(() => validateHandoff(second.handoff, second.args), /Required check not passed/);
});
test('a deliberately failing check cannot be masked by another report', () => {
  const {handoff, args} = fixture(); const failed = clone(args.reports[0]);
  failed.checks[0].status = 'failed'; failed.checks[0].exit_code = 1;
  args.reports.push(failed);
  assert.throws(() => validateHandoff(handoff, args), /Required check failed/);
});
test('changed head, changed base, dirty checkout and changed check registry invalidate evidence', () => {
  for (const kind of ['head', 'base', 'dirty', 'registry']) {
    const {handoff, args} = fixture();
    if (kind === 'head') args.headSha = 'c'.repeat(40);
    if (kind === 'base') args.baseSha = 'c'.repeat(40);
    if (kind === 'dirty') args.reports[0].clean = false;
    if (kind === 'registry') args.reports[0].registry_sha256 = 'wrong';
    assert.throws(() => validateHandoff(handoff, args), /Stale|dirty|changed/);
  }
});
test('a code fix requires fresh reports and a fresh review of that new head', () => {
  const {handoff, args} = fixture('B04', 'cloud'); const head = 'c'.repeat(40);
  args.headSha = head; args.phase = 'merge'; handoff.head_sha = head;
  args.reports[0].head_sha = head;
  assert.throws(() => validateHandoff(handoff, args), /Independent review/);
  handoff.independent_review.head_sha = head;
  assert.equal(validateHandoff(handoff, args).eligible, true);
});
test('a cloud run cannot claim to have granted the local Firefox Snap portal', () => {
  const {handoff, args} = fixture('B03', 'cloud');
  assert.throws(() => validateHandoff(handoff, args), /Wrong environment/);
});
test('cloud evidence combines with durable, same-revision local browser evidence', () => {
  const {handoff, args} = fixture('B03', 'cloud');
  const local = clone(args.reports[0]); local.environment = 'local';
  args.reports[0].checks = args.reports[0].checks.filter(c => !args.registry.checks[c.id].environments);
  args.reports.push(local);
  assert.throws(() => validateHandoff(handoff, args), /durable GitHub artifact/);
  local.artifact_url = 'https://github.com/MudabbirulSaad/rust-download-manager/actions/runs/123/artifacts/456';
  assert.equal(validateHandoff(handoff, args).eligible, true);
  local.head_sha = 'c'.repeat(40);
  assert.throws(() => validateHandoff(handoff, args), /Stale/);
});
test('a reported pass still requires exit zero, matching commands and real log evidence', () => {
  for (const kind of ['exit', 'command', 'log']) {
    const {handoff, args} = fixture(); const c = args.reports[0].checks[0];
    if (kind === 'exit') c.exit_code = 1;
    if (kind === 'command') c.command = ['true'];
    if (kind === 'log') c.log_sha256 = '';
    assert.throws(() => validateHandoff(handoff, args), /execution evidence|command changed/);
  }
});
test('merge requires an independent review and rejects unknown gate phases', () => {
  const {handoff, args} = fixture('B04', 'cloud'); args.phase = 'merge'; handoff.independent_review.status = 'not-run';
  assert.throws(() => validateHandoff(handoff, args), /Independent review/);
  args.phase = 'typo'; assert.throws(() => validateHandoff(handoff, args), /Unknown handoff gate phase/);
});
test('Jira outage after merge reconciles once without proposing another merge', () => {
  const state = {issue_key: 'RDM-15', pr_url: 'https://github.com/MudabbirulSaad/rust-download-manager/pull/2',
    merged: true, head_sha: 'a'.repeat(40), expected_head_sha: 'a'.repeat(40), merge_sha: 'b'.repeat(40),
    post_merge: {sha: 'b'.repeat(40), status: 'success'}, jira_status: 'In Review', jira_comment_exists: false};
  assert.deepEqual(reconcileCompletion(state), {action: 'reconcile_jira', merge: false,
    merge_sha: state.merge_sha, add_comment: true, transition_done: true});
  state.jira_comment_exists = true;
  assert.equal(reconcileCompletion(state).add_comment, false);
  state.jira_status = 'Done';
  assert.deepEqual(reconcileCompletion(state), {action: 'complete', merge: false, jira_write: false});
});
test('pending, skipped or wrong-revision post-merge CI never closes Jira', () => {
  const state = {issue_key: 'RDM-15', pr_url: 'https://github.com/example/repo/pull/1', merged: true,
    head_sha: 'head', expected_head_sha: 'head', merge_sha: 'merged', jira_status: 'In Review'};
  for (const post_merge of [{sha: 'merged', status: 'pending'}, {sha: 'merged', status: 'skipped'}, {sha: 'older', status: 'success'}]) {
    assert.equal(reconcileCompletion({...state, post_merge}).action, 'wait_for_post_merge_checks');
  }
  assert.throws(() => reconcileCompletion({...state, expected_head_sha: 'older'}), /differs/);
});
