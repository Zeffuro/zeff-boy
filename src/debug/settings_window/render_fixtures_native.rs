use std::io::BufWriter;
use std::path::Path;

use super::*;

#[path = "render_fixtures_native/cases_native.rs"]
mod cases;
#[allow(clippy::too_many_arguments)]
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    path: &Path,
    width: u32,
    height: u32,
    scale: f32,
    settings: &mut Settings,
    state: &mut DebugWindowState,
) -> anyhow::Result<()> {
    let context = egui::Context::default();
    crate::graphics::apply_egui_theme(
        &context,
        settings.ui.theme_preset,
        settings.ui.ui_density,
        settings.ui.debug_monospace_scale,
        settings.ui.effective_debug_colors(),
    );
    context.set_pixels_per_point(scale);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer =
        egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [width, height],
        pixels_per_point: scale,
    };
    let mut output = egui::FullOutput::default();
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let open_label = if name == "main-menu-800x600" {
        Some("File")
    } else if name == "debugger-colors" {
        Some("Debugger colors")
    } else if name == "firmware-details" {
        Some("File details")
    } else if name == "binding-capture" {
        Some("Add alternative")
    } else if name == "binding-axis-narrow" {
        Some("Right stick X options")
    } else if name == "calibration-active" {
        Some("Start center sampling")
    } else if name == "timing-active" {
        Some("Input timing measurements")
    } else {
        None
    };
    for frame in 0..24 {
        if frame == 1 {
            let target = if name == "search-conditional" {
                Some(search::SettingId::AudioLowPassCutoff)
            } else if name == "profiles-manage" {
                Some(search::SettingId::InputDevicesRenameSelectedProfile)
            } else if name.starts_with("profiles-") {
                Some(search::SettingId::InputDevicesSaveCurrentAsProfile)
            } else if name == "input-tilt-scoped" {
                Some(search::SettingId::InputDevicesTiltSensitivity)
            } else if name.starts_with("autofire-") {
                Some(search::SettingId::InputDevicesAutofireA)
            } else if name == "calibration-active" {
                Some(search::SettingId::InputDevicesStartCalibration)
            } else {
                None
            };
            if let Some(target) = target {
                search::begin(&context, target);
            }
        }
        let mut events = Vec::new();
        if (frame == 3 || frame == 4)
            && let Some(label) = open_label
            && let Some(position) = find_label(&output, label)
        {
            events.push(egui::Event::PointerMoved(position));
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: frame == 3,
                modifiers: egui::Modifiers::NONE,
            });
        }
        output = context.run_ui(
            egui::RawInput {
                time: Some(f64::from(frame) / 60.0),
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width as f32 / scale, height as f32 / scale),
                )),
                ..Default::default()
            },
            |ui| {
                if name == "main-menu-800x600" {
                    let labels = std::array::from_fn(|_| String::new());
                    let occupied = [false; 10];
                    let mut dock = egui_dock::DockState::new(Vec::new());
                    crate::debug::draw_menu_bar(
                        ui,
                        &crate::debug::MenuBarContext {
                            current_mode: crate::graphics::AspectRatioMode::KeepAspect,
                            speed_mode_label: None,
                            is_recording_audio: false,
                            is_recording_replay: false,
                            is_playing_replay: false,
                            supports_save_states: false,
                            supports_replay: false,
                            supports_audio: false,
                            supports_debugger: false,
                            is_paused: false,
                            active_system: ActiveSystem::Gb,
                            media_slot_snapshot: None,
                            media_event_change_allowed: false,
                            game_boy_serial_device: Default::default(),
                            game_boy_serial_device_change_allowed: false,
                            ws_display_rotated: false,
                            slot_labels: &labels,
                            slot_occupied: &occupied,
                            active_save_slot: 0,
                            can_undo_load_state: false,
                            can_undo_save_state: false,
                            recovery_state_available: false,
                            external_debugger: false,
                            debugger_window_open: false,
                            debug_presentation: settings.ui.debug_presentation,
                        },
                        &mut dock,
                        settings,
                        state,
                    );
                    egui::CentralPanel::default().show(ui, |_| {});
                    return;
                }
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(ui.visuals().panel_fill)
                            .inner_margin(8),
                    )
                    .show(ui, |ui| {
                        draw_settings_content(
                            ui,
                            settings,
                            state,
                            &SettingsContext {
                                active_system: Some(ActiveSystem::Pce),
                                gb_hardware_mode_label: None,
                                is_pocket_camera: false,
                            },
                        );
                    });
            },
        );
        for (id, delta) in &output.textures_delta.set {
            renderer.update_texture(device, queue, *id, delta);
        }
    }
    let jobs = context.tessellate(output.shapes, output.pixels_per_point);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("settings fixture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("settings fixture readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let callbacks = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("settings fixture pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.08,
                            g: 0.08,
                            b: 0.12,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            })
            .forget_lifetime();
        renderer.render(&mut pass, &jobs, &screen);
    }
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(
        callbacks
            .into_iter()
            .chain(std::iter::once(encoder.finish())),
    );
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)),
    })?;
    receive.recv_timeout(std::time::Duration::from_secs(30))??;
    let mapped = buffer.slice(..).get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks_exact(padded_row as usize) {
        pixels.extend_from_slice(&row[..(width * 4) as usize]);
    }
    let mut png = png::Encoder::new(BufWriter::new(std::fs::File::create(path)?), width, height);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.write_header()?.write_image_data(&pixels)?;
    drop(mapped);
    buffer.unmap();
    println!("{}", path.display());
    Ok(())
}

