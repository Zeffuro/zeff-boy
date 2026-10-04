use crate::source_stamp::SourceStamp;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Derived {
    pub digest: String,
    pub payload: Value,
}

pub fn derive(
    root: &Path,
    rustc: &Path,
    args: &[OsString],
    source: &SourceStamp,
) -> Result<Derived, String> {
    let strings: Vec<String> = args
        .iter()
        .map(|a| {
            a.to_str()
                .map(str::to_owned)
                .ok_or("non-UTF-8 rustc argument")
        })
        .collect::<Result<_, _>>()?;
    validate_flags(&strings)?;
    let version = query(rustc, &["-Vv"])?;
    let sysroot = PathBuf::from(query(rustc, &["--print", "sysroot"])?.trim());
    let reported_compiler =
        sysroot
            .join("bin")
            .join(if cfg!(windows) { "rustc.exe" } else { "rustc" });
    let compiler = checked_compiler(rustc, &reported_compiler)?;
    let target = option(&strings, "--target")
        .unwrap_or_else(|| {
            version
                .lines()
                .find_map(|line| line.strip_prefix("host: "))
                .unwrap_or("unknown")
        })
        .to_string();
    if !matches!(
        target.as_str(),
        "x86_64-pc-windows-msvc" | "x86_64-unknown-linux-gnu"
    ) {
        return Err(format!("unsupported diagnostic target: {target}"));
    }
    let stdlib =
        PathBuf::from(query(rustc, &["--print", "target-libdir", "--target", &target])?.trim());
    let target_dir = PathBuf::from(
        env::var_os("CARGO_TARGET_DIR").ok_or("missing owned Cargo target directory")?,
    );
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".cargo"))
        })
        .ok_or("cannot resolve Cargo home for registry library audit")?;
    let registry = resolved_registry(&cargo_home)?;
    let paths = Paths {
        root,
        target: &target_dir,
        sysroot: &sysroot,
        registry: &registry,
    };
    let mut artifacts = BTreeMap::new();
    for (index, argument) in strings.iter().enumerate() {
        if argument == "--extern" {
            let value = strings.get(index + 1).ok_or("missing --extern value")?;
            if let Some((name, path)) = value.split_once('=') {
                let path = PathBuf::from(path);
                add_file(
                    &path,
                    &format!("extern:{name}:{}", paths.normalize_path(&path)?),
                    &mut artifacts,
                )?;
            } else {
                return Err("--extern without an explicit artifact path is unsupported".into());
            }
        } else if argument == "-L" {
            let value = strings.get(index + 1).ok_or("missing -L value")?;
            let path = PathBuf::from(
                value
                    .split_once('=')
                    .map_or(value.as_str(), |(_, path)| path),
            );
            collect_libraries(&path, &paths, &mut artifacts)?;
        }
    }
    collect_libraries(&stdlib, &paths, &mut artifacts)?;
    for directory in [sysroot.join("bin"), sysroot.join("lib")] {
        if directory.is_dir() {
            for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path();
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                if path.is_file()
                    && (name.ends_with(".dll") || name.contains(".so") || name.ends_with(".dylib"))
                {
                    add_file(&path, &paths.normalize_path(&path)?, &mut artifacts)?;
                }
            }
        }
    }
    let mut generated = BTreeMap::new();
    if let Some(out) = env::var_os("OUT_DIR") {
        collect_generated(&PathBuf::from(out), &paths, &mut generated)?;
    }
    let mut environment = BTreeMap::new();
    for name in [
        "CARGO_PKG_NAME",
        "CARGO_PKG_VERSION",
        "CARGO_PKG_VERSION_MAJOR",
        "CARGO_PKG_VERSION_MINOR",
        "CARGO_PKG_VERSION_PATCH",
        "CARGO_PKG_VERSION_PRE",
        "CARGO_CRATE_NAME",
        "CARGO_BIN_NAME",
        "CARGO_PRIMARY_PACKAGE",
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_INCREMENTAL",
        "OUT_DIR",
        "SOURCE_DATE_EPOCH",
        "ZERO_AR_DATE",
        "ZEFF_NETPLAY_QUALIFICATION_SOURCE",
    ] {
        if let Some(value) = env::var_os(name) {
            let value = value
                .to_str()
                .ok_or_else(|| format!("non-UTF-8 audited environment: {name}"))?;
            environment.insert(name.to_string(), paths.normalize(value));
        }
    }
    if environment.get("ZEFF_NETPLAY_QUALIFICATION_SOURCE") != Some(&source.qualification) {
        return Err("root build script qualification source does not match current source".into());
    }
    let payload = bind_source(
        source,
        json!({
            "schema": "zeff-netplay-rustc-observation-v1",
            "platform": target,
            "cfg_test": strings.iter().any(|s| s == "--test") || strings.windows(2).any(|s| s[0] == "--cfg" && s[1] == "test"),
        "compiler": {"sha256": file_hash(&compiler)?, "version_verbose": version},
        "pipeline_executable_sha256": file_hash(&env::current_exe().map_err(|e| e.to_string())?)?,
            "argv": strings.iter().map(|s| paths.normalize(s)).collect::<Vec<_>>(),
            "environment": environment,
            "artifacts": artifacts,
            "generated_inputs": generated,
            "limits": ["artifact cache libraries are conservatively included"]
        }),
    );
    Ok(Derived {
        digest: payload_digest(&payload)?,
        payload,
    })
}

