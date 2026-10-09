import argparse
import functools
import hashlib
import http.server
import json
import os
import re
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.request
import urllib.parse

from netplay_production_evidence import audit, reviewed
from netplay_production_processes import Processes
from netplay_production_media import selected, coverage


ORIGIN = 'http://127.0.0.1:47181'
NATIVE = 'http://127.0.0.1:47180'
DOMAIN = b'ZeffNetplay-ArtifactPair/v1\0'
PUBLIC_KEY = 'c0c3e9303eccde66c0e207a014884b05b660b1f60ac9a16b9f28d1a3e5c12cba'
MEMBER_KEYS = ('artifact', 'source', 'version', 'target', 'test', 'profile',
               'opt_level', 'debug', 'compiler', 'target_features')


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def write(path, value):
    with path.open('x', encoding='utf-8') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def request(url, body=None, method=None, timeout=5):
    headers = {'Origin': ORIGIN, 'Content-Type': 'application/json'}
    data = canonical(body) if body is not None else None
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            data = response.read(16 * 1024 * 1024 + 1)
    except urllib.error.HTTPError as error:
        raise ValueError('qualification HTTP ' + str(error.code) + ': ' +
                         error.read(4096).decode(errors='replace')) from error
    require(len(data) <= 16 * 1024 * 1024, 'HTTP response exceeds bound')
    return data.decode()


def wait_ready(check, processes, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        require(all(process.poll() is None for process in processes), 'owned process exited')
        try:
            if check():
                return
        except (OSError, urllib.error.URLError):
            pass
        time.sleep(.1)
    raise TimeoutError('readiness deadline exceeded')


class Handler(http.server.SimpleHTTPRequestHandler):
    sidecar = None
    prefix = '/'

    def translate_path(self, path):
        if path.startswith(self.prefix):
            path = '/' + path[len(self.prefix):]
        return super().translate_path(path)

    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        self.send_header('Cache-Control', 'no-store')
        super().end_headers()

    def do_GET(self):
        if urllib.parse.urlsplit(self.path).path == self.prefix + 'zeff-boy.netplay.json':
            if self.sidecar is None:
                self.send_error(404)
            else:
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(self.sidecar)))
                self.end_headers()
                self.wfile.write(self.sidecar)
            return
        super().do_GET()

    def log_message(self, *_args):
        pass


class Browser:
    def __init__(self, url, edge, profile):
        self.url = url
        caps = {'capabilities': {'alwaysMatch': {'browserName': 'MicrosoftEdge',
                'ms:edgeOptions': {'binary': str(edge), 'args': [
                    '--headless=new', '--no-first-run', '--no-default-browser-check',
                    '--enable-unsafe-webgpu', '--use-webgpu-adapter=swiftshader',
                    '--use-gpu-in-tests', '--user-data-dir=' + str(profile)]}}}}
        self.session = json.loads(request(url + '/session', caps, timeout=30))['value']['sessionId']
        try:
            self.call('/timeouts', {'script': 120000, 'pageLoad': 60000})
        except BaseException:
            try:
                self.close()
            except Exception:
                pass
            raise

    def call(self, path, body=None, method=None, timeout=130):
        value = json.loads(request(self.url + '/session/' + self.session + path,
                                   body, method, timeout))['value']
        require(not isinstance(value, dict) or 'error' not in value, 'webdriver error: ' + str(value))
        return value

    def load(self):
        self.call('/url', {'url': ORIGIN + '/?netplay-qualification=' + secrets.token_hex(8)})
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if self.call('/execute/sync', {'script': 'return !!globalThis.zeffBoyQualification;', 'args': []}):
                return
            time.sleep(.2)
        raise TimeoutError('sealed qualification exports unavailable')

    def execute(self, name, options=None):
        script = '''const done=arguments[arguments.length-1];
Promise.resolve().then(()=>globalThis.zeffBoyQualification[arguments[0]](
    arguments[1]===null?undefined:JSON.stringify(arguments[1])))
.then(value=>done({ok:true,value}),error=>done({ok:false,error:String(error)}));'''
        value = self.call('/execute/async', {'script': script, 'args': [name, options]})
        require(value.get('ok') is True, 'browser qualification: ' + str(value))
        return json.loads(value['value'])

    def close(self):
        self.call('', method='DELETE', timeout=15)


