#!/usr/bin/env python3
"""Maintain execution contracts and update payloads without changing Jira identities.

Re-run after deliberate contract changes; this never writes to Jira itself.
"""
import csv
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PLAN = ROOT / 'planning/jira-backlog.json'

# Profiles cover behavior; diff-based checks can add obligations at execution time.
PROFILES = {
    'B01': ['http', 'media', 'desktop', 'browser'], 'B02': ['http', 'media', 'browser'],
    'B03': ['media', 'browser'], 'B04': ['http', 'media'],
    'B05': ['http', 'media', 'desktop'], 'B06': ['http', 'media', 'desktop', 'package'],
    'B07': ['browser', 'desktop', 'package'], 'B08': ['http', 'desktop'],
    'B09': ['http', 'desktop'], 'B10': ['http', 'media', 'desktop'],
    'B11': ['http', 'desktop'], 'B12': ['browser'], 'B13': ['http', 'browser', 'desktop'],
    'B14': ['media', 'desktop'], 'B15': ['media', 'desktop', 'browser'],
    'B16': ['http', 'media', 'desktop'], 'B17': ['http', 'media', 'desktop'],
    'B18': ['http', 'media'], 'B19': ['desktop'], 'B20': ['http', 'desktop'],
    'B21': ['http', 'desktop'], 'B22': ['package', 'browser', 'media', 'desktop'],
    'B23': ['http', 'media', 'desktop', 'browser'], 'B24': ['http', 'desktop'],
    'B25': ['http', 'media', 'desktop'], 'B26': ['media', 'desktop'],
    'B27': ['http', 'media'], 'B28': ['desktop'], 'B29': ['http', 'desktop'],
    'B30': ['http', 'desktop'], 'B31': ['http', 'desktop'],
    'B32': ['http', 'media', 'desktop'], 'B33': ['desktop'], 'B34': ['http'],
    'B35': ['browser', 'package'], 'B36': ['package'],
    'B37': ['http', 'media', 'desktop', 'browser', 'package'],
    'B38': ['http', 'media', 'desktop', 'browser', 'package'],
    'B39': ['media', 'browser'], 'B40': []
}