fn payload_digest(payload: &Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn bind_source(source: &SourceStamp, mut recipe: Value) -> Value {
    let fields = recipe.as_object_mut().expect("receipt recipe object");
    fields.insert("qualification_source".into(), json!(source.qualification));
    fields.insert("qualification_eligible".into(), json!(false));
    fields.insert(
        "evidence_gaps".into(),
        json!([
            "before/after source observations cannot exclude transient concurrent source mutation",
            "default platform linker and ambient native/system libraries are not fully identified",
        ]),
    );
    recipe
}

fn checked_compiler(invoked: &Path, reported: &Path) -> Result<PathBuf, String> {
    let actual = invoked
        .canonicalize()
        .map_err(|e| format!("cannot resolve invoked compiler: {e}"))?;
    let expected = reported
        .canonicalize()
        .map_err(|e| format!("cannot resolve reported sysroot compiler: {e}"))?;
    if actual != expected {
        return Err("invoked compiler does not match reported sysroot/bin/rustc; forwarding/compiler overrides unsupported".into());
    }
    Ok(actual)
}

fn query(rustc: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(rustc)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("compiler identity query failed".into());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn validate_flags(args: &[String]) -> Result<(), String> {
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument.starts_with('@') {
            return Err("rustc response files are unsupported".into());
        }
        match argument.as_str() {
            "--crate-name" | "--edition" | "--crate-type" | "--emit" | "--error-format"
            | "--json" | "--out-dir" | "--target" | "--cfg" | "--check-cfg" | "--extern"
            | "--cap-lints" | "-L" | "-l" => {
                index += 1;
                args.get(index).ok_or("rustc flag missing value")?;
            }
            "--test" => {}
            "-C" => {
                index += 1;
                validate_codegen(args.get(index).ok_or("missing rustc codegen option")?)?;
            }
            value if value.starts_with("-C") => validate_codegen(&value[2..])?,
            value
                if value.starts_with("--edition=")
                    || value.starts_with("--emit=")
                    || value.starts_with("--crate-type=")
                    || value.starts_with("--error-format=")
                    || value.starts_with("--json=") => {}
            value if !value.starts_with('-') && value.ends_with(".rs") => {}
            _ => return Err(format!("unsupported root rustc argument: {argument}")),
        }
        index += 1;
    }
    Ok(())
}

fn validate_codegen(value: &str) -> Result<(), String> {
    let key = value.split('=').next().unwrap_or_default();
    if matches!(
        key,
        "opt-level"
            | "embed-bitcode"
            | "debuginfo"
            | "debug-assertions"
            | "overflow-checks"
            | "panic"
            | "codegen-units"
            | "metadata"
            | "extra-filename"
            | "strip"
            | "lto"
    ) {
        Ok(())
    } else {
        Err(format!("unsupported root codegen option: {key}"))
    }
}

struct Paths<'a> {
    root: &'a Path,
    target: &'a Path,
    sysroot: &'a Path,
    registry: &'a Path,
}

fn resolved_registry(cargo_home: &Path) -> Result<PathBuf, String> {
    let registry = cargo_home
        .join("registry")
        .canonicalize()
        .map_err(|e| format!("cannot resolve Cargo registry: {e}"))?;
    if !registry.is_dir() {
        return Err("Cargo registry is not a directory".into());
    }
    Ok(registry)
}

