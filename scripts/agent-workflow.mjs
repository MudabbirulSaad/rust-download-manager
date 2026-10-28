import {spawn, spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync, writeFileSync, mkdirSync, mkdtempSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {resolve, join, dirname, isAbsolute} from 'node:path';
import {fileURLToPath} from 'node:url';

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const readJson = path => JSON.parse(readFileSync(path, 'utf8'));
const sha256 = value => createHash('sha256').update(value).digest('hex');
const unique = values => [...new Set(values)];
const invariant = (condition, message) => { if (!condition) throw new Error(message); };
const text = value => typeof value === 'string' && value.trim().length > 0;

export function requirementsHash(ticket) {
  const value = Object.fromEntries(['title', 'context', 'scope', 'criteria', 'depends'].sort().map(key => [key, ticket[key]]));
  return sha256(JSON.stringify(value));
}

export function loadProject(root = ROOT) {
  return {root, plan: readJson(join(root, 'planning/jira-backlog.json')),
    manifest: readJson(join(root, 'planning/jira-import-manifest.json')),
    registry: readJson(join(root, 'planning/agent-checks.json'))};
}

export function validateContracts({root, plan, manifest, registry}) {
  invariant(plan.agent_contract_version === 1, 'Unsupported execution contract version');
  invariant(plan.tickets.length === 40 && plan.epics.length === 7, 'Roadmap identities/count changed unexpectedly');
  const subtasks = plan.subtasks ?? [];
  const all = [...plan.tickets, ...subtasks];
  const codes = new Map(all.map(t => [t.code, t]));
  invariant(codes.size === all.length, 'Duplicate planning code');
  const keys = new Set(); let links = 0;
  for (const t of all) {
    const a = t.agent;
    const isSubtask = subtasks.includes(t);
    invariant(a?.version === 1 && (isSubtask ? /^RDM-\d+$/.test(a.key) : a.key === manifest.issue_keys[t.code]), `Missing/mismatched key: ${t.code}`);
    if (isSubtask) {
      const parent = plan.tickets.find(p => p.agent.key === t.parent_key);
      invariant(parent && parent.epic === t.epic, `Invalid subtask parent: ${a.key}`);
      invariant(a.covers_parent_criteria?.length && a.covers_parent_criteria.every(id => parent.agent.acceptance_ids.includes(id)), `Invalid parent acceptance mapping: ${a.key}`);
    }
    invariant(!keys.has(a.key), `Duplicate Jira key ${a.key}`); keys.add(a.key);
    invariant(a.epic_key === manifest.issue_keys[t.epic], `Wrong epic for ${a.key}`);
    invariant(a.requirements_sha256 === requirementsHash(t), `Stale requirements hash: ${a.key}`);
    invariant(t.criteria.length > 0 && t.criteria.every(text), `Missing acceptance criteria: ${a.key}`);
    invariant(JSON.stringify(a.acceptance_ids) === JSON.stringify(t.criteria.map((_, i) => `AC-${i + 1}`)), `Invalid acceptance IDs: ${a.key}`);
    invariant(a.profiles.includes('common') && a.profiles.every(p => registry.profiles[p]), `Invalid check profile: ${a.key}`);
    invariant(['agent-ready', 'needs-refinement', 'agent-blocked'].includes(a.readiness.state) && text(a.readiness.reason), `Invalid readiness: ${a.key}`);
    invariant(a.out_of_scope.length && a.implementation_notes.length && a.verification_commands.length, `Incomplete contract: ${a.key}`);
    invariant(JSON.stringify(a.dependencies) === JSON.stringify(t.depends.map(c => codes.get(c)?.agent.key)), `Dependency keys differ: ${a.key}`);
    for (const code of t.depends) { invariant(codes.has(code), `Unknown dependency ${code}`); if (!isSubtask) links++; }
    for (const file of [...t.files, ...a.fixtures]) invariant(existsSync(join(root, file)), `Missing starting point/fixture: ${a.key}: ${file}`);
  }
  invariant(links === manifest.dependency_links, 'Dependency count differs from imported Jira links');
  const active = new Set(), done = new Set();
  const visit = code => {
    invariant(!active.has(code), `Dependency cycle at ${code}`);
    if (done.has(code)) return;
    active.add(code); for (const dependency of codes.get(code).depends) visit(dependency);
    active.delete(code); done.add(code);
  };
  for (const code of codes.keys()) visit(code);
  for (const [name, ids] of Object.entries(registry.profiles)) {
    invariant(ids.length > 0, `Empty profile ${name}`);
    for (const id of ids) {
      const definition = registry.checks[id];
      invariant(definition?.command?.length && definition.command.every(text), `Invalid check ${id}`);
    }
  }
  return {tickets: plan.tickets.length, subtasks: subtasks.length, epics: plan.epics.length, dependencies: links,
    subtask_dependencies: subtasks.reduce((count, t) => count + t.depends.length, 0),
    readiness: Object.fromEntries(unique(plan.tickets.map(t => t.agent.readiness.state)).map(s => [s, plan.tickets.filter(t => t.agent.readiness.state === s).length]))};
}

export function findTicket(project, key) {
  const ticket = [...project.plan.tickets, ...(project.plan.subtasks ?? [])].find(t => t.agent.key === key || t.code === key);
  invariant(ticket, `Unknown RDM ticket: ${key}`); return ticket;
}

export function requiredChecks(ticket, changedFiles, registry, extraProfiles = []) {
  const profiles = new Set(['common', ...(ticket?.agent.profiles ?? []), ...extraProfiles]);
  for (const rule of registry.impact_rules) {
    if (changedFiles.some(file => rule.prefixes.some(prefix => file.startsWith(prefix)))) {
      for (const profile of rule.profiles) profiles.add(profile);
    }
  }
  for (const profile of profiles) invariant(registry.profiles[profile], `Unknown check profile ${profile}`);
  return {profiles: [...profiles], ids: unique([...profiles].flatMap(p => registry.profiles[p]))};
}

function git(root, args, required = true) {
  const result = spawnSync('git', args, {cwd: root, encoding: 'utf8'});
  if (required) invariant(result.status === 0, `git ${args[0]} failed: ${result.stderr?.trim()}`);
  return result.status === 0 ? result.stdout.trim() : null;
}

export function revision(root, base = 'origin/main') {
  const head = git(root, ['rev-parse', 'HEAD']);
  const baseSha = git(root, ['rev-parse', base]);
  const mergeBase = git(root, ['merge-base', baseSha, head]);
  const changed = git(root, ['diff', '--name-only', `${mergeBase}..${head}`]);
  const dirty = git(root, ['status', '--porcelain', '--untracked-files=normal']);
  return {head_sha: head, base_sha: baseSha, merge_base: mergeBase,
    branch: git(root, ['branch', '--show-current']), clean: !dirty,
    changed_files: changed ? changed.split('\n') : []};
}

export function validateHandoff(handoff, {ticket, registry, headSha, baseSha, required, reports, phase = 'handoff'}) {
  invariant(['handoff', 'review', 'merge'].includes(phase), 'Unknown handoff gate phase');
  invariant(handoff.schema_version === 1, 'Unsupported handoff schema');
  invariant(handoff.issue_key === ticket.agent.key, 'Handoff is for another ticket');
  invariant(handoff.requirements_sha256 === requirementsHash(ticket), 'Requirements changed; refresh the live comparison and handoff');
  invariant(handoff.head_sha === headSha, 'Stale handoff: HEAD changed');
  invariant(handoff.base_sha === baseSha, 'Stale handoff: base changed');
  invariant(handoff.live_requirements?.compared === true && text(handoff.live_requirements.checked_at) && text(handoff.live_requirements.issue_url), 'Live Jira requirements comparison is missing');
  invariant(handoff.live_requirements.issue_url === `https://rdm-mudabbirulsaad.atlassian.net/browse/${ticket.agent.key}`, 'Wrong Jira site/ticket in live comparison');
  invariant(Array.isArray(handoff.acceptance) && handoff.acceptance.length === ticket.criteria.length, 'Acceptance evidence is incomplete');
  const results = new Map();
  for (const report of reports) {
    invariant(report.schema_version === 1 && report.head_sha === headSha && report.clean === true, 'Stale or dirty test report');
    invariant(report.base_sha === baseSha, 'Report base changed; retest the merge candidate');
    invariant(report.registry_sha256 === sha256(JSON.stringify(registry)), 'Check definitions changed; rerun checks');
    for (const check of report.checks ?? []) {
      const definition = registry.checks[check.id];
      invariant(!(required.includes(check.id) && check.status === 'failed'), `Required check failed: ${check.id}`);
      if (!definition || check.status !== 'passed') continue;
      invariant(check.exit_code === 0 && text(check.log_sha256), `No successful execution evidence for ${check.id}`);
      invariant(JSON.stringify(check.command) === JSON.stringify(definition.command), `Check command changed: ${check.id}`);
      invariant(!definition.environments || definition.environments.includes(report.environment), `Wrong environment for ${check.id}`);
      if (report.environment !== handoff.environment) invariant(/^https:\/\/github\.com\//.test(report.artifact_url ?? ''), 'External test evidence needs a durable GitHub artifact link');
      if (definition.interactive) invariant(check.interactive === true && report.environment === 'local', 'Interactive portal evidence must be genuinely local');
      results.set(check.id, check);
    }
  }
  for (const id of required) invariant(results.has(id), `Required check not passed: ${id}`);
  const ids = new Set();
  for (const criterion of handoff.acceptance) {
    invariant(ticket.agent.acceptance_ids.includes(criterion.id) && !ids.has(criterion.id), 'Unknown/duplicate acceptance criterion'); ids.add(criterion.id);
    invariant(criterion.status === 'passed' && text(criterion.explanation), `Acceptance not established: ${criterion.id}`);
    invariant(criterion.check_ids?.length && criterion.check_ids.every(id => results.has(id)), `Acceptance references missing/failed checks: ${criterion.id}`);
  }
  invariant(Array.isArray(handoff.limitations) && Array.isArray(handoff.migrations), 'Record limitations and migrations explicitly, including empty lists');
  if (phase === 'review' || phase === 'merge') {
    invariant(handoff.independent_review?.status === 'passed' && handoff.independent_review.head_sha === headSha && text(handoff.independent_review.summary), 'Independent review of the final revision is missing');
    invariant(handoff.independent_review.open_findings === 0, 'Unresolved review findings remain');
  }
  return {eligible: true, issue_key: handoff.issue_key, head_sha: headSha, required_checks: required};
}

export function reconcileCompletion(state) {
  invariant(text(state.issue_key) && text(state.pr_url), 'Missing ticket/PR identity');
  if (!state.merged) return {action: 'wait_for_merge', merge: false};
  invariant(text(state.merge_sha) && state.head_sha === state.expected_head_sha, 'Merged revision differs from the reviewed revision');
  if (state.post_merge?.sha !== state.merge_sha || state.post_merge?.status !== 'success') {
    return {action: state.jira_status === 'Done' ? 'investigate_done_without_post_merge_success' : 'wait_for_post_merge_checks', merge: false, merge_sha: state.merge_sha};
  }
  if (state.jira_status === 'Done' && state.jira_comment_exists === true) return {action: 'complete', merge: false, jira_write: false};
  return {action: 'reconcile_jira', merge: false, merge_sha: state.merge_sha,
    add_comment: state.jira_comment_exists !== true, transition_done: state.jira_status !== 'Done'};
}

function options(args) {
  const result = {_: []};
  for (let i = 0; i < args.length; i++) {
    if (args[i].startsWith('--')) {
      const name = args[i].slice(2);
      if (args[i + 1] && !args[i + 1].startsWith('--')) result[name] = args[++i];
      else result[name] = true;
    } else result._.push(args[i]);
  }
  return result;
}

function environmentDetails() {
  const run = (command, args) => {
    const result = spawnSync(command, args, {encoding: 'utf8'});
    return result.status === 0 ? result.stdout.trim().split('\n')[0] : null;
  };
  return {node: process.version, rust: run('cargo', ['--version']),
    python: run('python3', ['--version']), ffmpeg: run('ffmpeg', ['-version']),
    os: existsSync('/etc/os-release') ? readFileSync('/etc/os-release', 'utf8').split('\n').find(s => s.startsWith('PRETTY_NAME='))?.slice(12).replaceAll('"', '') : process.platform};
}

async function executeCheck(definition, {root, directory, target, environment, interactive}) {
  const log = join(directory, `${definition.id}.log`);
  if (definition.environments && !definition.environments.includes(environment)) {
    return {id: definition.id, command: definition.command, status: 'external-evidence-required', reason: definition.reason};
  }
  if (definition.interactive && !interactive) {
    return {id: definition.id, command: definition.command, status: 'not-run', reason: 'Requires --interactive and a real user portal grant; not a headless fixture.'};
  }
  const env = {...process.env, RDM_MEDIA_DIR: join(root, '.cache/media'),
    RDM_TEST_BACKEND: join(target, 'debug/rdm-backend'), RDM_DATA_DIR: join(directory, 'isolated-data'),
    RDM_SOCKET: join(directory, 'isolated.sock'), RDM_SCREENSHOT_DIR: join(directory, 'screenshots'),
    CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS ?? '2'};
  for (const [key, value] of Object.entries(definition.env ?? {})) env[key] = value.replaceAll('{target}', target);
  const started = new Date().toISOString();
  console.log(`\n[${definition.id}] ${definition.command.join(' ')}`);
  let output = '';
  const code = await new Promise(resolveCode => {
    const child = spawn(definition.command[0], definition.command.slice(1), {cwd: root, env, stdio: ['inherit', 'pipe', 'pipe']});
    child.stdout.on('data', chunk => {output += chunk; process.stdout.write(chunk);});
    child.stderr.on('data', chunk => {output += chunk; process.stderr.write(chunk);});
    child.on('error', error => {output += error.message + '\n';});
    child.on('close', code => resolveCode(code ?? 127));
  });
  writeFileSync(log, output);
  return {id: definition.id, command: definition.command, status: code === 0 ? 'passed' : 'failed',
    exit_code: code, started_at: started, finished_at: new Date().toISOString(), log,
    log_sha256: sha256(output), interactive: definition.interactive ? !!interactive : false};
}

export async function main(args = process.argv.slice(2), root = ROOT) {
  const command = args.shift(); const opt = options(args); const project = loadProject(root);
  if (command === 'validate') {
    console.log(JSON.stringify(validateContracts(project), null, 2)); return 0;
  }
  if (command === 'ticket') {
    const t = findTicket(project, opt._[0] ?? opt.ticket);
    console.log(JSON.stringify({...t, live_requirements: 'Fetch Jira and compare this snapshot before implementation; this command performs no network request.'}, null, 2)); return 0;
  }
  if (command === 'doctor') {
    const details = environmentDetails();
    const expectedNode = readFileSync(join(root, '.nvmrc'), 'utf8').trim();
    const expectedRust = readFileSync(join(root, 'rust-toolchain.toml'), 'utf8').match(/channel\s*=\s*"([^"]+)"/)[1];
    const checks = {node: details.node === `v${expectedNode}`, rust: details.rust?.includes(` ${expectedRust} `) ?? false,
      ffmpeg: !!details.ffmpeg, python: !!details.python,
      media_tools: ['yt-dlp', 'node'].every(name => existsSync(join(root, '.cache/media', name))),
      git: !!git(root, ['rev-parse', '--show-toplevel'], false),
      webkit_or_ubuntu24_builder: spawnSync('pkg-config', ['--exists', 'webkit2gtk-4.1']).status === 0 ||
        spawnSync('docker', ['image', 'inspect', 'rdm-builder:ubuntu24'], {stdio: 'ignore'}).status === 0};
    console.log(JSON.stringify({environment: details, checks,
      connections: 'Verify GitHub push/PR/merge and the RDM Jira site through the signed-in tools. Local gh/browser sign-in does not establish cloud access.'}, null, 2));
    return Object.values(checks).every(Boolean) ? 0 : 1;
  }
  if (command === 'check') {
    const ticket = opt.ticket ? findTicket(project, opt.ticket) : null;
    const state = revision(root, opt.base ?? 'origin/main');
    invariant(state.clean || opt['allow-dirty'], 'Commit changes before final checks (or use --allow-dirty for a provisional report)');
    const environment = opt.environment ?? 'local';
    invariant(['local', 'cloud', 'ci'].includes(environment), 'Unknown environment');
    const selected = requiredChecks(ticket, state.changed_files, project.registry, opt.profiles ? opt.profiles.split(',') : []);
    const outputRoot = join(root, '.agent-runs'); mkdirSync(outputRoot, {recursive: true});
    const directory = mkdtempSync(join(outputRoot, `${ticket?.agent.key ?? 'checks'}-`));
    const target = resolve(root, process.env.CARGO_TARGET_DIR ?? 'target');
    const report = {schema_version: 1, issue_key: ticket?.agent.key ?? null,
      ...state, environment, environment_versions: environmentDetails(), profiles: selected.profiles,
      registry_sha256: sha256(JSON.stringify(project.registry)), required_checks: selected.ids, checks: []};
    const reportPath = join(directory, 'checks.json');
    for (const id of selected.ids) {
      report.checks.push(await executeCheck({...project.registry.checks[id], id}, {root, directory, target, environment, interactive: opt.interactive}));
      writeFileSync(reportPath, JSON.stringify(report, null, 2) + '\n');
      if (report.checks.at(-1).status === 'failed') break;
    }
    const final = revision(root, opt.base ?? 'origin/main');
    if (!final.clean || final.head_sha !== state.head_sha || final.base_sha !== state.base_sha) report.clean = false;
    writeFileSync(reportPath, JSON.stringify(report, null, 2) + '\n');
    console.log(`\nEvidence report: ${reportPath}`);
    const acceptable = selected.ids.every(id => report.checks.some(c => c.id === id &&
      (c.status === 'passed' || (opt['allow-external'] && environment !== 'local' && c.status === 'external-evidence-required'))));
    return acceptable && report.clean ? 0 : 1;
  }
  if (command === 'handoff') {
    const action = opt._[0];
    if (action === 'reconcile') {console.log(JSON.stringify(reconcileCompletion(readJson(resolve(opt.state))), null, 2)); return 0;}
    const ticket = findTicket(project, opt.ticket);
    if (action === 'init') {
      const report = readJson(resolve(opt.report));
      const handoff = {schema_version: 1, issue_key: ticket.agent.key,
        requirements_sha256: requirementsHash(ticket), head_sha: report.head_sha,
        base_sha: report.base_sha, environment: report.environment, pr_url: '',
        live_requirements: {compared: false, checked_at: '', issue_url: `https://rdm-mudabbirulsaad.atlassian.net/browse/${ticket.agent.key}`},
        reports: [resolve(opt.report)], acceptance: ticket.criteria.map((criterion, i) => ({id: `AC-${i + 1}`, criterion, status: 'not-run', explanation: '', check_ids: []})),
        limitations: [], migrations: [], independent_review: {status: 'not-run', head_sha: '', open_findings: null, summary: ''}};
      const output = opt.output ? resolve(opt.output) : join(dirname(resolve(opt.report)), 'handoff.json');
      writeFileSync(output, JSON.stringify(handoff, null, 2) + '\n'); console.log(output); return 0;
    }
    invariant(action === 'gate', 'Use handoff init, gate or reconcile');
    const handoff = readJson(resolve(opt.handoff));
    const state = revision(root, opt.base ?? 'origin/main'); invariant(state.clean, 'Working tree must be clean');
    const selected = requiredChecks(ticket, state.changed_files, project.registry);
    const reports = handoff.reports.map(path => readJson(isAbsolute(path) ? path : resolve(dirname(resolve(opt.handoff)), path)));
    console.log(JSON.stringify(validateHandoff(handoff, {ticket, registry: project.registry,
      headSha: state.head_sha, baseSha: state.base_sha, required: selected.ids, reports,
      phase: opt.phase ?? 'handoff'}), null, 2)); return 0;
  }
  throw new Error('Commands: validate | ticket RDM-KEY | doctor | check --ticket RDM-KEY | handoff init/gate/reconcile');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then(code => {process.exitCode = code;}).catch(error => {console.error(error.message); process.exitCode = 1;});
}