def member(facts):
    result = {key: facts[key] for key in MEMBER_KEYS}
    require(result['test'] is False, 'test artifact prohibited')
    for key in ('artifact', 'source', 'compiler'):
        require(len(result[key]) == 64 and bytes.fromhex(result[key]) != bytes(32), 'invalid ' + key)
    features = result['target_features'].split(',') if result['target_features'] else []
    require(features == sorted(set(features)), 'noncanonical target features')
    wasm = result['target'] == 'wasm32-unknown-unknown'
    require(wasm or result['target'] in ('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu'), 'unsupported target')
    result['execution'] = {'fp_contract': 'wasm32-core-rne-gradual-v1' if wasm else
                           'x86_64-mxcsr-rne-gradual-v1',
                           'compiler': result.pop('compiler'),
                           'target_features': result.pop('target_features')}
    return result


def sign(members, games, key_path, stage, evidence, review):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    require(members[0]['source'] == members[1]['source'], 'qualification source differs')
    require(members[0]['artifact'] != members[1]['artifact'], 'artifact identities equal')
    require(members[1]['target'] == 'wasm32-unknown-unknown', 'browser member target differs')
    wanted = coverage(games)
    for game in games:
        require(set(game) == {'source', 'mapper', 'submapper', 'timing'}, 'game fields differ')
        require(len(game['source']) == 64 and bytes.fromhex(game['source']) != bytes(32), 'invalid game hash')
    if stage == 'accepted':
        require(evidence is not None and review is not None, 'accepted signing requires evidence and independent review')
        require(evidence.get('passed') is True and evidence.get('accepted') is False, 'passed candidate evidence required')
        require(review.get('passed') is True and review.get('independent') is True, 'independent review not passed')
        require(evidence['members'] == members and evidence['games'] == games, 'evidence artifact coverage differs')
        require(len(evidence.get('cases', [])) == len(wanted) and all(case.get('passed') is True for case in evidence['cases']),
                'accepted gameplay coverage incomplete')
        require(len(evidence.get('quiet', [])) == 3 and all(case.get('passed') is True for case in evidence['quiet']),
                'accepted quiet coverage incomplete')
        require(review.get('evidence_sha256') == hashlib.sha256(canonical(evidence)).hexdigest(),
                'review does not bind supplied evidence')
    qualification = {'stage': stage, 'experimental': stage == 'candidate', 'members': members,
                     'games': games, 'evidence': evidence, 'review': review}
    contract = hashlib.sha256(b'ZeffNetplay-MixedArtifactContract/v1\0' + canonical(
        {'members': members, 'games': games})).hexdigest()
    payload = canonical({'format': 3, 'contract': contract, 'members': members, 'games': games,
                         'fp_controls': 0, 'evidence': hashlib.sha256(canonical(qualification)).hexdigest()})
    key = Ed25519PrivateKey.from_private_bytes(key_path.read_bytes())
    require(key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex() == PUBLIC_KEY, 'unexpected signing key')
    envelope = canonical({'format': 1, 'payload': payload.decode(),
                          'signature': key.sign(DOMAIN + payload).hex()}) + b'\n'
    require(len(envelope) <= 65536, 'sidecar too large')
    return envelope, hashlib.sha256(DOMAIN + payload).hexdigest()


def hex_digest(value):
    return bytes(value).hex() if isinstance(value, list) else value


def check_certificate(value, certificate, peer):
    require(isinstance(value, dict), 'certificate unavailable')
    require(hex_digest(value['certificate']) == certificate, 'certificate identity differs')
    require(hex_digest(value['peer_build']) == peer, 'peer artifact differs')


