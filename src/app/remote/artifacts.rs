use serde_json::{Value, json};

use crate::app::App;
use crate::emu_thread::{EmuCommand, EmuResponse};

impl App {
    pub(super) fn write_live_screenshot(
        &self,
        requested_path: Option<std::path::PathBuf>,
    ) -> anyhow::Result<Value> {
        let frame = self
            .latest_display_frame_snapshot()
            .ok_or_else(|| anyhow::anyhow!("no framebuffer is available yet"))?;
        let (width, height) = self
            .display_size_for_frame_len(frame.len())
            .ok_or_else(|| anyhow::anyhow!("unexpected framebuffer size: {} bytes", frame.len()))?;

        let path = resolve_screenshot_path(requested_path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_rgba_png(&path, width, height, &frame)?;

        Ok(json!({
            "path": path.display().to_string(),
            "width": width,
            "height": height,
            "bytes": frame.len(),
        }))
    }

    pub(super) fn live_save_state(
        &mut self,
        requested_path: Option<std::path::PathBuf>,
    ) -> anyhow::Result<Value> {
        anyhow::ensure!(self.emu_thread.is_some(), "no ROM is running");
        anyhow::ensure!(
            self.core_supports_save_states(),
            "the active core does not support save states"
        );

        let path = resolve_state_path(requested_path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::SaveStateToPath(path.clone()));
        }

        match self.recv_cold_response() {
            Some(EmuResponse::SaveStateOk(saved)) => Ok(json!({
                "path": saved,
                "requested_path": path.display().to_string(),
            })),
            Some(EmuResponse::SaveStateFailed(err)) => anyhow::bail!(err),
            Some(_) => anyhow::bail!("unexpected emulator response while saving state"),
            None => anyhow::bail!("emulator thread stopped while saving state"),
        }
    }

    pub(super) fn live_load_state(
        &mut self,
        requested_path: std::path::PathBuf,
    ) -> anyhow::Result<Value> {
        anyhow::ensure!(self.emu_thread.is_some(), "no ROM is running");
        anyhow::ensure!(
            self.core_supports_save_states(),
            "the active core does not support save states"
        );

        let path = resolve_state_path(Some(requested_path))?;
        if let Some(thread) = &self.emu_thread {
            let (buttons_pressed, dpad_pressed) = self.current_host_joypad_input();
            thread.send(EmuCommand::LoadStateFromPath {
                path: path.clone(),
                buttons_pressed,
                dpad_pressed,
            });
        }

        match self.recv_cold_response() {
            Some(EmuResponse::LoadStateOk {
                path: loaded,
                media_slot_snapshot,
                game_boy_serial_device,
            }) => {
                self.media_slot_snapshot = media_slot_snapshot;
                if let Some(device) = game_boy_serial_device {
                    self.game_boy_serial_device = device;
                }
                if let Some(thread) = &self.emu_thread {
                    self.latest_frame = thread.shared_framebuffer().load_full();
                }
                self.remote_debug_frames_remaining = 3;
                self.remote_graphics_frames_remaining = 3;
                Ok(json!({
                    "path": loaded,
                    "status": self.live_status_json(),
                }))
            }
            Some(EmuResponse::LoadStateFailed(err)) => anyhow::bail!(err),
            Some(_) => anyhow::bail!("unexpected emulator response while loading state"),
            None => anyhow::bail!("emulator thread stopped while loading state"),
        }
    }
}

fn resolve_screenshot_path(
    requested_path: Option<std::path::PathBuf>,
) -> anyhow::Result<std::path::PathBuf> {
    resolve_live_artifact_path(requested_path.unwrap_or_else(default_screenshot_path))
}

fn resolve_state_path(
    requested_path: Option<std::path::PathBuf>,
) -> anyhow::Result<std::path::PathBuf> {
    resolve_live_artifact_path(requested_path.unwrap_or_else(default_state_path))
}

fn resolve_live_artifact_path(path: std::path::PathBuf) -> anyhow::Result<std::path::PathBuf> {
    let repo_root = std::env::current_dir()?;
    let absolute = lexical_normalize_path(if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    });

    let allowed_results = lexical_normalize_path(repo_root.join("rom-tests").join("results"));
    let allowed_temp = lexical_normalize_path(repo_root.join("temp"));

    if !absolute.starts_with(&allowed_results) && !absolute.starts_with(&allowed_temp) {
        anyhow::bail!("live-control artifacts must be under ignored rom-tests/results/ or temp/");
    }

    Ok(absolute)
}

fn lexical_normalize_path(path: std::path::PathBuf) -> std::path::PathBuf {
    let mut normalized = std::path::PathBuf::new();
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

fn default_screenshot_path() -> std::path::PathBuf {
    std::path::PathBuf::from("rom-tests")
        .join("results")
        .join("live-control")
        .join("screenshots")
        .join(format!("zeff-{}.png", current_millis()))
}

fn default_state_path() -> std::path::PathBuf {
    std::path::PathBuf::from("rom-tests")
        .join("results")
        .join("live-control")
        .join("states")
        .join(format!("zeff-{}.state", current_millis()))
}

fn current_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

fn write_rgba_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    frame: &[u8],
) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(frame)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_screenshot_path_stays_under_results() {
        let path = default_screenshot_path();
        assert!(path.starts_with(std::path::Path::new("rom-tests").join("results")));
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("png"));
    }

    #[test]
    fn screenshot_path_rejects_tracked_folders() {
        let err = resolve_screenshot_path(Some(std::path::PathBuf::from("src/live-control.png")))
            .unwrap_err();
        assert!(err.to_string().contains("ignored"));
    }

    #[test]
    fn screenshot_path_allows_results_folder() {
        let path = resolve_screenshot_path(Some(std::path::PathBuf::from(
            "rom-tests/results/live-control/test.png",
        )))
        .unwrap();
        assert!(
            path.ends_with(
                std::path::Path::new("rom-tests")
                    .join("results")
                    .join("live-control")
                    .join("test.png")
            )
        );
    }

    #[test]
    fn default_state_path_stays_under_results() {
        let path = default_state_path();
        assert!(path.starts_with(std::path::Path::new("rom-tests").join("results")));
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("state"));
    }
}
