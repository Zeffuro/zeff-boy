const manifest = __MANIFEST__;
const glueUrl = new URL(__GLUE_URL__, location.href);
const baseUrl = new URL('.', glueUrl);
const bytes = new Map();
const blobs = new Map();
const hex = value => Array.from(new Uint8Array(value), byte => byte.toString(16).padStart(2, '0')).join('');
const digest = value => crypto.subtle.digest('SHA-256', value);
await Promise.all(Object.entries(manifest.files).map(async ([name, expected]) => {
    const response = await fetch(new URL(name, baseUrl), {cache: 'no-store'});
    if (!response.ok) throw new Error('Web build asset unavailable. Reload the page.');
    const data = await response.arrayBuffer();
    if (hex(await digest(data)) !== expected) throw new Error('Web build changed. Reload the page.');
    bytes.set(name, data);
}));
function moduleUrl(name, visiting = new Set()) {
    if (blobs.has(name)) return blobs.get(name);
    if (visiting.has(name) || !bytes.has(name)) throw new Error('Invalid web module dependency.');
    visiting.add(name);
    const source = new TextDecoder().decode(bytes.get(name)).replace(
        /(\bfrom\s*|\bimport\s*)(['"])(\.\.?\/[^'"]+)\2/g,
        (_, prefix, quote, path) => {
            const url = new URL(path, new URL(name, baseUrl));
            if (!url.href.startsWith(baseUrl.href)) throw new Error('Invalid web module path.');
            const relative = decodeURI(url.href.slice(baseUrl.href.length));
            return prefix + quote + moduleUrl(relative, new Set(visiting)) + quote;
        }
    );
    const url = URL.createObjectURL(new Blob([source], {type: 'text/javascript'}));
    blobs.set(name, url);
    return url;
}
globalThis.zeffBoyBundleIdentity = hex(await digest(new TextEncoder().encode(JSON.stringify(manifest))));
async function loadPairCertificate() {
    globalThis.zeffBoyPairCertificate = null;
    globalThis.zeffBoyPairCertificateError = null;
    const controller = new AbortController();
    let timer;
    const deadline = new Promise((_, reject) => {
        timer = setTimeout(() => {
            controller.abort();
            reject(new Error('Pair certificate timed out.'));
        }, 3000);
    });
    try {
        const certificate = (async () => {
            const response = await fetch(new URL('zeff-boy.netplay.json', baseUrl), {
                cache: 'no-store', signal: controller.signal
            });
            if (response.status === 404) {
                controller.abort();
                return null;
            }
            if (!response.ok || !response.body) throw new Error('Pair certificate unavailable.');
            const reader = response.body.getReader();
            const data = new Uint8Array(65536);
            let length = 0;
            while (true) {
                const {done, value} = await reader.read();
                if (done) break;
                if (value.byteLength > data.length - length) throw new Error('Pair certificate too large.');
                data.set(value, length);
                length += value.byteLength;
            }
            reader.releaseLock();
            return new TextDecoder().decode(data.subarray(0, length));
        })();
        globalThis.zeffBoyPairCertificate = await Promise.race([certificate, deadline]);
    } catch (error) {
        controller.abort();
        globalThis.zeffBoyPairCertificateError = error?.message === 'Pair certificate timed out.'
            || error?.message === 'Pair certificate too large.'
            || error?.message === 'Pair certificate unavailable.'
            ? error.message : 'Pair certificate could not be read.';
    } finally {
        clearTimeout(timer);
    }
}
await loadPairCertificate();
const bindings = await import(moduleUrl(decodeURI(glueUrl.href.slice(baseUrl.href.length))));
const wasm = await bindings.default({module_or_path: bytes.get(__WASM_NAME__)});
globalThis.zeffBoyQualification = Object.freeze({
    facts: bindings.zeffBoyQualificationFacts,
    run: bindings.zeffBoyQualifyNetplay,
    quiet: bindings.zeffBoyQualifyQuiet,
});
for (const url of blobs.values()) URL.revokeObjectURL(url);
