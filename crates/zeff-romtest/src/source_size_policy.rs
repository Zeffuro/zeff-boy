use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use serde::Deserialize;

const SOFT_LINE_LIMIT: usize = 500;
const HARD_LINE_LIMIT: usize = 900;
const BASELINE_VERSION: u32 = 1;
const BASELINE_PATH: &str = "crates/zeff-romtest/source-size-baseline.toml";

#[derive(Debug, Deserialize)]
struct Baseline {
    version: u32,
    #[serde(default)]
    grandfathered: BTreeMap<String, usize>,
    #[serde(default)]
    exceptions: BTreeMap<String, Exception>,
}

#[derive(Debug, Deserialize)]
struct Exception {
    max_lines: usize,
    reason: String,
}

#[derive(Debug, PartialEq, Eq)]
struct AuditReport {
    tracked_files: usize,
    soft_limit_files: usize,
    grandfathered_files: usize,
    exception_files: usize,
}

pub(crate) fn audit_repository() -> anyhow::Result<()> {
    let root = repository_root();
    let baseline_path = root.join(BASELINE_PATH);
    let baseline = parse_baseline(&baseline_path)?;
    let line_counts = source_line_counts(&root)?;
    let report = evaluate(&line_counts, &baseline)?;

    println!(
        "source-size policy passed: {} first-party Rust files; {} exceed the informational {}-line target; {} grandfathered and {} allowlisted over-{} files",
        report.tracked_files,
        report.soft_limit_files,
        SOFT_LINE_LIMIT,
        report.grandfathered_files,
        report.exception_files,
        HARD_LINE_LIMIT,
    );
    Ok(())
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("zeff-romtest must live two directories below the repository root")
        .to_path_buf()
}

fn parse_baseline(path: &Path) -> anyhow::Result<Baseline> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read source-size baseline {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("failed to parse source-size baseline {}", path.display()))
}

fn source_line_counts(root: &Path) -> anyhow::Result<BTreeMap<String, usize>> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "*.rs",
            ":(exclude)third_party/**",
        ])
        .output()
        .context("failed to run git ls-files for source-size policy")?;
    if !output.status.success() {
        bail!("git ls-files failed while checking source-size policy");
    }

    let mut line_counts = BTreeMap::new();
    for raw_path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(raw_path)
            .context("git ls-files returned a non-UTF-8 Rust source path")?;
        let source_path = root.join(path);
        if !source_path.is_file() {
            continue;
        }
        let source = fs::read_to_string(&source_path).with_context(|| {
            format!(
                "failed to read tracked Rust source {}",
                source_path.display()
            )
        })?;
        line_counts.insert(path.to_string(), source.lines().count());
    }
    Ok(line_counts)
}

fn evaluate(
    line_counts: &BTreeMap<String, usize>,
    baseline: &Baseline,
) -> anyhow::Result<AuditReport> {
    validate_baseline(line_counts, baseline)?;

    let mut violations = Vec::new();
    let mut soft_limit_files = 0;
    for (path, line_count) in line_counts {
        if *line_count > SOFT_LINE_LIMIT {
            soft_limit_files += 1;
        }
        if *line_count <= HARD_LINE_LIMIT {
            continue;
        }

        if let Some(exception) = baseline.exceptions.get(path) {
            if *line_count > exception.max_lines {
                violations.push(format!(
                    "{path} has {line_count} lines, above its allowlisted maximum of {} ({})",
                    exception.max_lines, exception.reason
                ));
            }
        } else if let Some(grandfathered_maximum) = baseline.grandfathered.get(path) {
            if *line_count > *grandfathered_maximum {
                violations.push(format!(
                    "{path} grew from its grandfathered baseline of {grandfathered_maximum} to {line_count} lines; add a bounded [exceptions] entry with a reason before growing it"
                ));
            }
        } else {
            violations.push(format!(
                "new first-party source file {path} has {line_count} lines, above the {HARD_LINE_LIMIT}-line limit; add a bounded [exceptions] entry with a reason before landing it"
            ));
        }
    }

    if !violations.is_empty() {
        bail!("source-size policy violation:\n{}", violations.join("\n"));
    }

    Ok(AuditReport {
        tracked_files: line_counts.len(),
        soft_limit_files,
        grandfathered_files: baseline.grandfathered.len(),
        exception_files: baseline.exceptions.len(),
    })
}

fn validate_baseline(
    line_counts: &BTreeMap<String, usize>,
    baseline: &Baseline,
) -> anyhow::Result<()> {
    if baseline.version != BASELINE_VERSION {
        bail!(
            "source-size baseline version {} is unsupported; expected {}",
            baseline.version,
            BASELINE_VERSION
        );
    }

    let tracked_paths = line_counts.keys().cloned().collect::<BTreeSet<_>>();
    for (path, maximum) in &baseline.grandfathered {
        validate_baseline_path(path, &tracked_paths, "grandfathered")?;
        if *maximum <= HARD_LINE_LIMIT {
            bail!(
                "grandfathered source-size baseline for {path} must be above {HARD_LINE_LIMIT} lines"
            );
        }
        if line_counts[path] <= HARD_LINE_LIMIT {
            bail!(
                "grandfathered source-size baseline for {path} is stale; remove it now that the file is at or below {HARD_LINE_LIMIT} lines"
            );
        }
    }

    for (path, exception) in &baseline.exceptions {
        validate_baseline_path(path, &tracked_paths, "exception")?;
        if exception.max_lines <= HARD_LINE_LIMIT {
            bail!("source-size exception for {path} must set max_lines above {HARD_LINE_LIMIT}");
        }
        if exception.reason.trim().is_empty() {
            bail!("source-size exception for {path} must include a non-empty reason");
        }
        if line_counts[path] <= HARD_LINE_LIMIT {
            bail!(
                "source-size exception for {path} is stale; remove it now that the file is at or below {HARD_LINE_LIMIT} lines"
            );
        }
    }
    Ok(())
}

