use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::Server;
use super::pair_gb_trade_fixture::ensure_deadline;
use super::pair_gb_trade_fixture_config::GbTradeFixtureConfig;

impl Server {
    pub(super) fn clear_gb_trade_fast_forward(&self, left_addr: &str, right_addr: &str) {
        let _ = self.call_live_at(
            left_addr,
            json!({ "command": "fast_forward", "enabled": false }),
        );
        let _ = self.call_live_at(
            right_addr,
            json!({ "command": "fast_forward", "enabled": false }),
        );
    }

    pub(super) fn stop_partial_gb_trade_recording(&self, left_addr: &str, right_addr: &str) {
        let _ = self.call_live_at(left_addr, json!({ "command": "stop_replay" }));
        let _ = self.call_live_at(right_addr, json!({ "command": "stop_replay" }));
    }

    pub(super) fn load_state_pair(
        &self,
        left_addr: &str,
        right_addr: &str,
        state_path: &Path,
    ) -> anyhow::Result<()> {
        let path = state_path.display().to_string();
        self.call_live_at(left_addr, json!({ "command": "load_state", "path": path }))?;
        self.call_live_at(right_addr, json!({ "command": "load_state", "path": path }))?;
        std::thread::sleep(Duration::from_millis(500));
        Ok(())
    }

    pub(super) fn start_replay_pair(
        &self,
        left_addr: &str,
        right_addr: &str,
        config: &GbTradeFixtureConfig,
    ) -> anyhow::Result<()> {
        self.call_live_at(
            left_addr,
            json!({
                "command": "record_replay",
                "path": config.left_replay_path.display().to_string(),
            }),
        )?;
        self.call_live_at(
            right_addr,
            json!({
                "command": "record_replay",
                "path": config.right_replay_path.display().to_string(),
            }),
        )?;
        Ok(())
    }

    pub(super) fn stop_replay_pair(
        &self,
        left_addr: &str,
        right_addr: &str,
        config: &GbTradeFixtureConfig,
        deadline: Instant,
    ) -> anyhow::Result<Value> {
        self.call_live_at(left_addr, json!({ "command": "stop_replay" }))?;
        self.call_live_at(right_addr, json!({ "command": "stop_replay" }))?;

        let left = self.wait_replay_saved(left_addr, &config.left_replay_path, deadline)?;
        let right = self.wait_replay_saved(right_addr, &config.right_replay_path, deadline)?;
        Ok(json!({
            "left_path": config.left_replay_path.display().to_string(),
            "right_path": config.right_replay_path.display().to_string(),
            "left": left,
            "right": right,
        }))
    }

    pub(super) fn host_join_pair(
        &self,
        left_addr: &str,
        right_addr: &str,
        link_addr: &str,
    ) -> anyhow::Result<()> {
        self.call_live_at(
            left_addr,
            json!({ "command": "host_link", "addr": link_addr }),
        )?;
        std::thread::sleep(Duration::from_millis(600));
        self.call_live_at(
            right_addr,
            json!({ "command": "join_link", "addr": link_addr }),
        )?;
        std::thread::sleep(Duration::from_millis(800));
        Ok(())
    }

    pub(super) fn wait_pair_recording_progress(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        let left_start = replay_recorded_frames(&self.status(left_addr)?);
        let right_start = replay_recorded_frames(&self.status(right_addr)?);
        loop {
            ensure_deadline(deadline)?;
            std::thread::sleep(Duration::from_millis(500));
            let left = self.status(left_addr)?;
            let right = self.status(right_addr)?;
            if replay_recording_active(&left)
                && replay_recording_active(&right)
                && replay_recorded_frames(&left) > left_start + 5
                && replay_recorded_frames(&right) > right_start + 5
            {
                return Ok(());
            }
        }
    }

    pub(super) fn wait_replay_saved(
        &self,
        addr: &str,
        path: &Path,
        deadline: Instant,
    ) -> anyhow::Result<Value> {
        loop {
            ensure_deadline(deadline)?;
            let status = self.status(addr)?;
            let replay = status.get("replay").cloned().unwrap_or(Value::Null);
            let still_active = replay
                .get("starting")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || replay
                    .get("recording")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                || replay
                    .get("saving")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            if !still_active && bytes > 0 {
                return Ok(json!({
                    "saved": true,
                    "bytes": bytes,
                    "status": status,
                }));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    pub(super) fn status(&self, addr: &str) -> anyhow::Result<Value> {
        self.call_live_at(addr, json!({ "command": "status" }))
    }
}

fn replay_recording_active(status: &Value) -> bool {
    status
        .pointer("/replay/recording")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn replay_recorded_frames(status: &Value) -> u64 {
    status
        .pointer("/replay/recorded_frames")
        .and_then(Value::as_u64)
        .unwrap_or(0)
}
