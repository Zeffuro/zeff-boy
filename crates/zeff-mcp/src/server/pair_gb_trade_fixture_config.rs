use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::Value;

use crate::args::{optional_bool, optional_string, optional_u64};

#[derive(Clone, Debug)]
pub(super) struct GbTradeFixtureConfig {
    pub(super) state_path: PathBuf,
    pub(super) left_replay_path: PathBuf,
    pub(super) right_replay_path: PathBuf,
    pub(super) link_addr: String,
    pub(super) left_party_index: u8,
    pub(super) right_party_index: u8,
    pub(super) record_replay: bool,
    pub(super) fast_forward: bool,
    pub(super) timeout_seconds: u64,
}

impl GbTradeFixtureConfig {
    pub(super) fn from_args(repo_root: &Path, args: &Value) -> anyhow::Result<Self> {
        Ok(Self {
            state_path: configured_path(
                repo_root,
                args,
                &["state_path", "state"],
                default_pair_path("gb-trade-fixture.state"),
            ),
            left_replay_path: configured_path(
                repo_root,
                args,
                &["left_replay_path", "host_replay_path"],
                default_recording_path("automated-host-trade.zrpl"),
            ),
            right_replay_path: configured_path(
                repo_root,
                args,
                &["right_replay_path", "join_replay_path"],
                default_recording_path("automated-join-trade.zrpl"),
            ),
            link_addr: optional_string(args, "link_addr")
                .or_else(|| optional_string(args, "addr"))
                .unwrap_or_else(|| "127.0.0.1:8765".to_string()),
            left_party_index: party_index_arg(args, "left_party_index", 2)?,
            right_party_index: party_index_arg(args, "right_party_index", 0)?,
            record_replay: optional_bool(args, "record_replay").unwrap_or(true),
            fast_forward: optional_bool(args, "fast_forward").unwrap_or(true),
            timeout_seconds: optional_u64(args, "timeout_seconds")
                .unwrap_or(240)
                .clamp(30, 600),
        })
    }

    pub(super) fn prepare_paths(&self, repo_root: &Path) -> anyhow::Result<()> {
        ensure_artifact_path(repo_root, &self.state_path)?;
        anyhow::ensure!(
            self.state_path.is_file(),
            "GB trade fixture state is missing: {}",
            self.state_path.display()
        );

        if self.record_replay {
            for path in [&self.left_replay_path, &self.right_replay_path] {
                ensure_artifact_path(repo_root, path)?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
            }
        }

        Ok(())
    }
}

fn configured_path(repo_root: &Path, args: &Value, keys: &[&str], default: PathBuf) -> PathBuf {
    let path = keys
        .iter()
        .find_map(|key| optional_string(args, key).map(PathBuf::from))
        .unwrap_or(default);
    normalize_path(if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    })
}

fn default_pair_path(file_name: &str) -> PathBuf {
    PathBuf::from("temp").join("pair-run").join(file_name)
}

fn default_recording_path(file_name: &str) -> PathBuf {
    PathBuf::from("temp")
        .join("pair-run")
        .join("automated-recording")
        .join(file_name)
}

fn ensure_artifact_path(repo_root: &Path, path: &Path) -> anyhow::Result<()> {
    let path = normalize_path(path.to_path_buf());
    let temp = normalize_path(repo_root.join("temp"));
    let results = normalize_path(repo_root.join("rom-tests").join("results"));
    if path.starts_with(&temp) || path.starts_with(&results) {
        Ok(())
    } else {
        bail!("GB trade fixture paths must stay under temp/ or rom-tests/results/")
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn party_index_arg(args: &Value, key: &str, default: u8) -> anyhow::Result<u8> {
    let value = optional_u64(args, key).unwrap_or(u64::from(default));
    if value <= 5 {
        Ok(value as u8)
    } else {
        bail!("{key} must be between 0 and 5")
    }
}
