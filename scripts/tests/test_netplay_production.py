import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location('production_runner', SCRIPTS / 'test-netplay-production.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
import netplay_production_evidence as evidence
from netplay_production_processes import Processes


class EvidenceTests(unittest.TestCase):
    def test_mixed_signer_emits_format_three_control_field(self):
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            candidate, _ = self.make_candidate(root)
            members = [{'source': '55' * 32, 'artifact': '66' * 32,
                        'target': 'x86_64-pc-windows-msvc'},
                       {'source': '55' * 32, 'artifact': '77' * 32,
                        'target': 'wasm32-unknown-unknown'}]
            key = Ed25519PrivateKey.generate()
            path = root / 'key.bin'
            path.write_bytes(key.private_bytes_raw())
            with patch.object(runner, 'PUBLIC_KEY', key.public_key().public_bytes_raw().hex()):
                encoded, _ = runner.sign(members, candidate['games'], path, 'candidate', {}, None)
            envelope = json.loads(encoded)
            key.public_key().verify(bytes.fromhex(envelope['signature']),
                                    runner.DOMAIN + envelope['payload'].encode())
            payload = json.loads(envelope['payload'])
            self.assertEqual(payload['format'], 3)
            self.assertEqual(payload['fp_controls'], 0)
            self.assertEqual(payload['members'], members)

    def quiet_report(self):
        return {'options': {'timing': 0, 'frames': 300}, 'finite': True,
                'restored': True, 'arithmetic': {'subnormal_count': 75},
                'subnormal_count': 100, 'tail_peak_bits': 1, 'peak_bits': 0x3f000000,
                'trace': [None] * 300, 'continuation_frames': 32,
                'tail_continuation_frames': 32}

    def test_arithmetic_alone_does_not_establish_pcm_subnormals(self):
        report = dict(self.quiet_report(), subnormal_count=0)
        with self.assertRaisesRegex(ValueError, 'PCM coverage absent'):
            runner.quiet_pair(report, report, report['options'])

    def test_zero_or_normal_tail_does_not_establish_quiet_subnormal_pcm(self):
        for bits in (0, 0x00800000):
            report = dict(self.quiet_report(), tail_peak_bits=bits)
            with self.assertRaisesRegex(ValueError, 'tail is not subnormal'):
                runner.quiet_pair(report, report, report['options'])

    @unittest.skipUnless(os.name == 'nt', 'Windows process-tree invariant')
    def test_job_closes_started_child_tree(self):
        owners = Processes()
        code = "import subprocess,sys,time; sys.stdin.readline(); p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); print(p.pid,flush=True); time.sleep(60)"
        process = subprocess.Popen([sys.executable, '-c', code], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, text=True,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            owners.add(process)
            process.stdin.write('start\n')
            process.stdin.flush()
            self.assertGreater(int(process.stdout.readline()), 0)
            self.assertTrue(owners.close())
            self.assertIsNotNone(process.poll())
        finally:
            if owners.handle:
                owners.close()
            process.stdin.close()
            process.stdout.close()

    def make_candidate(self, root):
        cases = []
        games = [{'source': str(timing + 1) * 64, 'mapper': 34, 'submapper': 0,
                  'timing': 2 if timing == 3 else timing} for timing in (0, 1, 3)]
        for timing in (0, 1, 3):
            source = str(timing + 1) * 64
            for delay in (0, 2):
                for host in (True, False):
                    path = root / ('case-' + str(len(cases)) + '.json')
                    path.write_text(json.dumps({'source_sha256': source, 'options': {'timing': timing, 'delay': delay,
                                    'native_host': host, 'frames': 72}}))
                    cases.append({'passed': True, 'game_source': source, 'options': {'timing': timing, 'delay': delay,
                                  'native_host': host, 'frames': 72}, 'report': path.name,
                                  'sha256': evidence.digest(path)})
        quiet = []
        for timing in (0, 1, 3):
            path = root / ('quiet-' + str(timing) + '.json')
            path.write_text(json.dumps({side: {'options': {'timing': timing, 'frames': 300}}
                                        for side in ('native', 'browser')}))
            quiet.append({'timing': timing, 'passed': True, 'report': path.name,
                          'sha256': evidence.digest(path)})
        audit = {'module_sha256': '11' * 32, 'bundle_identity': '22' * 32,
                 'validator_sha256': '33' * 32, 'validator_version': 'fixture',
                 'features': evidence.FEATURES, 'exit': 0, 'passed': True}
        value = {'passed': True, 'accepted': False, 'stage': 'candidate', 'members': [],
                 'games': games, 'certificate': '44' * 32, 'cases': cases, 'quiet': quiet,
                 'wasm_audit': audit, 'cleanup': {'owned_processes_closed': True, 'errors': []}}
        return value, audit

    def check(self, root, value, audit):
        path, review = root / 'qualification.json', root / 'review.json'
        path.write_bytes(evidence.canonical(value))
        review.write_bytes(evidence.canonical({'passed': True, 'independent': True,
            'evidence_sha256': hashlib.sha256(evidence.canonical(value)).hexdigest()}))
        return evidence.reviewed(path, review, [], value['games'], audit, lambda *_: None, lambda *_: None)

    def test_passed_candidate_consumed_without_relabel(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value, audit = self.make_candidate(root)
            self.assertFalse(self.check(root, value, audit)[0]['accepted'])

    def test_duplicate_case_does_not_establish_full_coverage(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value, audit = self.make_candidate(root)
            value['cases'][1] = value['cases'][0]
            with self.assertRaisesRegex(ValueError, 'Cartesian'):
                self.check(root, value, audit)

    def test_review_does_not_accept_changed_receipt_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value, audit = self.make_candidate(root)
            (root / value['cases'][0]['report']).write_text('{"changed":true}')
            with self.assertRaisesRegex(ValueError, 'receipt bytes changed'):
                self.check(root, value, audit)

    def test_unreviewed_wasm_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value, audit = self.make_candidate(root)
            current = dict(audit, module_sha256='55' * 32)
            with self.assertRaisesRegex(ValueError, 'audit differs'):
                self.check(root, value, current)

    def test_relabelled_passed_case_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value, audit = self.make_candidate(root)
            value['cases'][1]['report'] = value['cases'][0]['report']
            value['cases'][1]['sha256'] = value['cases'][0]['sha256']
            with self.assertRaisesRegex(ValueError, 'options differ'):
                self.check(root, value, audit)

    def test_same_options_cannot_relabel_a_different_game(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value, audit = self.make_candidate(root)
            value['cases'][0]['game_source'] = value['games'][1]['source']
            value['cases'][0]['options']['timing'] = 1
            with self.assertRaisesRegex(ValueError, 'options differ|source differs'):
                self.check(root, value, audit)


if __name__ == '__main__':
    unittest.main()