NOTES = {
 'B01': ('Refresh the existing job only after file identity validation; preserve idempotency and durable ranges.', 'Replacing a source is not permission to append bytes from a different representation.'),
 'B02': ('Persist only explicitly allowlisted public metadata, separate from the in-memory credential context.', 'Do not persist cookies, bearer tokens, signed page-query parameters or browser profile databases.'),
 'B03': ('Use frame/player/navigation identity in candidate selection; an ambiguous selection needs an explicit choice.', 'Do not select an advertisement, individual fragment, blob URL or arbitrary parent HTML as a complete video.'),
 'B04': ('Carry expected content intent through typed job state; inspect unexpected HTML and verify final media before atomic completion.', 'Explicit HTML downloads remain valid. Failed media validation must retain recovery files and never publish a completed filename.'),
 'B05': ('Classify retryable transport failures separately from authentication/content changes; make backoff cancellable.', 'No busy retry loop, automatic unsafe append, or implied login-startup change.'),
 'B06': ('Use the selected destination filesystem for space checks, with media merge overhead; cleanup only identified RDM-owned partials.', 'Do not delete partials, completed files or unrelated files without the user-selected cleanup operation.'),
 'B07': ('Separate version/connection diagnostics, reproducible extension packaging and external store/signing approval.', 'Store credentials and publication approval are external dependencies, not inferred by a coding agent.'),
 'B08': ('Apply filesystem changes without overwriting; update persistent paths only after a successful move/rename.', 'History removal must not become implicit disk deletion.'),
 'B09': ('Reuse validated metadata probing; distinguish a per-job held state from disabling a shared queue.', 'Preview must not silently create/start a durable transfer.'),
 'B10': ('Persist explicit ordering and priority; validate scheduler fairness under queue, global and origin ceilings.', 'Deleting a queue does not remove files or silently orphan jobs.'),
 'B11': ('Add ordered rules and a match preview on top of destination validation; preserve manual overrides.', 'Changing a category rule must not silently move existing files.'),
 'B12': ('Keep capture-policy decisions separate from optional cookie permissions and approved GET handoffs.', 'Never infer credential consent from content-script access or cancel before durable acknowledgement.'),
 'B13': ('Resolve relative page links, preview a bounded selected list and reuse durable handoff correlation.', 'No recursive crawl, POST generation or unbounded numeric pattern expansion in this ticket.'),
 'B14': ('Expose actual provider format metadata and selected format identity; validate the final streams against the selection.', 'A quality cap is not an exact format and an unavailable selection must not be silently misreported.'),
 'B15': ('Persist useful source/title metadata and report measured fragments/stages without fabricating byte totals.', 'Do not remove signed parameters from the actual media URL or invent a container extension.'),
 'B16': ('Bind credential/header profiles to origins and store secrets only in the explicitly enabled keyring.', 'No global Authorization header, insecure downgrade leakage or secret-bearing exports.'),
 'B17': ('Define explicit versus environment proxy selection, exclusions and a shared route for both transfer engines.', 'Browser-only VPN access is not automatically shared by the backend.'),
 'B18': ('Coordinate live rate changes across both engines and measure aggregate limits and fairness.', 'Do not claim exact throughput or universal acceleration from UI counters alone.'),
 'B19': ('Use Tauri/Linux tray and notification APIs with stable job identifiers; tie inhibitors to active work.', 'No default sleep inhibition, shutdown action or replacement of desktop-session settings.'),
 'B20': ('Migrate SQLite compatibly, use indexed stable pagination and restore only into an isolated test directory.', 'No destructive retention default or ephemeral credentials in backups.'),
 'B21': ('Keep expected and measured digests distinct; preserve mismatch state and unambiguous checksum-list matching.', 'A locally computed digest without an expected value must not be called Verified.'),
 'B22': ('Separate trusted update verification, recovery coordination and distribution/signing operations.', 'Do not bypass signature validation or publish with credentials obtained from repository files.'),
 'B23': ('Preserve structured causes locally while exporting a deliberately redacted report with actionable recovery.', 'No complete URLs/query secrets, cookies or unnecessary browsing history in exported diagnostics.'),
 'B24': ('Model time-zone/window semantics explicitly and test missed windows, weekdays and daylight-saving changes.', 'Power/completion actions remain opt-in and manual stopping is not queue completion.'),
 'B25': ('Define persistent accounting windows, policy precedence and user overrides before implementation.', 'Do not invent quota limits, battery thresholds or metered-network defaults.'),
 'B26': ('Separate media options/metadata, selected playlist jobs and output processing into reviewable changes.', 'No automatic whole-playlist download or implicit lossy conversion.'),
 'B27': ('Retain baseline measurements, controlled latency/rate-limit fixtures and negative speedup results.', 'Never trade range/durability/integrity guarantees for a faster benchmark.'),
 'B28': ('Use supported desktop clipboard/drop APIs and preview links with the existing Add flow.', 'Do not retain unrelated clipboard data or enable clipboard monitoring by default.'),
 'B29': ('Add transport adapters under the same job lifecycle and scoped credential model.', 'SFTP is a separate follow-up; no certificate bypass or silently changed transport.'),
 'B30': ('Separate crawl planning/bounds, retrieval/state, offline rewriting and the user selection interface.', 'No unlimited crawl or automatic replacement of existing offline content.'),
 'B31': ('Parse bounded ZIP metadata from validated ranges, with explicit unsupported/encrypted outcomes.', 'Preview must not extract, execute entries or fetch an unbounded central directory.'),
 'B32': ('Use opt-in executable/action profiles with argument arrays and distinct scan versus transfer outcomes.', 'No shell interpolation, automatic execution of downloads or deletion inferred from a scanner exit code.'),
 'B33': ('Keep persisted layout preferences distinct from data sorting; test keyboard workflows and expanded translations.', 'Do not let localization/layout changes hide service or recovery actions.'),
 'B34': ('Implement human and machine CLI adapters over the existing private service protocol.', 'Do not widen browser native-host administrative permissions to match the local CLI.'),
 'B35': ('Build a tested browser/profile matrix using supported registration and disposable profiles.', 'Do not edit protected browser preferences or call an untested sandbox/portal Supported.'),
 'B36': ('Publish original source with pinned toolchains/lockfiles and reproducible notices/provenance.', 'Exclude the proprietary installer, local capture artifacts, caches, credentials and generated binaries.'),
 'B37': ('Use shared check definitions for CI and retain exact-commit artifacts on both Ubuntu runtime baselines.', 'A headless browser fixture is not a real Firefox Snap portal permission grant.'),
 'B38': ('Tie each release acceptance scenario to controlled fixtures and installed-package evidence at the release revision.', 'Do not close this gate while prerequisite features or required environment checks remain incomplete.'),
 'B39': ('Publish dated method/version evidence per workflow and distinguish deterministic fixtures from live sites.', 'A visible overlay or successful extraction probe alone is not a full-download compatibility claim.'),
 'B40': ('Maintain priority versus readiness versus status distinctions and explicit evidence-based completion.', 'Do not mark existing 0.2.2 behavior as newly implemented or make P2 expansion a 1.0 release blocker.')
}

