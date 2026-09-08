use crate::debug::DebugWindowState;
use crate::settings::{Settings, ShortcutAction};

use super::super::{
    layout::row,
    search::{self, SettingId as Id},
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    let requested = [Id::InputDevicesSpeedUpKey, Id::InputDevicesRewindKey]
        .into_iter()
        .chain(ShortcutAction::ALL.iter().copied().map(shortcut_id))
        .any(|id| search::requested(ui, id));
    egui::CollapsingHeader::new("Shortcuts")
        .default_open(true)
        .open(requested.then_some(true))
        .show(ui, |ui| {
            if state.rebinding_shortcut.is_some()
                || state.rebinding_speedup
                || state.rebinding_rewind
            {
                ui.label(
                    egui::RichText::new("Press a key to rebind...").color(egui::Color32::YELLOW),
                );
            }
            if row(ui, Id::InputDevicesSpeedUpKey, None, |ui| {
                let key_label = if state.rebinding_speedup {
                    "Press a key…".to_owned()
                } else {
                    super::joypad::key_label(settings.speedup_key_code())
                };
                ui.add_sized([240.0, 26.0], egui::Button::new(key_label))
            })
            .clicked()
            {
                super::joypad::clear_capture(state);
                state.rebinding_speedup = true;
            }
            if row(ui, Id::InputDevicesRewindKey, None, |ui| {
                let key_label = if state.rebinding_rewind {
                    "Press a key…".to_owned()
                } else {
                    super::joypad::key_label(settings.rewind.key_code())
                };
                ui.add_sized([240.0, 26.0], egui::Button::new(key_label))
            })
            .clicked()
            {
                super::joypad::clear_capture(state);
                state.rebinding_rewind = true;
            }
            for &action in ShortcutAction::ALL {
                let id = shortcut_id(action);
                if row(ui, id, None, |ui| {
                    let capture_label = if state.rebinding_shortcut == Some(action) {
                        "Press a key…".to_owned()
                    } else {
                        super::joypad::key_label(settings.shortcut_bindings.get(action))
                    };
                    ui.add_sized([240.0, 26.0], egui::Button::new(capture_label))
                })
                .clicked()
                {
                    super::joypad::clear_capture(state);
                    state.rebinding_shortcut = Some(action);
                }
            }
        });
}

pub(super) fn shortcut_id(action: ShortcutAction) -> Id {
    match action {
        ShortcutAction::Pause => Id::InputDevicesPauseResumeShortcut,
        ShortcutAction::Fullscreen => Id::InputDevicesFullscreenShortcut,
        ShortcutAction::SlowMotion => Id::InputDevicesToggleSlowMotionShortcut,
        ShortcutAction::UncappedSpeed => Id::InputDevicesToggleUncappedShortcut,
        ShortcutAction::MuteToggle => Id::InputDevicesMuteToggleShortcut,
        ShortcutAction::Screenshot => Id::InputDevicesScreenshotShortcut,
        ShortcutAction::ResetGame => Id::InputDevicesResetGameShortcut,
        ShortcutAction::FrameAdvance => Id::InputDevicesFrameAdvanceShortcut,
        ShortcutAction::QuickSave => Id::InputDevicesQuickSaveShortcut,
        ShortcutAction::QuickLoad => Id::InputDevicesQuickLoadShortcut,
        ShortcutAction::SlotNext => Id::InputDevicesNextSaveSlotShortcut,
        ShortcutAction::SlotPrev => Id::InputDevicesPreviousSaveSlotShortcut,
        ShortcutAction::RotateWs => Id::InputDevicesRotateWonderswanShortcut,
        ShortcutAction::DebugContinue => Id::InputDevicesRunDebuggerShortcut,
        ShortcutAction::DebugStep => Id::InputDevicesStepDebuggerShortcut,
    }
}
