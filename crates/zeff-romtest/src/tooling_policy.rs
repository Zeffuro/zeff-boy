use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

const APPROVED_POWERSHELL_SCRIPTS: &[&str] = &[
    "build-coleco-cvbasic-controller.ps1",
    "build-mgba-suite.ps1",
    "build-pgo.ps1",
    "build-ws-test-suite.ps1",
    "compare-interrupt-trace.ps1",
    "scan-sega8-romset.ps1",
    "smoke-pce-romset.ps1",
    "submit-winget.ps1",
    "test-wasm-browser-speculation.ps1",
];

const REMOVED_WRAPPERS: &[&str] = &[
    "build-nes-regional-acceptance-roms.ps1",
    "build-pce-cd-adpcm-fixture.ps1",
    "build-pce-vdc-contention-fixture.ps1",
    "build-sega8-smoke-roms.ps1",
    "get-all-code.ps1",
    "test-wasm-speculation.ps1",
];

pub(crate) fn audit_repository() -> anyhow::Result<()> {
    let root = repository_root();
    let actual_scripts = tracked_powershell_scripts(&root)?;
    let approved_scripts = APPROVED_POWERSHELL_SCRIPTS
        .iter()
        .map(|name| (*name).to_string())
        .collect::<BTreeSet<_>>();
    if actual_scripts != approved_scripts {
        bail!(
            "PowerShell policy violation: scripts/ must contain only the approved boundary scripts; expected {approved_scripts:?}, found {actual_scripts:?}"
        );
    }

    let justfile = read(&root.join("justfile"))?;
    let ci = read(&root.join(".github/workflows/ci.yml"))?;
    let release = read(&root.join(".github/workflows/release.yml"))?;
    let gitignore = read(&root.join(".gitignore"))?;
    let pgo_script = read(&root.join("scripts/build-pgo.ps1"))?;
    let sources = read(&root.join("rom-tests/sources.toml"))?;
    let manifests = read_tracked_tree(&root, "rom-tests/manifests")?;
    let public_control_plane = format!("{justfile}\n{ci}\n{release}\n{sources}\n{manifests}");

    for removed in REMOVED_WRAPPERS {
        if public_control_plane.contains(removed) {
            bail!("removed wrapper '{removed}' is still referenced by the public tooling surface");
        }
    }

    for script in [
        "build-coleco-cvbasic-controller.ps1",
        "build-mgba-suite.ps1",
        "build-pgo.ps1",
        "build-ws-test-suite.ps1",
        "compare-interrupt-trace.ps1",
        "scan-sega8-romset.ps1",
        "smoke-pce-romset.ps1",
    ] {
        require_contains(&justfile, script, "justfile")?;
    }
    require_contains(&ci, "test-wasm-browser-speculation.ps1", "CI workflow")?;
    require_contains(&release, "submit-winget.ps1", "release workflow")?;
    require_contains(&gitignore, "/local-artifacts/", ".gitignore")?;
    require_contains(&pgo_script, "local-artifacts\\pgo", "PGO build script")?;
    if pgo_script.contains("target\\pgo") || pgo_script.contains("target/pgo") {
        bail!(
            "tooling policy violation: durable PGO sessions must stay outside Cargo's target tree"
        );
    }

    let slow_gate =
        "if: github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'";
    for job_name in [
        "ROM tests",
        "Benchmarks",
        "WASM detached-frame proof (Node)",
        "WASM browser proof (Edge)",
        "Fuzz compile check",
    ] {
        require_job_gate(&ci, job_name, slow_gate)?;
    }
    if ci.matches(slow_gate).count() != 5 {
        bail!("CI tier policy violation: exactly five slow jobs must be schedule/manual-only");
    }

    Ok(())
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("zeff-romtest must live two directories below the repository root")
        .to_path_buf()
}

fn read(path: &Path) -> anyhow::Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

fn tracked_powershell_scripts(root: &Path) -> anyhow::Result<BTreeSet<String>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "scripts/*.ps1"])
        .output()
        .context("failed to run git ls-files for tooling policy")?;
    if !output.status.success() {
        bail!("git ls-files failed while checking the PowerShell policy");
    }
    Ok(String::from_utf8(output.stdout)
        .context("git ls-files returned non-UTF-8 script paths")?
        .lines()
        .filter(|path| root.join(path).is_file())
        .filter_map(|path| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect())
}

fn read_tracked_tree(root: &Path, pathspec: &str) -> anyhow::Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--", pathspec])
        .output()
        .with_context(|| format!("failed to list tracked files under {pathspec}"))?;
    if !output.status.success() {
        bail!("git ls-files failed while listing tracked files under {pathspec}");
    }

    let paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .with_context(|| format!("git returned a non-UTF-8 path under {pathspec}"))
                .map(|path| root.join(path))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut contents = String::new();
    for path in paths {
        contents.push_str(&read(&path)?);
        contents.push('\n');
    }
    Ok(contents)
}

fn require_contains(contents: &str, needle: &str, surface: &str) -> anyhow::Result<()> {
    if contents.contains(needle) {
        Ok(())
    } else {
        bail!("tooling policy violation: '{needle}' needs an explicit owner in {surface}")
    }
}

fn require_job_gate(ci: &str, name: &str, gate: &str) -> anyhow::Result<()> {
    let name_line = format!("    name: {name}");
    let start = ci
        .find(&name_line)
        .with_context(|| format!("CI workflow is missing the '{name}' job"))?;
    let remainder = &ci[start + name_line.len()..];
    let job = std::iter::once(name_line.as_str())
        .chain(
            remainder
                .lines()
                .take_while(|line| line.is_empty() || line.starts_with("    ")),
        )
        .collect::<Vec<_>>()
        .join("\n");

    if job.contains(gate) {
        Ok(())
    } else {
        bail!("CI tier policy violation: '{name}' must be schedule/manual-only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_tooling_has_a_single_owner_for_every_powershell_boundary() {
        audit_repository().unwrap();
    }

    #[test]
    fn slow_gate_must_belong_to_the_named_job() {
        let gate =
            "if: github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'";
        let workflow = format!(
            "jobs:\n  romtest:\n    name: ROM tests\n    runs-on: ubuntu-latest\n  bench:\n    name: Benchmarks\n    {gate}\n"
        );

        assert!(require_job_gate(&workflow, "Benchmarks", gate).is_ok());
        assert!(require_job_gate(&workflow, "ROM tests", gate).is_err());
    }
}
