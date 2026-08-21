use super::MenuAction;
use crate::debug::DebugWindowState;
use crate::debug::dock::{DebugTab, toggle_dock_tab};
use crate::emu_backend::ActiveSystem;
use egui_dock::DockState;

pub(super) struct ToolsMenuState<'a> {
    pub(super) active_system: ActiveSystem,
    pub(super) media_slot_snapshot: Option<&'a zeff_emu_common::media::MediaSlotSnapshot>,
    pub(super) media_event_change_allowed: bool,
    pub(super) game_boy_serial_device: zeff_gb_core::hardware::GameBoySerialDevice,
    pub(super) game_boy_serial_device_change_allowed: bool,
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    actions: &mut Vec<MenuAction>,
    dock_state: &mut DockState<DebugTab>,
    debug_windows: &mut DebugWindowState,
    state: ToolsMenuState<'_>,
) {
    let ToolsMenuState {
        active_system,
        media_slot_snapshot,
        media_event_change_allowed,
        game_boy_serial_device,
        game_boy_serial_device_change_allowed,
    } = state;
    if ui.button("Cheats").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Cheats);
        ui.close();
    }
    if ui.button("Mods").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Mods);
        ui.close();
    }
    if let Some(snapshot) = media_slot_snapshot {
        ui.separator();
        ui.add_enabled_ui(media_event_change_allowed, |ui| {
            ui.menu_button("FDS Disk", |ui| {
                draw_fds_menu(ui, actions, snapshot);
            });
        });
    }
    ui.separator();
    ui.label("Game Boy Link Port");
    let gb_enabled = active_system == ActiveSystem::GameBoy;
    ui.add_enabled_ui(gb_enabled && game_boy_serial_device_change_allowed, |ui| {
        ui.menu_button("Attached Device", |ui| {
            for (device, label) in [
                (
                    zeff_gb_core::hardware::GameBoySerialDevice::Disconnected,
                    "Disconnected",
                ),
                (
                    zeff_gb_core::hardware::GameBoySerialDevice::Printer,
                    "Game Boy Printer",
                ),
                (
                    zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader,
                    "Bardigun Barcode Reader",
                ),
                (
                    zeff_gb_core::hardware::GameBoySerialDevice::BarcodeBoy,
                    "Barcode Boy",
                ),
            ] {
                if ui.radio(game_boy_serial_device == device, label).clicked()
                    && game_boy_serial_device != device
                {
                    actions.push(MenuAction::SetGameBoySerialDevice(device));
                    ui.close();
                }
            }
        });
    });
    if !gb_enabled {
        ui.label("Load GB/GBC content first");
    }
    #[cfg(not(target_arch = "wasm32"))]
    if ui
        .add_enabled(
            gb_enabled
                && game_boy_serial_device_change_allowed
                && game_boy_serial_device
                    == zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader,
            egui::Button::new("Scan Bardigun Card..."),
        )
        .clicked()
    {
        actions.push(MenuAction::ScanBardigunBarcodeFile);
        ui.close();
    }
    #[cfg(target_arch = "wasm32")]
    if game_boy_serial_device == zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader
    {
        ui.label("Card-file scanning is native-only");
    }
    if ui
        .add_enabled(
            gb_enabled
                && game_boy_serial_device_change_allowed
                && game_boy_serial_device
                    == zeff_gb_core::hardware::GameBoySerialDevice::BarcodeBoy,
            egui::Button::new("Scan Barcode Boy..."),
        )
        .clicked()
    {
        actions.push(MenuAction::OpenBarcodeBoyScan);
        ui.close();
    }
    if ui
        .add_enabled(
            gb_enabled || debug_windows.printer.len() != 0,
            egui::Button::new("Printer Output"),
        )
        .clicked()
    {
        actions.push(MenuAction::OpenPrinterWindow);
        ui.close();
    }
    ui.separator();
    ui.label("Link Cable");
    let remote_link_enabled = cfg!(not(target_arch = "wasm32"))
        && crate::link::remote_link_system_for_active_system(active_system).is_some();
    #[cfg(target_arch = "wasm32")]
    ui.label("TCP link is native-only");
    if ui
        .add_enabled(remote_link_enabled, egui::Button::new("Host TCP Link"))
        .on_hover_text(
            "Open this after loading the first GB/GBC or WonderSwan/WSC ROM, then Join from another app instance",
        )
        .clicked()
    {
        actions.push(MenuAction::HostTcpLink);
        ui.close();
    }
    if ui
        .add_enabled(remote_link_enabled, egui::Button::new("Join TCP Link"))
        .on_hover_text("Join a localhost link hosted by another app instance")
        .clicked()
    {
        actions.push(MenuAction::JoinTcpLink);
        ui.close();
    }
    if ui.button("Disconnect Link").clicked() {
        actions.push(MenuAction::DisconnectLink);
        ui.close();
    }
    ui.separator();
    ui.label("PPU Layers");
    if ui
        .checkbox(&mut debug_windows.layer_enable_bg, "Background")
        .changed()
    {
        debug_windows.gba_layer_enable_bg = [debug_windows.layer_enable_bg; 4];
    }
    let mut gba_bg_changed = false;
    ui.add_enabled_ui(debug_windows.layer_enable_bg, |ui| {
        ui.horizontal(|ui| {
            ui.label("GBA");
            for bg in 0..4 {
                gba_bg_changed |= ui
                    .checkbox(
                        &mut debug_windows.gba_layer_enable_bg[bg],
                        format!("BG{bg}"),
                    )
                    .changed();
            }
        });
    });
    if gba_bg_changed {
        debug_windows.layer_enable_bg = debug_windows
            .gba_layer_enable_bg
            .iter()
            .any(|&enabled| enabled);
    }
    ui.checkbox(&mut debug_windows.layer_enable_window, "Window");
    ui.checkbox(&mut debug_windows.layer_enable_sprites, "Sprites");
}

