use crate::debug::ui_helpers::{EnumLabel, enum_combo_box};
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

impl EnumLabel for HardwareModePreference {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::ForceDmg => "DMG",
            Self::ForceSgb => "SGB",
            Self::ForceCgb => "CGB",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Auto, Self::ForceDmg, Self::ForceSgb, Self::ForceCgb]
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
) {
    ui.heading("Speed");
    ui.add(
        egui::Slider::new(&mut settings.emulation.fast_forward_multiplier, 1..=16)
            .text("Fast-forward multiplier"),
    );
    ui.add(
        egui::Slider::new(&mut settings.emulation.uncapped_frames_per_tick, 1..=240)
            .text("Uncapped frames/tick"),
    );
    ui.checkbox(
        &mut settings.emulation.uncapped_speed,
        "Start in uncapped mode",
    );
    ui.checkbox(&mut settings.emulation.frame_skip, "Frame skip when behind")
        .on_hover_text(
            "When enabled, skip emulation frames to stay in real-time if the \
             host can't keep up. When disabled, the emulator catches up \
             gradually (more accurate, may drift behind).",
        );
    ui.checkbox(
        &mut settings.emulation.auto_save_state,
        "Auto save/load state",
    )
    .on_hover_text(
        "Automatically save emulator state when closing and \
             restore it when loading the same ROM.",
    );
    ui.checkbox(
        &mut settings.emulation.pause_on_unfocus,
        "Pause when window loses focus",
    )
    .on_hover_text(
        "Automatically pause emulation when the window or browser tab \
             loses focus, and resume when it regains focus.",
    );

    ui.separator();
    ui.heading("Rewind");
    ui.checkbox(&mut settings.rewind.enabled, "Enable rewind")
        .on_hover_text(
            "Hold the rewind key to rewind gameplay. \
             Captures a snapshot every 4 frames (~15 fps capture rate).",
        );
    ui.horizontal(|ui| {
        ui.label("History (seconds):");
        ui.add(
            egui::DragValue::new(&mut settings.rewind.seconds)
                .range(1..=120)
                .speed(1),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Rewind speed:");
        ui.add(
            egui::DragValue::new(&mut settings.rewind.speed)
                .range(1..=10)
                .speed(1),
        );
        ui.label(match settings.rewind.speed {
            1 => "(fastest - pop every tick)",
            2 => "(fast)",
            3..=4 => "(normal)",
            _ => "(slow)",
        });
    });

    ui.separator();
    super::draw_console_section_header(ui, "Game Boy", active_system, ActiveSystem::GameBoy);
    enum_combo_box(
        ui,
        "Hardware mode",
        &mut settings.emulation.hardware_mode_preference,
    );
    ui.label(
        egui::RichText::new("Selects DMG, SGB, or CGB hardware when loading a Game Boy ROM.")
            .weak()
            .small(),
    );
    ui.checkbox(
        &mut settings.emulation.sgb_border_enabled,
        "Enable SGB border rendering",
    )
    .on_hover_text(
        "When enabled, renders Super Game Boy borders for compatible ROMs. \
         Requires SGB or Auto hardware mode and an SGB-supported ROM.",
    );
    ui.checkbox(
        &mut settings.emulation.nes_zapper_enabled,
        "Enable NES Zapper (Light Gun)",
    )
    .on_hover_text(
        "When enabled, replaces Player 2 controller with a Zapper light gun. \
         Click the game screen to fire. Only works with NES games that support the Zapper.",
    );
}