impl Paths<'_> {
    fn normalize(&self, value: &str) -> String {
        let mut value = value.replace('\\', "/");
        let registry = self.registry.to_string_lossy().replace('\\', "/");
        // Replace the extended spelling before its embedded plain path.
        value = value.replace(&registry, "$CARGO_REGISTRY");
        if let Some(plain) = registry.strip_prefix("//?/UNC/") {
            value = value.replace(&format!("//{plain}"), "$CARGO_REGISTRY");
        } else if let Some(plain) = registry.strip_prefix("//?/") {
            value = value.replace(plain, "$CARGO_REGISTRY");
        }
        for (path, label) in [
            (self.registry, "$CARGO_REGISTRY"),
            (self.target, "$TARGET"),
            (self.root, "$ROOT"),
            (self.sysroot, "$SYSROOT"),
        ] {
            value = value.replace(&path.to_string_lossy().replace('\\', "/"), label);
        }
        value
    }

    fn normalize_path(&self, path: &Path) -> Result<String, String> {
        let mut path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            env::current_dir().map_err(|e| e.to_string())?.join(path)
        };
        if ![self.root, self.target, self.sysroot]
            .iter()
            .any(|root| path.starts_with(root))
        {
            let canonical = path.canonicalize().map_err(|e| e.to_string())?;
            if !canonical.starts_with(self.registry) {
                return Err(format!(
                    "artifact outside audited roots: {}",
                    path.display()
                ));
            }
            path = canonical;
        }
        if path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err("artifact input path contains parent traversal".into());
        }
        let value = self.normalize(path.to_str().ok_or("non-UTF-8 artifact path")?);
        if !value.starts_with('$') {
            return Err(format!(
                "artifact outside audited roots: {}",
                path.display()
            ));
        }
        Ok(value)
    }
}