fn validate_baseline_path(
    path: &str,
    tracked_paths: &BTreeSet<String>,
    category: &str,
) -> anyhow::Result<()> {
    if path.contains('\\') || Path::new(path).is_absolute() || path.starts_with("./") {
        bail!(
            "{category} source-size path '{path}' must be a repository-relative slash-separated Rust path"
        );
    }
    if !path.ends_with(".rs") || !tracked_paths.contains(path) {
        bail!("{category} source-size path '{path}' is not a tracked first-party Rust source file");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEST_REPOSITORY: AtomicUsize = AtomicUsize::new(0);

    fn line_counts(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
        entries
            .iter()
            .map(|(path, count)| ((*path).to_string(), *count))
            .collect()
    }

    fn baseline() -> Baseline {
        Baseline {
            version: BASELINE_VERSION,
            grandfathered: BTreeMap::from([("src/existing.rs".to_string(), 950)]),
            exceptions: BTreeMap::new(),
        }
    }

    #[test]
    fn accepts_soft_target_and_grandfathered_file_at_its_baseline() {
        let counts = line_counts(&[("src/soft.rs", 501), ("src/existing.rs", 950)]);

        assert_eq!(
            evaluate(&counts, &baseline()).unwrap(),
            AuditReport {
                tracked_files: 2,
                soft_limit_files: 2,
                grandfathered_files: 1,
                exception_files: 0,
            }
        );
    }

    #[test]
    fn rejects_new_or_growing_over_limit_sources() {
        let counts = line_counts(&[("src/existing.rs", 951), ("src/new.rs", 901)]);
        let error = evaluate(&counts, &baseline()).unwrap_err().to_string();

        assert!(error.contains("src/existing.rs grew from its grandfathered baseline"));
        assert!(error.contains("new first-party source file src/new.rs"));
    }

    #[test]
    fn bounded_exception_requires_a_reason_and_sets_the_allowed_maximum() {
        let counts = line_counts(&[("src/existing.rs", 975)]);
        let mut allowed = baseline();
        allowed.exceptions.insert(
            "src/existing.rs".to_string(),
            Exception {
                max_lines: 975,
                reason: "A split is scheduled with the next subsystem change.".to_string(),
            },
        );
        assert!(evaluate(&counts, &allowed).is_ok());

        allowed
            .exceptions
            .get_mut("src/existing.rs")
            .unwrap()
            .max_lines = 974;
        assert!(
            evaluate(&counts, &allowed)
                .unwrap_err()
                .to_string()
                .contains("above its allowlisted maximum")
        );

        allowed
            .exceptions
            .get_mut("src/existing.rs")
            .unwrap()
            .reason = "  ".to_string();
        assert!(
            evaluate(&counts, &allowed)
                .unwrap_err()
                .to_string()
                .contains("must include a non-empty reason")
        );
    }

    #[test]
    fn reasoned_exception_can_admit_a_new_over_limit_file() {
        let counts = line_counts(&[("src/new.rs", 901)]);
        let mut allowed = Baseline {
            version: BASELINE_VERSION,
            grandfathered: BTreeMap::new(),
            exceptions: BTreeMap::new(),
        };
        allowed.exceptions.insert(
            "src/new.rs".to_string(),
            Exception {
                max_lines: 901,
                reason: "The generated protocol table must remain adjacent to its parser."
                    .to_string(),
            },
        );

        assert!(evaluate(&counts, &allowed).is_ok());
    }

    #[test]
    fn inventory_includes_untracked_sources_and_excludes_ignored_ones() {
        let root = std::env::temp_dir().join(format!(
            "zeff-romtest-source-size-{}-{}",
            std::process::id(),
            NEXT_TEST_REPOSITORY.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(root.join("tracked.rs"), "fn tracked() {}\n").unwrap();
        fs::write(root.join("untracked.rs"), "fn untracked() {}\n").unwrap();
        fs::write(root.join("ignored.rs"), "fn ignored() {}\n").unwrap();

        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(init.success());
        let add = Command::new("git")
            .args(["add", ".gitignore", "tracked.rs"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(add.success());

        let line_counts = source_line_counts(&root).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(
            line_counts,
            BTreeMap::from([
                ("tracked.rs".to_string(), 1),
                ("untracked.rs".to_string(), 1),
            ])
        );
    }

    #[test]
    fn rejects_non_source_baseline_entries() {
        let counts = line_counts(&[("src/existing.rs", 950)]);
        let mut invalid = baseline();
        invalid
            .grandfathered
            .insert("third_party/vendor.rs".to_string(), 950);
        assert!(
            evaluate(&counts, &invalid)
                .unwrap_err()
                .to_string()
                .contains("not a tracked first-party Rust source file")
        );
    }

    #[test]
    fn rejects_stale_entries_after_a_file_is_split_below_the_limit() {
        let counts = line_counts(&[("src/existing.rs", HARD_LINE_LIMIT)]);
        let error = evaluate(&counts, &baseline()).unwrap_err().to_string();

        assert!(error.contains("is stale"));
    }
}