def check_case(report, members, certificate, options, game):
    frames = options['frames']
    require(report.get('options') == options, 'executed gameplay options differ')
    require(report.get('source_sha256') == game['source'], 'executed game source differs')
    require(report.get('production_browser') is True and report.get('test') is False, 'production report required')
    require(report['build_sha256'] == members[1]['artifact'], 'browser artifact differs')
    check_certificate(report['certificate'], certificate, members[0]['artifact'])
    native, browser = report['native'], report['browser']
    for index, result in enumerate((native, browser)):
        require(result['error'] is None, 'peer error: ' + str(result['error']))
        require(all(result[key] is True for key in ('ready', 'restored', 'published', 'finished')), 'peer completion fences failed')
        require(result['verified'] >= frames and len(result['frames']) >= frames, 'insufficient confirmed receipts')
        require(member(result['facts']) == members[index], 'executed member facts differ')
        require(result['facts'].get('game_source') == game['source'], 'executed peer game source differs')
    check_certificate(native['facts']['certificate'], certificate, members[1]['artifact'])
    require(native['replayed'] > 0, 'native correction was not exercised')
    require(native['frames'][:frames] == browser['frames'][:frames], 'peer frame receipts differ')
    require([row['frame'] for row in native['frames'][:frames]] == list(range(1, frames + 1)), 'frame order differs')


def quiet_pair(native, browser, options, members=None):
    if members is not None:
        require(member(native['facts']) == members[0] and member(browser['facts']) == members[1],
                'reviewed quiet executed members differ')
    for report in (native, browser):
        require(report.get('options') == options, 'executed quiet options differ')
        require(report['finite'] is True and report['restored'] is True, 'quiet PCM/restoration failed')
        require(report['arithmetic']['subnormal_count'] > 0, 'subnormal arithmetic coverage absent')
        require(report['subnormal_count'] > 0, 'subnormal PCM coverage absent')
        require(0 < report['tail_peak_bits'] < 0x00800000, 'quiet PCM tail is not subnormal')
        require(len(report['trace']) == 300, 'quiet frame coverage differs')
        require(report['continuation_frames'] == 32 and report['tail_continuation_frames'] == 32,
                'restored quiet continuation coverage differs')
    for key in ('arithmetic', 'trace', 'subnormal_count', 'peak_bits', 'tail_peak_bits'):
        require(native[key] == browser[key], 'native/browser quiet ' + key + ' differs')


def driver_path(repo, edge):
    version = edge_version(edge)
    driver = repo / 'target/wasm-browser-tools' / ('msedgedriver-' + version) / 'msedgedriver.exe'
    require(driver.is_file(), 'version-matched EdgeDriver missing: ' + str(driver))
    reported = subprocess.check_output([str(driver), '--version'], text=True)
    require('.'.join(version.split('.')[:3]) in reported, 'EdgeDriver version differs')
    return driver


def edge_version(edge):
    code = "(Get-Item -LiteralPath '" + str(edge).replace("'", "''") + "').VersionInfo.ProductVersion"
    output = subprocess.check_output(['powershell', '-NoProfile', '-Command', code], text=True).strip()
    found = re.search(r'\d+\.\d+\.\d+\.\d+', output)
    require(found is not None, 'Edge product version unavailable')
    return found.group()


