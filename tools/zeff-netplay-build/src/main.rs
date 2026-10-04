mod receipt;

#[path = "../../../build_support/source_stamp.rs"]
mod source_stamp;

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::PathBuf,
    process::{Command, ExitCode},
};

const ROOT: &str = "ZEFF_NETPLAY_PIPELINE_ROOT_V1";
const EXPECTED: &str = "ZEFF_NETPLAY_PIPELINE_SOURCE_V1";
const OUTPUT: &str = "ZEFF_NETPLAY_PIPELINE_OUTPUT_V1";
const RECEIPT: &str = "ZEFF_NETPLAY_BUILD_RECEIPT_V1";
const OBSERVED: &str = "ZEFF_NETPLAY_OBSERVED_RECEIPT_V1";

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("zeff-netplay-build: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<u8, String> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if env::var_os(ROOT).is_some() {
        return wrapper(&args);
    }
    build(&args)
}

fn build(args: &[OsString]) -> Result<u8, String> {
    if args.len() < 5 || args[0] != "build" || args[1] != "--root" || args[3] != "--" {
        return Err("usage: zeff-netplay-build build --root <repo> -- [+named-toolchain] build|test [supported Cargo options]".into());
    }
    validate_environment()?;
    let root = PathBuf::from(&args[2])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let cargo_args = validate_cargo_args(&args[4..])?;
    let before = source_stamp::snapshot(&root).map_err(|e| e.to_string())?;
    let serial = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let output = root
        .join(".tmp/netplay-build-observations")
        .join(format!("{serial}-{}", std::process::id()));
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let target = root.join("target/netplay-observed");
    let status = diagnostic_command(OsStr::new("cargo"))
        .args(cargo_args)
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0")
        .env(
            "RUSTC_WRAPPER",
            env::current_exe().map_err(|e| e.to_string())?,
        )
        .env(ROOT, &root)
        .env(EXPECTED, &before.full)
        .env(OUTPUT, &output)
        .status()
        .map_err(|e| e.to_string())?;
    let after = source_stamp::snapshot(&root).map_err(|e| e.to_string())?;
    if before.full != after.full {
        return Err(
            "source inputs changed during the diagnostic Cargo build; qualification unavailable"
                .into(),
        );
    }
    if status.success() {
        let count = fs::read_dir(&output).map_err(|e| e.to_string())?.count();
        if count == 0 {
            return Err(
                "Cargo produced no diagnostic root rustc receipt; qualification unavailable (a fresh root compile is required)".into(),
            );
        }
        println!(
            "diagnostic only; qualification unavailable; source={} qualification_source={} observed_receipts={}",
            before.full,
            before.qualification,
            output.display()
        );
    }
    Ok(status.code().unwrap_or(1).try_into().unwrap_or(1))
}