fn draw_fds_menu(
    ui: &mut egui::Ui,
    actions: &mut Vec<MenuAction>,
    snapshot: &zeff_emu_common::media::MediaSlotSnapshot,
) {
    use zeff_emu_common::media::MediaEvent;

    let selected_side = snapshot.state.side;
    for side in 0..snapshot.side_count {
        let label = fds_side_label(side);
        let clicked = if snapshot.inserted() {
            ui.add_enabled(
                selected_side != Some(side),
                egui::RadioButton::new(selected_side == Some(side), label),
            )
            .clicked()
        } else {
            ui.button(format!("Insert {label}")).clicked()
        };
        if clicked {
            let Some(event) = media_event_for_side(snapshot, side) else {
                continue;
            };
            actions.push(MenuAction::ApplyMediaEvent(event));
            ui.close();
        }
    }

    if snapshot.inserted() {
        ui.separator();
        let mut write_protected = snapshot.state.write_protected;
        if ui
            .checkbox(&mut write_protected, "Write protected")
            .changed()
        {
            actions.push(MenuAction::ApplyMediaEvent(MediaEvent::SetWriteProtected {
                slot: snapshot.state.slot.clone(),
                write_protected,
            }));
            ui.close();
        }
        if ui.button("Eject Disk").clicked() {
            actions.push(MenuAction::ApplyMediaEvent(MediaEvent::Eject {
                slot: snapshot.state.slot.clone(),
            }));
            ui.close();
        }
    }
}

fn media_event_for_side(
    snapshot: &zeff_emu_common::media::MediaSlotSnapshot,
    side: u8,
) -> Option<zeff_emu_common::media::MediaEvent> {
    use zeff_emu_common::media::MediaEvent;

    if side >= snapshot.side_count {
        return None;
    }
    if snapshot.inserted() {
        if snapshot.state.side == Some(side) {
            return None;
        }
        return Some(MediaEvent::SelectSide {
            slot: snapshot.state.slot.clone(),
            side,
        });
    }
    Some(MediaEvent::Insert {
        slot: snapshot.state.slot.clone(),
        media_id: snapshot.source_media_id.clone()?,
        side: Some(side),
        write_protected: false,
    })
}

fn fds_side_label(side: u8) -> String {
    let disk = usize::from(side) / 2 + 1;
    let face = if side.is_multiple_of(2) { 'A' } else { 'B' };
    format!("Disk {disk}, Side {face}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_emu_common::media::{
        MediaEvent, MediaObjectId, MediaSlotId, MediaSlotSnapshot, MediaSlotState,
    };

    fn snapshot(inserted: bool, side_count: u8) -> MediaSlotSnapshot {
        let media_id = MediaObjectId::from("sha256:test");
        MediaSlotSnapshot {
            state: MediaSlotState {
                slot: MediaSlotId::from("fds.drive0"),
                media_id: inserted.then(|| media_id.clone()),
                side: inserted.then_some(0),
                write_protected: false,
                mutation_counter: 0,
            },
            source_media_id: Some(media_id),
            side_count,
        }
    }

    #[test]
    fn fds_side_labels_group_faces_by_disk() {
        assert_eq!(fds_side_label(0), "Disk 1, Side A");
        assert_eq!(fds_side_label(1), "Disk 1, Side B");
        assert_eq!(fds_side_label(2), "Disk 2, Side A");
        assert_eq!(fds_side_label(3), "Disk 2, Side B");
    }

    #[test]
    fn side_action_selects_inserted_media_and_inserts_ejected_media() {
        assert!(matches!(
            media_event_for_side(&snapshot(true, 2), 1),
            Some(MediaEvent::SelectSide { side: 1, .. })
        ));
        assert!(matches!(
            media_event_for_side(&snapshot(false, 4), 3),
            Some(MediaEvent::Insert { side: Some(3), .. })
        ));
        assert_eq!(media_event_for_side(&snapshot(true, 2), 0), None);
        assert_eq!(media_event_for_side(&snapshot(true, 1), 1), None);
    }
}
