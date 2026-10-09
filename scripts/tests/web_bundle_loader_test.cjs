const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const {webcrypto} = require('node:crypto');

async function run(source, scenario) {
    let timeout;
    let signal;
    let reads = 0;
    let initialized = false;
    const events = [];
    const context = vm.createContext({
        URL: class extends URL {
            static createObjectURL() { return 'blob:mock'; }
            static revokeObjectURL() {}
        },
        Blob, TextEncoder, TextDecoder, Uint8Array, AbortController,
        location: {href: 'https://example.test/stable/index.html'},
        crypto: webcrypto,
        setTimeout(callback, delay) {
            assert.equal(delay, 3000);
            timeout = callback;
            return 1;
        },
        clearTimeout(id) { assert.equal(id, 1); events.push('cleared'); },
        fetch: async (url, options) => {
            const name = url.pathname.split('/').at(-1);
            if (name !== 'zeff-boy.netplay.json') {
                const data = fs.readFileSync(process.argv[3] + '/' + (name === 'save.js' ? 'snippets/' : '') + name);
                return {ok: true, arrayBuffer: async () => data};
            }
            assert.equal(url.href, 'https://example.test/stable/zeff-boy.netplay.json');
            assert.equal(options.cache, 'no-store');
            signal = options.signal;
            events.push('fetch');
            if (scenario === 'fetch-timeout') {
                queueMicrotask(timeout);
                return new Promise(() => {});
            }
            if (scenario === 'fetch-failure') throw null;
            if (scenario === 'missing') return {ok: false, status: 404};
            if (scenario === 'http-error') return {ok: false, status: 503};
            return {ok: true, status: 200, body: {getReader: () => ({
                async read() {
                    reads++;
                    if (scenario === 'timeout' || (scenario === 'delayed-chunk' && reads === 2)) {
                        queueMicrotask(timeout);
                        return new Promise(() => {});
                    }
                    if (scenario === 'read-failure') throw new Error('private server detail');
                    if (scenario === 'oversize') return {done: false, value: new Uint8Array(32769)};
                    if (scenario === 'limit') return reads === 1
                        ? {done: false, value: new Uint8Array(65536).fill(32)} : {done: true};
                    if (reads === 1) return {done: false, value: new TextEncoder().encode('{"raw":')};
                    if (reads === 2) return {done: false, value: new TextEncoder().encode('"é"}')};
                    events.push('body-complete');
                    return {done: true};
                },
                releaseLock() { events.push('released'); }
            })}};
        },
        mockBindings: {default: async options => {
            assert.ok(options.module_or_path);
            assert.ok(events.includes('cleared'));
            initialized = true;
            events.push('initialized');
            return {};
        }}
    });
    const executable = source.replace(
        /await import\(moduleUrl\(decodeURI\(glueUrl.href.slice\(baseUrl.href.length\)\)\)\)/,
        '(moduleUrl(decodeURI(glueUrl.href.slice(baseUrl.href.length))), mockBindings)'
    );
    assert.notEqual(executable, source);
    await vm.runInContext('(async () => {' + executable + '\n})()', context);
    assert.equal(initialized, true);
    const certificate = context.zeffBoyPairCertificate;
    const error = context.zeffBoyPairCertificateError;
    if (scenario === 'valid') {
        assert.equal(certificate, '{"raw":"é"}');
        assert.equal(error, null);
        assert.equal(signal.aborted, false);
        assert.ok(events.indexOf('body-complete') < events.indexOf('initialized'));
    } else if (scenario === 'limit') {
        assert.equal(certificate.length, 65536);
        assert.equal(error, null);
    } else {
        assert.equal(certificate, null);
        assert.equal(signal.aborted, true);
        const expected = {
            missing: null,
            oversize: 'Pair certificate too large.',
            timeout: 'Pair certificate timed out.',
            'fetch-timeout': 'Pair certificate timed out.',
            'delayed-chunk': 'Pair certificate timed out.',
            'read-failure': 'Pair certificate could not be read.',
            'fetch-failure': 'Pair certificate could not be read.',
            'http-error': 'Pair certificate unavailable.'
        };
        assert.equal(error, expected[scenario]);
        if (scenario === 'oversize') assert.equal(reads, 2);
    }
    return context.zeffBoyBundleIdentity;
}

(async () => {
    const html = fs.readFileSync(process.argv[2], 'utf8');
    let identity;
    for (const scenario of ['missing', 'valid', 'limit', 'oversize', 'delayed-chunk',
        'timeout', 'fetch-timeout', 'read-failure', 'fetch-failure', 'http-error']) {
        const actual = await run(html, scenario);
        if (identity) assert.equal(actual, identity);
        identity = actual;
    }
    process.stdout.write('loader scenarios: 10 passed\n');
})().catch(error => { console.error(error); process.exitCode = 1; });