fn wrapper(args: &[OsString]) -> Result<u8, String> {
    let (rustc, arguments) = args
        .split_first()
        .ok_or("wrapper missing rustc executable")?;
    let root = PathBuf::from(env::var_os(ROOT).ok_or("wrapper missing source root")?);
    let manifest = env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
    let crate_name = arguments
        .windows(2)
        .find(|pair| pair[0] == "--crate-name")
        .map(|pair| &pair[1]);
    let is_root = manifest
        .as_ref()
        .and_then(|p| p.canonicalize().ok())
        .as_ref()
        == Some(&root)
        && crate_name.is_some_and(|name| name == "zeff_boy");
    let mut command = diagnostic_command(rustc);
    command.args(arguments);
    if !is_root {
        return command
            .status()
            .map(|s| s.code().unwrap_or(1).try_into().unwrap_or(1))
            .map_err(|e| e.to_string());
    }
    let before = source_stamp::snapshot(&root).map_err(|e| e.to_string())?;
    if env::var(EXPECTED).map_err(|e| e.to_string())? != before.full {
        return Err("diagnostic root rustc source does not match parent snapshot; qualification unavailable".into());
    }
    let derived = receipt::derive(&root, &PathBuf::from(rustc), arguments, &before)?;
    command.env(OBSERVED, &derived.digest);
    let status = command.status().map_err(|e| e.to_string())?;
    if source_stamp::snapshot(&root)
        .map_err(|e| e.to_string())?
        .full
        != before.full
    {
        return Err(
            "source inputs changed during diagnostic root rustc; qualification unavailable".into(),
        );
    }
    if status.success() {
        let output = PathBuf::from(env::var_os(OUTPUT).ok_or("wrapper missing receipt directory")?);
        let report = diagnostic_report(&derived, &before);
        fs::write(
            output.join(format!("{}.json", derived.digest)),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(status.code().unwrap_or(1).try_into().unwrap_or(1))
}

fn diagnostic_command(program: &OsStr) -> Command {
    let mut command = Command::new(program);
    command
        .env_remove(RECEIPT)
        .env_remove(OBSERVED)
        .env("RUSTC_WORKSPACE_WRAPPER", "");
    command
}

fn diagnostic_report(
    derived: &receipt::Derived,
    source: &source_stamp::SourceStamp,
) -> serde_json::Value {
    serde_json::json!({
        "receipt_version": 1,
        "status": "diagnostic only; qualification unavailable",
        "qualification_eligible": false,
        "observed_receipt": derived.digest,
        "full_source": source.full,
        "source_file_count": source.file_count,
        "inputs": derived.payload,
    })
}

fn validate_environment() -> Result<(), String> {
    for (name, value) in env::vars_os() {
        let name = name.to_string_lossy();
        if forbidden_environment(&name) && !value.is_empty() {
            return Err(format!("unsupported inherited build environment: {name}"));
        }
    }
    Ok(())
}

fn forbidden_environment(name: &str) -> bool {
    matches!(
        name,
        "RUSTC"
            | "RUSTC_WRAPPER"
            | "RUSTC_WORKSPACE_WRAPPER"
            | "RUSTFLAGS"
            | "CARGO_ENCODED_RUSTFLAGS"
            | "RUSTDOCFLAGS"
            | "CARGO_TARGET_DIR"
            | "RUSTC_BOOTSTRAP"
            | "RUSTC_FORCE_UNSTABLE"
            | "CARGO_BUILD_RUSTC"
            | "CARGO_BUILD_RUSTC_WRAPPER"
            | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
    ) || (name.starts_with("CARGO_TARGET_")
        && (name.ends_with("_LINKER") || name.ends_with("_RUSTFLAGS") || name.ends_with("_RUNNER")))
        || name.starts_with("ZEFF_NETPLAY_")
}

fn validate_cargo_args(args: &[OsString]) -> Result<Vec<OsString>, String> {
    let strings: Vec<&str> = args
        .iter()
        .map(|a| a.to_str().ok_or("non-UTF-8 Cargo argument"))
        .collect::<Result<_, _>>()?;
    let mut index = 0;
    if strings.first().is_some_and(|a| a.starts_with('+')) {
        let toolchain = &strings[0][1..];
        if !toolchain.chars().all(|c| c.is_ascii_digit() || c == '.') || toolchain.is_empty() {
            return Err("only explicitly named numeric Rust toolchains are supported".into());
        }
        index += 1;
    }
    if !strings
        .get(index)
        .is_some_and(|a| matches!(*a, "build" | "test"))
    {
        return Err("supported Cargo commands are build and test".into());
    }
    index += 1;
    while let Some(argument) = strings.get(index) {
        match *argument {
            "--release"
            | "--locked"
            | "--offline"
            | "--frozen"
            | "--no-default-features"
            | "--all-features"
            | "--lib"
            | "--no-run" => {}
            "--features" | "--target" | "--profile" | "--package" | "-p" | "--bin" => {
                index += 1;
                let value = *strings.get(index).ok_or("missing Cargo option value")?;
                if value.starts_with('-') || value.starts_with('@') {
                    return Err("invalid Cargo option value".into());
                }
                if matches!(*argument, "--package" | "-p" | "--bin") && value != "zeff-boy" {
                    return Err("pipeline builds the zeff-boy root only".into());
                }
                if *argument == "--profile" && !matches!(value, "dev" | "release" | "test") {
                    return Err("unknown build profile".into());
                }
            }
            _ => return Err(format!("unsupported Cargo argument: {argument}")),
        }
        index += 1;
    }
    Ok(args.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn rejects_custom_flags_aliases_and_response_files() {
        for values in [
            &["rustc", "--", "-C", "linker=custom"][..],
            &["build", "--config", "build.rustflags=[]"],
            &["build", "@args"],
            &["+nightly", "build"],
        ] {
            assert!(validate_cargo_args(&args(values)).is_err());
        }
        assert!(
            validate_cargo_args(&args(&[
                "+1.99.0",
                "build",
                "--release",
                "--no-default-features",
                "--features",
                "camera"
            ]))
            .is_ok()
        );
    }

    #[test]
    fn diagnostic_children_remove_admission_env_and_disable_configured_workspace_wrapper() {
        let mut command = diagnostic_command(OsStr::new("rustc"));
        command.env(OBSERVED, "diagnostic-hash");
        let variables: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(variables.get(OsStr::new(RECEIPT)), Some(&None));
        assert_eq!(
            variables.get(OsStr::new(OBSERVED)),
            Some(&Some(OsStr::new("diagnostic-hash")))
        );
        assert_eq!(
            variables.get(OsStr::new("RUSTC_WORKSPACE_WRAPPER")),
            Some(&Some(OsStr::new("")))
        );
        let cargo = diagnostic_command(OsStr::new("cargo"));
        assert!(
            cargo
                .get_envs()
                .any(|(name, value)| name == OBSERVED && value.is_none())
        );
        for name in [
            "CARGO_BUILD_RUSTC",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
        ] {
            assert!(forbidden_environment(name));
        }
    }

    #[test]
    fn diagnostic_report_cannot_declare_qualification() {
        let derived = receipt::Derived {
            digest: "observed".into(),
            payload: serde_json::json!({"qualification_eligible": false}),
        };
        let source = source_stamp::SourceStamp {
            full: "full".into(),
            qualification: "source".into(),
            file_count: 3,
            inputs: Vec::new(),
        };
        let report = diagnostic_report(&derived, &source);
        assert_eq!(report["qualification_eligible"], false);
        assert!(report.get("receipt").is_none());
        assert_eq!(report["observed_receipt"], "observed");
        assert!(
            report["status"]
                .as_str()
                .unwrap()
                .contains("qualification unavailable")
        );
    }
}
