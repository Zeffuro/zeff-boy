use super::{MdTimedPortUi, MdTimedPortView};
use crate::debug::DebugUiActions;
use zeff_md_core::emulator::DebugPortWrite;

pub(in crate::debug) fn draw(
    ui: &mut egui::Ui,
    view: &MdTimedPortView,
    state: &mut MdTimedPortUi,
    actions: &mut DebugUiActions,
) {
    ui.separator();
    ui.strong("Timed Mega Drive ports");
    ui.weak("Writes advance hardware time while the CPU remains paused.");
    ui.monospace(format!(
        "SOUND request:{}  granted:{}  reset:{}",
        u8::from(view.sound_requested),
        u8::from(view.sound_granted),
        u8::from(view.sound_reset_asserted)
    ));
    let editable = view.can_submit && state.can_queue();
    if !view.can_submit {
        ui.weak("Pause a healthy Mega Drive CPU to submit a batch.");
    }
    ui.add_enabled_ui(editable, |ui| {
        for (address, label, data) in [
            (0xc00000, "C00000 VDP data", true),
            (0xc00004, "C00004 VDP control", false),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                let text = if data {
                    &mut state.data
                } else {
                    &mut state.control
                };
                ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(72.0)
                        .hint_text("0000"),
                );
                if ui.button("Queue word").clicked() {
                    let text = if data {
                        state.data.clone()
                    } else {
                        state.control.clone()
                    };
                    state.notice = state.word(address, &text).err();
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("A11100 SOUND request");
            ui.selectable_value(&mut state.request_sound, false, "Release (0)");
            ui.selectable_value(&mut state.request_sound, true, "Request (1)");
            if ui.button("Queue byte").clicked() {
                state.draft.push(DebugPortWrite::Byte {
                    address: 0xa11100,
                    value: u8::from(state.request_sound),
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label("A11200 SOUND reset");
            ui.selectable_value(&mut state.release_reset, false, "Assert (0)");
            ui.selectable_value(&mut state.release_reset, true, "Release (1)");
            if ui.button("Queue byte").clicked() {
                state.draft.push(DebugPortWrite::Byte {
                    address: 0xa11200,
                    value: u8::from(state.release_reset),
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label("C00011 PSG");
            ui.add(
                egui::TextEdit::singleline(&mut state.psg)
                    .desired_width(72.0)
                    .hint_text("00"),
            );
            if ui.button("Queue byte").clicked() {
                state.notice = state.psg_byte(&state.psg.clone()).err();
            }
        });
        ui.horizontal(|ui| {
            ui.label("SOUND RAM / YM / bank");
            ui.add(
                egui::TextEdit::singleline(&mut state.sound_address)
                    .desired_width(80.0)
                    .hint_text("A00000"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut state.sound_data)
                    .desired_width(48.0)
                    .hint_text("00"),
            );
            if ui.button("Queue byte").clicked() {
                state.notice = state
                    .sound_byte(&state.sound_address.clone(), &state.sound_data.clone())
                    .err();
            }
        });
        ui.weak("Aperture: A00000–A01FFF RAM, A04000–A04003 YM, A06000 bank.");
        ui.weak(
            "Without a SOUND bus grant, or during reset, writes may acknowledge without effect.",
        );
        ui.horizontal(|ui| {
            ui.label("UART TX / control");
            ui.add(
                egui::TextEdit::singleline(&mut state.uart_address)
                    .desired_width(80.0)
                    .hint_text("A1000F"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut state.uart_data)
                    .desired_width(48.0)
                    .hint_text("00"),
            );
            if ui.button("Queue byte").clicked() {
                state.notice = state
                    .uart_byte(&state.uart_address.clone(), &state.uart_data.clone())
                    .err();
            }
        });
        ui.weak("TX: A1000F / A10015. Control: A10013 / A10019.");
        ui.weak(
            "Unsupported control changes or timing are rejected; TX status may be unavailable.",
        );
        if let Some(ports) = view.save {
            ui.monospace(format!(
                "SRAM {:06X}–{:06X}; control {:06X}; enabled:{} protected:{}",
                ports.data_start,
                ports.data_end,
                ports.control,
                u8::from(ports.enabled),
                u8::from(ports.write_protected),
            ));
            ui.weak("Odd data bytes store; even data bytes acknowledge without changing SRAM.");
            ui.weak(
                "Control 01 unprotects; 03 protects. Other values may fault. No disable control.",
            );
            ui.horizontal(|ui| {
                ui.label("SRAM data / control byte");
                ui.add(
                    egui::TextEdit::singleline(&mut state.save_address)
                        .desired_width(80.0)
                        .hint_text("200001"),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut state.save_data)
                        .desired_width(48.0)
                        .hint_text("00"),
                );
                if ui.button("Queue byte").clicked() {
                    state.notice = state
                        .save_byte(
                            view.save,
                            &state.save_address.clone(),
                            &state.save_data.clone(),
                        )
                        .err();
                }
            });
        } else {
            ui.weak("Timed SRAM writes are unavailable for this cartridge owner.");
        }
        for (index, write) in state.draft.iter().enumerate() {
            ui.monospace(match write {
                DebugPortWrite::Word { address, value } => {
                    format!("{index}: {address:06X} Word {value:04X}")
                }
                DebugPortWrite::Byte { address, value } => {
                    let lane = if address & 1 == 0 { "Upper" } else { "Lower" };
                    format!("{index}: {address:06X} {lane} byte {value:02X}")
                }
            });
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !state.draft.is_empty(),
                    egui::Button::new("Submit ordered batch"),
                )
                .clicked()
            {
                match state.prepare() {
                    Ok(request) => actions.md_timed_ports = Some(request),
                    Err(error) => state.notice = Some(error),
                }
            }
            if ui.button("Clear batch").clicked() {
                state.draft.clear();
                state.notice = None;
            }
        });
    });
    ui.label(state.status());
    if let Some(notice) = &state.notice {
        ui.colored_label(ui.visuals().error_fg_color, notice);
    }
}