REFINE = {
 'B07': 'Permanent distribution requires external signing/store ownership; use separate diagnostic, packaging and approval subtasks.',
 'B11': 'Confirm rule precedence and migration/override behavior before marking ready.',
 'B17': 'Confirm proxy precedence and supported SOCKS authentication combinations.',
 'B18': 'Define the measured bandwidth tolerance and fairness acceptance fixture.',
 'B22': 'Distribution channel, signing identity and release credentials require explicit configuration.',
 'B24': 'Confirm time-zone, missed-window and completion-action semantics.',
 'B25': 'Quota window, metered/battery policy precedence and defaults require refinement.',
 'B26': 'Split metadata/options, playlist job lifecycle and optional output processing into independent subtasks.',
 'B29': 'Finalize supported FTP/FTPS security/resume contract and fixture server setup.',
 'B30': 'Split bounded crawl engine, offline rewriting and selection UI; confirm crawl limits and synchronization policy.',
 'B32': 'Confirm scanner/action policy and user-selected failure handling.',
 'B35': 'Confirm which additional browsers/profiles are available for real verification.'
}

def semantic_hash(ticket):
    value = {k: ticket[k] for k in ['title', 'context', 'scope', 'criteria', 'depends']}
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()

def description(ticket):
    a = ticket['agent']
    sections = [ticket['context'], 'Outcome\n' + ticket['scope'],
        'Boundaries\n' + '\n'.join(a['out_of_scope']),
        'Starting points\n' + '\n'.join(ticket['files'] + a['fixtures']),
        'Contracts\n' + '\n'.join(a['implementation_notes']),
        'Dependencies\n' + (', '.join(a['dependencies']) or 'None; confirm live Jira before starting.'),
        'Acceptance criteria\n' + '\n'.join(f'AC-{i+1}: {text}' for i, text in enumerate(ticket['criteria'])),
        'Verification\n' + '\n'.join(a['verification_commands']),
        'Required local evidence\n' + ('; '.join(a['local_evidence']) or 'No additional interactive local check declared; diff impact may add checks.'),
        'Agent readiness\n' + a['readiness']['state'] + ': ' + a['readiness']['reason'],
        'Delivery\nOne linked PR; exact base/head SHAs; live requirements comparison; AC-by-AC evidence; actual check logs, environment versions, limitations and migration notes. Use the committed rdm-implement-ticket and rdm-review-ticket skills. In Review only after the handoff gate passes; Done only after merge and passing post-merge checks.',
        f"Execution contract v{a['version']} | {a['key']} | planning ID {ticket['code']}\nRequirements SHA-256: {a['requirements_sha256']}\nSource: https://github.com/MudabbirulSaad/rust-download-manager\nJira owns live requirements and status. Historical evidence is not a new revision's test result."]
    return '\n\n'.join(sections)

