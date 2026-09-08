use super::super::search::{self, SettingId};
use super::controller_diagram::{DiagramAction, DiagramKind};
use crate::settings::{
    AutofireOverride, AutofirePattern, AutofireTarget, BindingAction, InputScope, Settings,
};

const ACTIONS: [(BindingAction, SettingId); 8] = [
    (BindingAction::A, SettingId::InputDevicesAutofireA),
    (BindingAction::B, SettingId::InputDevicesAutofireB),
    (BindingAction::X, SettingId::InputDevicesAutofireX),
    (BindingAction::Y, SettingId::InputDevicesAutofireY),
    (BindingAction::L, SettingId::InputDevicesAutofireL),
    (BindingAction::R, SettingId::InputDevicesAutofireR),
    (BindingAction::Start, SettingId::InputDevicesAutofireStart),
    (BindingAction::Select, SettingId::InputDevicesAutofireSelect),
];

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    scope: &InputScope,
    player: u8,
    kind: DiagramKind,
) {
    let reveal = ACTIONS.iter().any(|(_, id)| search::requested(ui, *id));
    egui::CollapsingHeader::new(format!("Autofire · Player {player}"))
        .id_salt(("autofire", player))
        .open(reveal.then_some(true))
        .show(ui, |ui| {
            ui.label("Hold a mapped button to repeat it on emulated frames. Each press starts ON, then repeats the chosen ON/OFF pattern.");
            ui.label(egui::RichText::new("Applies to keyboard and controller input in this scope. Replays play their recorded inputs. Available buttons depend on the system and connected ports.").weak());
            for (action, id) in ACTIONS {
                let target = AutofireTarget { player, action };
                let resolved = settings.autofire(scope, target);
                let mut enabled = resolved.value.is_some();
                let mut pattern = resolved.value.unwrap_or_default();
                let unsupported_ws = kind == DiagramKind::WonderSwan && !matches!(action, BindingAction::A | BindingAction::B | BindingAction::Start);
                let mut changed = false;
                let mut inherit = false;
                ui.push_id(target.key(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let response = ui.add_enabled(!unsupported_ws, egui::Checkbox::new(&mut enabled, kind.action_label(DiagramAction::Joypad(action))));
                        search::target(ui, id, &response);
                        changed |= response.changed();
                        if unsupported_ws { ui.weak("Unavailable on WonderSwan"); }
                        if resolved.origin != *scope {
                            ui.weak(format!("From {}", super::scope_label(&resolved.origin)));
                        } else if *scope != InputScope::Global {
                            inherit = ui.small_button("Use inherited").clicked();
                        }
                    });
                    if enabled && !unsupported_ws {
                        ui.vertical(|ui| {
                            changed |= ui.add(egui::Slider::new(&mut pattern.period_frames, 1..=AutofirePattern::MAX_PERIOD_FRAMES).text("Period (frames)")).changed();
                            if pattern.on_frames > pattern.period_frames {
                                pattern.on_frames = pattern.period_frames;
                                changed = true;
                            }
                            changed |= ui.add(egui::Slider::new(&mut pattern.on_frames, 1..=pattern.period_frames).text("ON (frames)")).changed();
                            ui.weak(format!("{} ON / {} OFF", pattern.on_frames, pattern.period_frames - pattern.on_frames));
                        });
                    }
                });
                if inherit {
                    settings.inherit_autofire(scope, target);
                } else if changed && let Err(error) = settings.set_autofire(scope, target, if enabled { AutofireOverride::enabled(pattern) } else { AutofireOverride::disabled() }) {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
            ui.label(egui::RichText::new("The global Turbo shortcut keeps its alternating Player 1 behavior and starts OFF.").weak());
        });
}