fn find_label(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| {
        if let egui::Shape::Text(text) = &shape.shape
            && text.galley.job.text.starts_with(label)
        {
            Some(text.pos + egui::vec2(8.0, text.galley.size().y * 0.5))
        } else {
            None
        }
    })
}

fn firmware_rows() -> Vec<crate::debug::types::FirmwareInventoryRow> {
    use crate::debug::types::{FirmwareInventoryRow, FirmwareInventoryStatusKind as Status};
    [
        (
            "nintendo.gb.boot.dmg",
            "Game Boy",
            "Game Boy DMG boot ROM",
            Some("C:/Games/Firmware/dmg_boot.bin"),
            Status::Recognized,
        ),
        (
            "nintendo.gb.boot.dmg",
            "Game Boy",
            "Game Boy DMG boot ROM",
            Some("C:/Games/Firmware/Backup/dmg_boot.bin"),
            Status::Recognized,
        ),
        (
            "nintendo.gb.boot.cgb",
            "Game Boy Color",
            "Game Boy Color boot ROM",
            Some("C:/Games/Firmware/Alternative collection/Color handheld/cgb_boot.bin"),
            Status::UnknownHash,
        ),
        (
            "nintendo.gb.boot.cgb",
            "Game Boy Color",
            "Game Boy Color boot ROM",
            Some("C:/Games/Firmware/Backup/cgb_boot.bin"),
            Status::Recognized,
        ),
        (
            "nintendo.gba.bios",
            "Game Boy Advance",
            "Game Boy Advance BIOS",
            Some("C:/Games/Firmware/gba_bios.bin"),
            Status::Recognized,
        ),
        (
            "nintendo.gba.bios",
            "Game Boy Advance",
            "Game Boy Advance BIOS",
            Some("C:/Games/Firmware/Alternative collection/gba_bios.bin"),
            Status::UnknownHash,
        ),
        (
            "nintendo.fds.bios",
            "NES / Famicom Disk System",
            "Famicom Disk System BIOS",
            None,
            Status::NotFound,
        ),
        (
            "coleco.vision.bios",
            "ColecoVision",
            "ColecoVision BIOS",
            Some("C:/Games/Firmware/coleco.rom"),
            Status::WrongSize,
        ),
        (
            "sega.sms.boot",
            "Master System",
            "Master System boot ROM",
            Some("C:/Games/Firmware/bios.sms"),
            Status::Recognized,
        ),
        (
            "sega.sms.boot",
            "Master System",
            "Master System boot ROM",
            Some("C:/Games/Firmware/Backup/bios.sms"),
            Status::Recognized,
        ),
        (
            "sega.gg.boot",
            "Game Gear",
            "Game Gear boot ROM",
            None,
            Status::NotFound,
        ),
    ]
    .into_iter()
    .map(
        |(id, system, firmware, path, status)| FirmwareInventoryRow {
            firmware_id: id.into(),
            system: system.into(),
            firmware: firmware.into(),
            path: path.map(str::to_owned),
            status,
            detail: match status {
                Status::Recognized => "Matches a recognized firmware entry.",
                Status::UnknownHash => {
                    "The size matches, but this file has an unrecognized SHA-256."
                }
                Status::WrongSize => "The file size does not match the expected firmware size.",
                Status::NotFound => "No matching firmware file was found.",
            }
            .into(),
            sha256_prefix: path.map(|_| "cF053eCb4cCc".to_ascii_lowercase()),
            managed_key: None,
        },
    )
    .collect()
}
