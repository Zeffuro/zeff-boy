import hashlib
import json
import os
from pathlib import Path
import re


def bind_bundle(staging):
    html_path = staging / "index.html"
    html = html_path.read_text(encoding="utf-8")
    loader = re.compile(
        r"import init, \* as bindings from '([^']+)';\s*"
        r"const wasm = await init\(\{ module_or_path: '([^']+)' \}\);"
    )
    match = loader.search(html)
    if match is None:
        raise ValueError("Unsupported Trunk loader")
    glue_url, wasm_url = match.groups()
    glue_name, wasm_name = Path(glue_url).name, Path(wasm_url).name
    assets = [staging / wasm_name, staging / glue_name, *sorted((staging / "snippets").rglob("*.js"))]
    runtime = (Path(__file__).parent / "web_bundle_loader.js").read_text(encoding="utf-8")
    manifest = {
        "abi": "zeff-browser-bundle-v1",
        "loader": hashlib.sha256(runtime.encode()).hexdigest(),
        "files": {
            path.relative_to(staging).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(assets)
        },
    }
    runtime = runtime.replace("__MANIFEST__", json.dumps(manifest, separators=(",", ":")))
    runtime = runtime.replace("__GLUE_URL__", json.dumps(glue_url))
    runtime = runtime.replace("__WASM_NAME__", json.dumps(wasm_name))
    html_path.write_text(html[:match.start()] + runtime + html[match.end():], encoding="utf-8")


if __name__ == "__main__":
    bind_bundle(Path(os.environ["TRUNK_STAGING_DIR"]))
