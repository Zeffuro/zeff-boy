use std::io::BufWriter;
use std::path::Path;

use super::*;

#[test]
#[ignore = "manual GPU-backed settings layout captures; requires an output directory"]
fn render_settings_fixtures() -> anyhow::Result<()> {
    let output = std::env::var_os("ZEFF_SETTINGS_CAPTURE_DIR").ok_or_else(|| {
        anyhow::anyhow!("set ZEFF_SETTINGS_CAPTURE_DIR before rendering fixtures")
    })?;
    let output = std::path::PathBuf::from(output);
    std::fs::create_dir_all(&output)?;
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

    for (name, category, input_page, width, height, scale) in [
        (
            "general-desktop",
            SettingsCategory::General,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "audio-desktop",
            SettingsCategory::Audio,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "video-desktop",
            SettingsCategory::Video,
            InputDevicesPage::Controls,
            1100,
            1150,
            1.0,
        ),
        (
            "emulation-desktop",
            SettingsCategory::Emulation,
            InputDevicesPage::Controls,
            1100,
            1250,
            1.0,
        ),
        (
            "firmware-before-scan",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "firmware-after-scan",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            1180,
            1.0,
        ),
        (
            "firmware-details",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            1250,
            1.0,
        ),
        (
            "firmware-light",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            1180,
            1.0,
        ),
        (
            "firmware-scanning",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "firmware-narrow",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            480,
            1200,
            1.0,
        ),
        (
            "storage-desktop",
            SettingsCategory::Storage,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "camera-desktop",
            SettingsCategory::Camera,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "interface-dark",
            SettingsCategory::Interface,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "interface-light",
            SettingsCategory::Interface,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "debugger-desktop",
            SettingsCategory::Debugger,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "debugger-colors",
            SettingsCategory::Debugger,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "search-rewind",
            SettingsCategory::General,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "search-colors",
            SettingsCategory::General,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "controls-standard",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-gb",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-gba",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-nes",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-pce2",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-pce6",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-ms",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-gg",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-sg1000",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-coleco",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-ws",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            480,
            1100,
            1.0,
        ),
        (
            "controls-200pct",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1800,
            2.0,
        ),
        (
            "profiles-empty",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "profiles-saved",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1050,
            1.0,
        ),
        (
            "profiles-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            480,
            1500,
            1.0,
        ),
        (
            "hotkeys-desktop",
            SettingsCategory::InputDevices,
            InputDevicesPage::Hotkeys,
            1100,
            1100,
            1.0,
        ),
        (
            "test-desktop",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            1100,
            1000,
            1.0,
        ),
        (
            "test-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            480,
            1100,
            1.0,
        ),
        (
            "test-no-device",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            1100,
            900,
            1.0,
        ),
        (
            "audio-narrow",
            SettingsCategory::Audio,
            InputDevicesPage::Controls,
            480,
            1100,
            1.0,
        ),
        (
            "video-narrow",
            SettingsCategory::Video,
            InputDevicesPage::Controls,
            480,
            1400,
            1.0,
        ),
    ] {
        let mut settings = Settings::default();
        if name != "profiles-empty" {
            settings
                .save_input_profile("Living room controllers")
                .map_err(anyhow::Error::msg)?;
        }
        let mut state = DebugWindowState::new();
        state.settings_ui.category = category;
        state.settings_ui.input_page = input_page;
        state.settings_ui.selected_profile_id = Some("profile-1".into());
        use controls::controller_diagram::DiagramKind;
        state.settings_ui.controller_layout = Some(match name {
            "controls-standard" => DiagramKind::StandardGamepad,
            "controls-gb" => DiagramKind::GameBoy,
            "controls-nes" => DiagramKind::Nes,
            "controls-pce2" => DiagramKind::PceTwoButton,
            "controls-pce6" => DiagramKind::PceSixButton,
            "controls-ms" => DiagramKind::SegaMasterSystem,
            "controls-gg" => DiagramKind::GameGear,
            "controls-sg1000" => DiagramKind::Sg1000,
            "controls-ws" => DiagramKind::WonderSwan,
            "controls-coleco" => DiagramKind::Coleco,
            _ => DiagramKind::GameBoyAdvance,
        });
        if name == "controls-gb" {
            state.settings_ui.binding_source = controls::BindingSource::Keyboard;
        }
        if name == "interface-light" || name == "firmware-light" {
            settings.ui.theme_preset = crate::settings::UiThemePreset::Light;
        }
        state.camera_devices_needs_refresh = false;
        if name == "search-rewind" {
            state.settings_ui.search = "rewind".into();
        }
        if name == "search-colors" {
            state.settings_ui.search = "color".into();
        }
        if name.starts_with("firmware-")
            && name != "firmware-before-scan"
            && name != "firmware-scanning"
        {
            state.firmware_inventory.needs_refresh = false;
            state.firmware_inventory.rows = firmware_rows();
        }
        let mut _scan_sender = None;
        if name == "firmware-scanning" {
            let (sender, receiver) = std::sync::mpsc::channel();
            _scan_sender = Some(sender);
            state.firmware_inventory.scan_receiver = Some(receiver);
        }
        let fingerprint = crate::settings::GamepadFingerprint {
            name: "Wireless Controller".into(),
            uuid: "030000005e0400008e02000000000000".into(),
        };
        for index in 0..2 {
            state
                .settings_ui
                .gamepad_snapshot
                .devices
                .push(crate::input::GamepadDeviceSnapshot {
                    id: crate::input::RuntimeGamepadId(index + 1),
                    fingerprint: fingerprint.clone(),
                    buttons: if index == 0 {
                        vec!["South".into()]
                    } else {
                        Vec::new()
                    },
                    left_stick: if index == 0 {
                        (0.72, -0.15)
                    } else {
                        (0.02, 0.01)
                    },
                    right_stick: if index == 0 {
                        (-0.42, 0.65)
                    } else {
                        (0.0, 0.0)
                    },
                    waiting_for_neutral: false,
                });
            let player = &mut state.settings_ui.gamepad_snapshot.players[index as usize];
            player.status = crate::input::GamepadAssignmentStatus::Connected;
            player.device = Some(crate::input::RuntimeGamepadId(index + 1));
            player.buttons = if index == 0 {
                crate::input::HostButton::A.host_mask_bit()
            } else {
                0
            };
        }
        if name == "test-no-device" {
            state.settings_ui.gamepad_snapshot = crate::input::GamepadSnapshot::default();
        }
        render(
            &device,
            &queue,
            &output.join(format!("{name}.png")),
            width,
            height,
            scale,
            &mut settings,
            &mut state,
        )?;
    }
    Ok(())
}

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
    let open_label = if name.starts_with("profiles-") {
        Some("Saved input profiles")
    } else if name == "debugger-colors" {
        Some("Debugger colors")
    } else if name == "firmware-details" {
        Some("File details")
    } else {
        None
    };
    for frame in 0..24 {
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
