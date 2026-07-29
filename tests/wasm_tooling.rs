use std::fs;
use std::path::Path;

fn read_repo_file(relative_path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path))
        .unwrap_or_else(|err| panic!("failed to read {relative_path}: {err}"))
}

fn quoted_assignment<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim().strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn cargo_lock_package_version(lockfile: &str, package_name: &str) -> Option<String> {
    let mut in_target_package = false;

    for line in lockfile.lines() {
        if line.trim() == "[[package]]" {
            in_target_package = false;
            continue;
        }

        if let Some(name) = quoted_assignment(line, "name") {
            in_target_package = name == package_name;
            continue;
        }

        if in_target_package && let Some(version) = quoted_assignment(line, "version") {
            return Some(version.to_owned());
        }
    }

    None
}

fn trunk_wasm_bindgen_version(trunk_config: &str) -> Option<String> {
    trunk_config
        .lines()
        .find_map(|line| quoted_assignment(line, "wasm_bindgen").map(str::to_owned))
}

#[test]
fn trunk_wasm_bindgen_matches_locked_dependency() {
    let trunk_version = trunk_wasm_bindgen_version(&read_repo_file("Trunk.toml"))
        .expect("Trunk.toml should pin [tools].wasm_bindgen");
    let locked_version = cargo_lock_package_version(&read_repo_file("Cargo.lock"), "wasm-bindgen")
        .expect("Cargo.lock should contain wasm-bindgen");

    assert_eq!(
        trunk_version, locked_version,
        "Trunk's wasm-bindgen binary must match the wasm-bindgen crate schema"
    );
}