fn file_hash(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn add_file(path: &Path, name: &str, entries: &mut BTreeMap<String, String>) -> Result<(), String> {
    if !fs::metadata(path).map_err(|e| e.to_string())?.is_file() {
        return Err("artifact is not a regular file".into());
    }
    entries.insert(name.to_string(), file_hash(path)?);
    Ok(())
}

fn collect_libraries(
    path: &Path,
    paths: &Paths<'_>,
    entries: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    paths.normalize_path(path)?;
    walk(path, &mut |path| {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("non-UTF-8 library filename")?;
        if name.starts_with("zeff_boy-") || name.starts_with("libzeff_boy-") {
            return Ok(());
        }
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if matches!(
            extension,
            "rlib" | "rmeta" | "a" | "lib" | "o" | "obj" | "so" | "dll" | "dylib" | "res"
        ) {
            add_file(path, &paths.normalize_path(path)?, entries)?;
        }
        Ok(())
    })
}

fn collect_generated(
    path: &Path,
    paths: &Paths<'_>,
    entries: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    paths.normalize_path(path)?;
    walk(path, &mut |path| {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("non-UTF-8 generated filename")?;
        if name.starts_with("zeff-netplay-source-stamp") || name.starts_with("zeff-netplay-receipt")
        {
            return Ok(());
        }
        add_file(path, &paths.normalize_path(path)?, entries)
    })
}

fn walk(path: &Path, visit: &mut impl FnMut(&Path) -> Result<(), String>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("symlink in artifact inputs: {}", path.display()));
    }
    if metadata.is_file() {
        return visit(path);
    }
    if !metadata.is_dir() {
        return Err("unsupported artifact input type".into());
    }
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        walk(&entry.map_err(|e| e.to_string())?.path(), visit)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustc_custom_linker_sysroot_response_and_unknown_flags_fail_closed() {
        for args in [
            vec!["-C", "linker=custom"],
            vec!["--sysroot", "custom"],
            vec!["@args"],
            vec!["-Z", "unstable-options"],
            vec!["-C", "link-arg=custom"],
        ] {
            assert!(
                validate_flags(&args.into_iter().map(str::to_string).collect::<Vec<_>>()).is_err()
            );
        }
        assert!(
            validate_flags(
                &[
                    "src/main.rs",
                    "--crate-name",
                    "zeff_boy",
                    "--cfg",
                    "feature=\"camera\"",
                    "-C",
                    "opt-level=3"
                ]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
            )
            .is_ok()
        );
    }

    #[test]
    fn normalization_preserves_recipe_and_relocates_owned_paths() {
        let first = Paths {
            root: Path::new("/one"),
            target: Path::new("/one/target/qualified"),
            sysroot: Path::new("/rust"),
            registry: Path::new("/cargo/registry"),
        };
        let second = Paths {
            root: Path::new("/two"),
            target: Path::new("/two/target/qualified"),
            sysroot: Path::new("/rust2"),
            registry: Path::new("/cargo2/registry"),
        };
        assert_eq!(
            first.normalize("dependency=/one/target/qualified/debug/deps"),
            second.normalize("dependency=/two/target/qualified/debug/deps")
        );
        assert_ne!(
            first.normalize("feature=\"camera\""),
            first.normalize("feature=\"audio\"")
        );
    }

    #[test]
    fn qualification_table_edits_do_not_recurse_into_receipt() {
        let root = env::temp_dir().join(format!("zeff-receipt-table-{}", std::process::id()));
        fs::create_dir_all(root.join("src/netplay")).unwrap();
        for name in ["Cargo.toml", "Cargo.lock", "build.rs"] {
            fs::write(root.join(name), name).unwrap();
        }
        let table = root.join("src/netplay/qualification.json");
        let parser = root.join("src/netplay/compatibility.rs");
        fs::write(&table, b"[]").unwrap();
        fs::write(&parser, b"parser").unwrap();
        let before = crate::source_stamp::snapshot(&root).unwrap();
        let recipe = json!({"argv": ["--test"], "artifacts": {"library": "actual artifact hash"}});
        let original = payload_digest(&bind_source(&before, recipe.clone())).unwrap();
        let observed = bind_source(&before, recipe.clone());
        assert_eq!(observed["qualification_eligible"], false);
        assert_eq!(observed["evidence_gaps"].as_array().unwrap().len(), 2);
        fs::write(&table, b"[{\"reviewed\":true}]").unwrap();
        let after = crate::source_stamp::snapshot(&root).unwrap();
        assert_ne!(before.full, after.full);
        assert_eq!(
            original,
            payload_digest(&bind_source(&after, recipe.clone())).unwrap()
        );
        fs::write(&parser, b"changed parser").unwrap();
        let changed = crate::source_stamp::snapshot(&root).unwrap();
        assert_ne!(
            original,
            payload_digest(&bind_source(&changed, recipe)).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn forwarding_compiler_path_cannot_claim_reported_compiler_identity() {
        let root = env::temp_dir().join(format!("zeff-compiler-identity-{}", std::process::id()));
        fs::create_dir_all(root.join("sysroot/bin")).unwrap();
        let invoked = root.join("forwarding-compiler");
        let reported = root.join("sysroot/bin/rustc");
        fs::write(&invoked, b"forwarder").unwrap();
        fs::write(&reported, b"actual compiler").unwrap();
        let error = checked_compiler(&invoked, &reported).unwrap_err();
        assert!(error.contains("does not match"));
        let actual = checked_compiler(&reported, &reported).unwrap();
        assert_eq!(
            file_hash(&actual).unwrap(),
            Sha256::digest(b"actual compiler")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        assert!(checked_compiler(&root.join("missing"), &reported).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registry_native_libraries_are_hashed_but_external_sdk_is_rejected() {
        let root = env::temp_dir().join(format!("zeff-registry-audit-{}", std::process::id()));
        let home = root.join("cargo-home");
        let native = home.join("registry/src/index/windows/lib");
        fs::create_dir_all(&native).unwrap();
        let library = native.join("windows.lib");
        fs::write(&library, b"native library").unwrap();
        fs::write(native.join("README"), b"not a linker input").unwrap();
        let registry = resolved_registry(&home).unwrap();
        let paths = Paths {
            root: &root.join("repo"),
            target: &root.join("target"),
            sysroot: &root.join("rust"),
            registry: &registry,
        };
        let mut artifacts = BTreeMap::new();
        collect_libraries(&native, &paths, &mut artifacts).unwrap();
        let name = "$CARGO_REGISTRY/src/index/windows/lib/windows.lib";
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[name], file_hash(&library).unwrap());
        assert!(
            paths
                .normalize(&native.to_string_lossy())
                .starts_with("$CARGO_REGISTRY/")
        );
        let original = artifacts[name].clone();
        fs::write(&library, b"changed native library").unwrap();
        collect_libraries(&native, &paths, &mut artifacts).unwrap();
        assert_ne!(artifacts[name], original);
        let sdk = root.join("external-sdk");
        fs::create_dir_all(&sdk).unwrap();
        fs::write(sdk.join("external.lib"), b"SDK library").unwrap();
        assert!(
            collect_libraries(&sdk, &paths, &mut artifacts)
                .unwrap_err()
                .contains("outside audited roots")
        );
        assert_eq!(artifacts.len(), 1);
        assert!(resolved_registry(&root.join("missing-home")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
