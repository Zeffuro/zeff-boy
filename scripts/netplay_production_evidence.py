import hashlib
import json
from pathlib import Path
import re
import subprocess


FEATURES = '-all,mvp,mutable-global,saturating-float-to-int,sign-extension,reference-types,multi-value'


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require(value, message):
    if not value:
        raise ValueError(message)


def audit(dist, validator, output):
    modules = list(dist.glob('*.wasm'))
    require(len(modules) == 1, 'expected exactly one final WASM module')
    module = modules[0]
    html = (dist / 'index.html').read_text(encoding='utf-8')
    embedded = re.search(r'const manifest = (\{[^\n]+\});', html)
    require(embedded is not None, 'sealed manifest unavailable')
    manifest = json.loads(embedded.group(1))
    require(manifest['files'].get(module.name) == digest(module), 'audited WASM differs from sealed manifest')
    bundle_identity = hashlib.sha256(json.dumps(manifest, separators=(',', ':')).encode()).hexdigest()
    help_text = subprocess.check_output([str(validator), 'validate', '--help'], text=True, timeout=15)
    require('--features' in help_text and '-all' in help_text, 'validator feature controls unavailable')
    version = subprocess.check_output([str(validator), '--version'], text=True, timeout=15).strip()
    arguments = ['validate', '--features=' + FEATURES, str(module)]
    result = subprocess.run([str(validator), *arguments], capture_output=True, timeout=120)
    (output / 'wasm-validation.log').write_bytes(result.stdout + result.stderr)
    receipt = {'module_sha256': digest(module), 'validator_sha256': digest(validator),
               'bundle_identity': bundle_identity,
               'validator_version': version, 'argv': arguments, 'features': FEATURES,
               'exit': result.returncode, 'passed': result.returncode == 0}
    (output / 'wasm-audit.json').write_bytes(canonical(receipt) + b'\n')
    require(receipt['passed'], 'final emitted WASM violates allowed instruction proposals')
    return receipt


def audited_same(left, right):
    keys = ('module_sha256', 'bundle_identity', 'validator_sha256', 'validator_version', 'features', 'exit', 'passed')
    require(all(left[key] == right[key] for key in keys), 'reviewed emitted-module audit differs')
    require(left['features'] == FEATURES and left['exit'] == 0 and left['passed'] is True,
            'instruction audit did not pass exact allowed proposal set')


def read_bound(root, reference):
    path = (root / reference['report']).resolve()
    require(path.is_relative_to(root.resolve()) and path.is_file(), 'receipt path outside evidence directory')
    require(digest(path) == reference['sha256'], 'reviewed receipt bytes changed')
    return json.loads(path.read_text(encoding='utf-8'))


def reviewed(evidence_path, review_path, members, games, current_audit, check_case, check_quiet):
    from netplay_production_media import coverage
    require(evidence_path is not None and review_path is not None, 'accepted signing requires candidate evidence and review')
    evidence = json.loads(evidence_path.read_text(encoding='utf-8'))
    review = json.loads(review_path.read_text(encoding='utf-8'))
    require(evidence.get('passed') is True and evidence.get('accepted') is False
            and evidence.get('stage') == 'candidate', 'passed experimental candidate evidence required')
    require(evidence['members'] == members and evidence['games'] == games, 'reviewed artifact coverage differs')
    require(review.get('passed') is True and review.get('independent') is True, 'independent review did not pass')
    require(review.get('evidence_sha256') == hashlib.sha256(canonical(evidence)).hexdigest(),
            'independent review does not bind candidate evidence')
    audited_same(evidence['wasm_audit'], current_audit)
    wanted = coverage(games)
    cases = evidence.get('cases', [])
    require(len(cases) == len(wanted), 'gameplay coverage incomplete')
    actual = []
    for case in cases:
        options = case['options']
        require(case.get('passed') is True and type(options['native_host']) is bool, 'invalid case receipt')
        game = next((game for game in games if game['source'] == case.get('game_source')), None)
        require(game is not None, 'reviewed game coverage differs')
        actual.append((game['source'], options['delay'], options['native_host']))
        require(options['timing'] == (3 if game['timing'] == 2 else game['timing']), 'reviewed game timing differs')
        minimum = 72 if game in games[:3] else 240
        require(minimum <= options['frames'] <= 1000, 'reviewed frame coverage outside bounds')
        report = read_bound(evidence_path.parent, case)
        require(report.get('options') == options, 'gameplay receipt options differ from ledger')
        require(report.get('source_sha256') == game['source'], 'gameplay receipt source differs from ledger')
        check_case(report, members, evidence['certificate'], options, game)
    require(set(actual) == wanted and len(set(actual)) == len(wanted), 'gameplay Cartesian coverage differs')
    quiet = evidence.get('quiet', [])
    require(len(quiet) == 3 and {entry['timing'] for entry in quiet} == {0, 1, 3}, 'quiet coverage differs')
    for entry in quiet:
        require(entry.get('passed') is True, 'quiet case did not pass')
        report = read_bound(evidence_path.parent, entry)
        options = {'timing': entry['timing'], 'frames': 300}
        require(report['native'].get('options') == options and report['browser'].get('options') == options,
                'quiet receipt options differ from ledger')
        check_quiet(report['native'], report['browser'], options)
    require(evidence['cleanup']['owned_processes_closed'] is True and not evidence['cleanup']['errors'],
            'candidate cleanup did not pass')
    return evidence, review