def main():
    plan = json.loads(PLAN.read_text())
    manifest = json.loads((ROOT / 'planning/jira-import-manifest.json').read_text())
    plan['agent_contract_version'] = 1
    plan['repository'] = 'https://github.com/MudabbirulSaad/rust-download-manager'
    for t in plan['tickets']:
        key = manifest['issue_keys'][t['code']]
        profiles = ['common'] + PROFILES[t['code']]
        fixtures = ['tests/agent-workflow.test.mjs']
        for profile, files in {
            'http': ['tests/acceptance.py', 'crates/core/tests/state.rs'],
            'media': ['tests/media_acceptance.py', 'tests/media.test.ts'],
            'browser': ['tests/browser_chromium.mjs', 'tests/browser_firefox_snap.py', 'tests/extension.test.ts'],
            'desktop': ['tests/App.test.tsx', 'tests/desktop_webdriver.py'],
            'package': ['tests/io_failures.py', 'packaging/Dockerfile.runtime']
        }.items():
            if profile in profiles: fixtures += files
        deps = [manifest['issue_keys'][c] for c in t['depends']]
        if t['code'] in REFINE:
            state, reason = 'needs-refinement', REFINE[t['code']]
        elif deps:
            state, reason = 'agent-blocked', 'Prerequisites must be delivered: ' + ', '.join(deps)
        else:
            state, reason = 'agent-ready', 'No implementation dependencies; confirm live issue and environment at assignment.'
        t['agent'] = {
            'version': 1, 'key': key, 'epic_key': manifest['issue_keys'][t['epic']],
            'requirements_sha256': semantic_hash(t), 'dependencies': deps,
            'profiles': profiles, 'starting_files': t['files'], 'fixtures': fixtures,
            'implementation_notes': [NOTES[t['code']][0], 'Keep shared Rust/TypeScript interfaces and existing SQLite histories compatible; use versioned/defaulted fields as applicable.'],
            'out_of_scope': [NOTES[t['code']][1], 'Unrelated roadmap features and release publication/host installation unless explicitly assigned.'],
            'verification_commands': [f'npm run agent:ticket -- {key}', 'npm run agent:validate',
                f'npm run agent:check -- --ticket {key} --environment local --base origin/main',
                f'npm run agent:check -- --ticket {key} --environment cloud --base origin/main'],
            'local_evidence': (['brave-handoff: actual local Brave extension/native-host tests at the final head SHA',
                'firefox-snap-handoff: actual local Firefox Snap portal test at the final head SHA; permission grant is interactive'] if 'browser' in profiles else []) +
                (['package-matrix: installed Ubuntu 24.04/26.04 results at the final head SHA, produced locally or by CI'] if 'package' in profiles else []),
            'readiness': {'state': state, 'reason': reason},
            'acceptance_ids': [f'AC-{i+1}' for i in range(len(t['criteria']))]
        }
    PLAN.write_text(json.dumps(plan, indent=2, ensure_ascii=False) + '\n')
    payloads = [{'key': t['agent']['key'], 'summary': t['title'], 'description': description(t),
        'add_labels': [t['agent']['readiness']['state']],
        'remove_labels': ['agent-ready', 'needs-refinement', 'agent-blocked', 'agent-review-ready']}
        for t in plan['tickets']]
    (ROOT / 'planning/jira-agent-updates.json').write_text(json.dumps(payloads, indent=2, ensure_ascii=False) + '\n')
    with (ROOT / 'planning/jira-agent-updates.csv').open('w', newline='') as out:
        writer = csv.writer(out, lineterminator='\n')
        writer.writerow(['Issue key', 'Summary', 'Description', 'Labels', 'Labels', 'Labels'])
        for t in plan['tickets']:
            writer.writerow([t['agent']['key'], t['title'], description(t),
                t['tier'].lower(), 'rdm-' + t['code'].lower(), t['agent']['readiness']['state']])
    print(f"Prepared {len(payloads)} existing-ticket execution contracts; no Jira writes performed.")

if __name__ == '__main__':
    main()