def run(args):
    repo = Path(__file__).resolve().parent.parent
    output = args.output.resolve()
    output.mkdir()
    original_native = args.native.resolve()
    original_dist = args.dist.resolve()
    require(original_native.is_file() and (original_dist / 'index.html').is_file(), 'production artifacts missing')
    require(not output.is_relative_to(original_dist), 'output must be outside the input bundle')
    require(args.key is not None, 'explicit --key required for production pair signing')
    require(72 <= args.frames <= 1000, 'frames outside qualification bound')
    edge = args.edge.resolve()
    driver = args.driver.resolve() if args.driver else driver_path(repo, edge)
    reported = subprocess.check_output([str(driver), '--version'], text=True)
    require('.'.join(edge_version(edge).split('.')[:3]) in reported, 'EdgeDriver version differs')
    (output / 'artifact').mkdir()
    native = output / 'artifact' / ('zeff-boy.exe' if os.name == 'nt' else 'zeff-boy')
    shutil.copy2(original_native, native)
    dist = output / 'dist'
    shutil.copytree(original_dist, dist)
    require(digest(native) == digest(original_native), 'copied native digest differs')
    processes, logs = [], []
    server = browser = None
    sidecar_path = native.with_suffix('.netplay.json')
    old_sidecar = sidecar_path.read_bytes() if sidecar_path.exists() else None
    changed_sidecar = False
    summary = {'passed': False, 'accepted': False, 'stage': args.stage, 'cases': [], 'quiet': []}
    media = selected(args.games)
    owners = Processes()

    def launch(command, log_name, env=None):
        log = (output / log_name).open('wb')
        logs.append(log)
        process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=os.name != 'nt',
                                   creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        processes.append(process)
        owners.add(process)
        return process

    try:
        summary['wasm_audit'] = audit(dist, args.wasm_tools.resolve(), output)
        nonce = secrets.token_hex(16)
        env = {key: value for key, value in os.environ.items() if not key.startswith('ZEFF_NETPLAY_APP_')}
        env.update(ZEFF_MUTE_AUDIO='1', ZEFF_CONFIG_DIR=str(output / 'native/config'))
        launch([str(native), '--netplay-production-proof', str(output / 'native'), nonce], 'native.log', env)
        wait_ready(lambda: request(NATIVE + '/test/mixed/health') == nonce, processes)
        Handler.sidecar = None
        html = (dist / 'index.html').read_text(encoding='utf-8')
        glue = re.search(r'const glueUrl = new URL\(("[^"\n]+"), location.href\)', html)
        require(glue is not None, 'sealed Trunk loader missing')
        glue_path = urllib.parse.urlsplit(json.loads(glue.group(1))).path
        Handler.prefix = urllib.parse.urljoin('/', glue_path).rsplit('/', 1)[0] + '/'
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 47181), functools.partial(Handler, directory=str(dist)))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        url = 'http://127.0.0.1:' + str(port)
        launch([str(driver), '--port=' + str(port), '--allowed-ips=127.0.0.1'], 'driver.log')
        wait_ready(lambda: json.loads(request(url + '/status'))['value']['ready'], processes)
        browser = Browser(url, edge, output / 'edge-profile')
        browser.load()
        nf = json.loads(request(NATIVE + '/test/gameplay/facts'))
        bf = browser.execute('facts')
        facts = bf.get('facts', bf)
        members = [member(nf), member(facts)]
        require(nf['full_source'] == facts['full_source'], 'full compiled source differs')
        source_receipt = json.loads(args.source_receipt.read_text(encoding='utf-8'))
        require(nf['full_source'] == source_receipt['full_source'] and
                all(row['source'] == source_receipt['qualification_source'] for row in members),
                'compiled artifacts differ from frozen source receipt')
        summary['source_receipt_sha256'] = digest(args.source_receipt)
        require(members[0]['artifact'] == digest(native), 'native executable digest differs')
        identity = browser.call('/execute/sync', {'script': 'return globalThis.zeffBoyBundleIdentity;', 'args': []})
        require(members[1]['artifact'] == identity, 'verified sealed bundle identity differs')
        require(identity == summary['wasm_audit']['bundle_identity'], 'audited sealed bundle identity differs')
        games = nf['games'] + [entry['game'] for entry in media]
        wanted = coverage(games)
        summary.update(members=members, games=games, native_facts=nf, browser_facts=bf)
        write(output / 'facts.json', {'native': nf, 'browser': bf})
        for timing in (0, 1, 3):
            options = {'timing': timing, 'frames': 300}
            nquiet = json.loads(request(NATIVE + '/test/gameplay/quiet', options, timeout=60))
            bquiet = browser.execute('quiet', options)
            write(output / ('quiet-' + str(timing) + '.json'), {'native': nquiet, 'browser': bquiet})
            require(member(nquiet['facts']) == members[0] and member(bquiet['facts']) == members[1], 'quiet artifact facts differ')
            quiet_pair(nquiet, bquiet, options)
            quiet_path = output / ('quiet-' + str(timing) + '.json')
            summary['quiet'].append({'timing': timing, 'passed': True,
                                     'report': quiet_path.name, 'sha256': digest(quiet_path)})
        evidence, review = ({'wasm_audit': summary['wasm_audit'], 'quiet': summary['quiet']}, None)
        if args.stage == 'accepted':
            evidence, review = reviewed(args.evidence, args.review, members, games,
                                       summary['wasm_audit'], check_case,
                                       lambda n, b, o: quiet_pair(n, b, o, members))
        envelope, certificate = sign(members, games, args.key, args.stage, evidence, review)
        (output / (args.stage + '-pair.netplay.json')).write_bytes(envelope)
        sidecar_path.write_bytes(envelope)
        changed_sidecar = True
        Handler.sidecar = envelope
        browser.load()
        signed_facts = browser.execute('facts')
        check_certificate(signed_facts['certificate'], certificate, members[0]['artifact'])
        summary['certificate'] = certificate
        for timing in (0, 1, 3):
            for delay in (0, 2):
                for native_host in (True, False):
                    options = {'timing': timing, 'delay': delay, 'native_host': native_host, 'frames': args.frames}
                    report = browser.execute('run', options)
                    name = 'case-' + str(timing) + '-' + str(delay) + '-' + str(int(native_host)) + '.json'
                    write(output / name, report)
                    game = next(game for game in games[:3] if game['timing'] == (2 if timing == 3 else timing))
                    check_case(report, members, certificate, options, game)
                    summary['cases'].append({'options': options, 'game_source': game['source'], 'report': name, 'passed': True,
                                             'sha256': digest(output / name)})
                    print(json.dumps({'passed': name}), flush=True)
        for index, entry in enumerate(media):
            for native_host in (True, False):
                options = {'timing': entry['timing'], 'delay': 2,
                           'native_host': native_host, 'frames': max(240, args.frames)}
                report = browser.execute('run', dict(options, rom=list(entry['bytes'])))
                name = 'media-' + str(index) + '-' + str(int(native_host)) + '.json'
                write(output / name, report)
                check_case(report, members, certificate, options, entry['game'])
                summary['cases'].append({'options': options, 'game_source': entry['game']['source'],
                                         'name': entry['name'], 'report': name, 'passed': True,
                                         'sha256': digest(output / name)})
                print(json.dumps({'passed': name, 'game': entry['name']}), flush=True)
        summary['passed'] = len(summary['cases']) == len(wanted)
        summary['accepted'] = args.stage == 'accepted'
    except BaseException as error:
        summary['error'] = str(error)
        if browser:
            try:
                summary['native_failure'] = json.loads(request(NATIVE + '/test/gameplay/status', timeout=3))
            except Exception as diagnostic:
                summary['diagnostic_error'] = str(diagnostic)
        raise
    finally:
        cleanup = []
        if browser:
            try:
                browser.close()
            except Exception as error:
                cleanup.append(str(error))
        if server:
            server.shutdown()
            server.server_close()
        tree_closed = False
        try:
            tree_closed = owners.close()
        except Exception as error:
            cleanup.append(str(error))
        for log in logs:
            log.close()
        if changed_sidecar:
            if old_sidecar is None:
                sidecar_path.unlink()
            else:
                sidecar_path.write_bytes(old_sidecar)
        summary['cleanup'] = {'owned_processes_closed': tree_closed,
                              'sidecar_restored': not changed_sidecar or
                              (sidecar_path.read_bytes() if sidecar_path.exists() else None) == old_sidecar,
                              'errors': cleanup}
        summary['passed'] &= tree_closed and not cleanup
        write(output / 'qualification.json', summary)


def main():
    parser = argparse.ArgumentParser(description='Qualify exact production native and sealed browser artifacts.')
    parser.add_argument('--native', required=True, type=Path)
    parser.add_argument('--dist', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--key', type=Path)
    parser.add_argument('--stage', choices=('candidate', 'accepted'), default='candidate')
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--review', type=Path)
    parser.add_argument('--wasm-tools', required=True, type=Path)
    parser.add_argument('--source-receipt', required=True, type=Path)
    parser.add_argument('--frames', type=int, default=72)
    parser.add_argument('--games', type=Path)
    parser.add_argument('--edge', type=Path, default=Path('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe'))
    parser.add_argument('--driver', type=Path)
    args = parser.parse_args()
    run(args)


if __name__ == '__main__':
    main()
