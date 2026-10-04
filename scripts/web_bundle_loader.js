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
const bindings = await import(moduleUrl(decodeURI(glueUrl.href.slice(baseUrl.href.length))));
const wasm = await bindings.default({module_or_path: bytes.get(__WASM_NAME__)});
for (const url of blobs.values()) URL.revokeObjectURL(url);
