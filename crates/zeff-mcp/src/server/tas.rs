use super::*;

impl Server {
    pub(super) fn tool_tas_open(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_open",
            "path": required_string(args, "path")?,
        }))
    }

    pub(super) fn tool_tas_create(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_create",
            "path": required_string(args, "path")?,
            "replace_existing": optional_bool(args, "replace_existing").unwrap_or(false),
        }))
    }

    pub(super) fn tool_tas_link(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_link",
            "at_end": optional_bool(args, "at_end").unwrap_or(false),
            "record": optional_bool(args, "record").unwrap_or(false),
        }))
    }

    pub(super) fn tool_tas_select(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_select",
            "boundary": optional_u64(args, "boundary")
                .context("missing required integer argument: boundary")?,
        }))
    }

    pub(super) fn tool_tas_select_range(&self, args: &Value) -> anyhow::Result<Value> {
        let start = optional_u64(args, "start")
            .context("missing required non-negative integer argument: start")?;
        let end = optional_u64(args, "end")
            .context("missing required non-negative integer argument: end")?;
        if start >= end {
            bail!("TAS selection range must satisfy start < end");
        }
        if end > 1_000_000_000 {
            bail!("TAS selection end must not exceed 1000000000");
        }
        self.call_live(json!({
            "command": "tas_select_range",
            "start": start,
            "end": end,
        }))
    }

    pub(super) fn tool_tas_insert_neutral_frames(&self, args: &Value) -> anyhow::Result<Value> {
        let boundary = optional_u64(args, "boundary")
            .context("missing required non-negative integer argument: boundary")?;
        let count = optional_u64(args, "count")
            .context("missing required positive integer argument: count")?;
        if boundary > 1_000_000_000 {
            bail!("TAS insertion boundary must not exceed 1000000000");
        }
        if count == 0 || count > 1_000_000_000 {
            bail!("TAS frame count must be between 1 and 1000000000");
        }
        self.call_live(json!({
            "command": "tas_insert_neutral_frames",
            "boundary": boundary,
            "count": count,
        }))
    }

    pub(super) fn tool_tas_set_input(&self, args: &Value) -> anyhow::Result<Value> {
        let frame = optional_u64(args, "frame")
            .context("missing required non-negative integer argument: frame")?;
        if frame >= 1_000_000_000 {
            bail!("TAS input frame must be less than 1000000000");
        }
        let player = optional_u64(args, "player").unwrap_or(1);
        if !(1..=5).contains(&player) {
            bail!("player must be 1 through 5");
        }
        let control = required_string(args, "control")?;
        if !is_tas_digital_control(&control) {
            bail!("unknown TAS digital control: {control}");
        }
        let pressed =
            optional_bool(args, "pressed").context("missing required boolean argument: pressed")?;
        self.call_live(json!({
            "command": "tas_set_input",
            "frame": frame,
            "player": player,
            "control": control,
            "pressed": pressed,
        }))
    }

    pub(super) fn tool_tas_fork_branch(&self, args: &Value) -> anyhow::Result<Value> {
        let mut request = json!({
            "command": "tas_fork_branch",
            "branch_id": required_string(args, "id")?,
        });
        if let Some(name) = optional_string(args, "name") {
            request["name"] = json!(name);
        }
        self.call_live(request)
    }

    pub(super) fn tool_tas_recording(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_recording",
            "action": optional_string(args, "action").unwrap_or_else(|| "start".to_owned()),
        }))
    }

    pub(super) fn tool_tas_playback(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_playback",
            "action": optional_string(args, "action").unwrap_or_else(|| "start".to_owned()),
        }))
    }

    pub(super) fn tool_tas_disconnect(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_disconnect",
            "keep": optional_bool(args, "keep").unwrap_or(false),
        }))
    }

    pub(super) fn tool_tas_record_frame(&self, args: &Value) -> anyhow::Result<Value> {
        let mut request = json!({ "command": "tas_record_frame" });
        if let Some(mode) = optional_string(args, "mode") {
            request["mode"] = json!(mode);
        }
        self.call_live(request)
    }
}
