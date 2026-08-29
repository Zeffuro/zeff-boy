use crate::debug::ui_helpers::{EnumLabel, enum_combo_box};
use crate::emu_backend::ActiveSystem;
use crate::settings::{
    PceArcadeCardPreference, PceCdArchiveMemoryLimit, PceConsoleWiringPreference,
    PceControllerPreference, PceMemoryBasePreference, PceMouseCursorMode, RewindMode,
    Sega8ConsoleRegionPreference, Sega8VideoStandardPreference, Settings,
};
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

impl EnumLabel for Sega8VideoStandardPreference {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Ntsc => "NTSC",
            Self::Pal => "PAL",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Auto, Self::Ntsc, Self::Pal]
    }
}

impl EnumLabel for Sega8ConsoleRegionPreference {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Export => "Export",
            Self::Japanese => "Japanese",
            Self::JapanesePowerBaseConverter => "Japanese PBC",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[
            Self::Auto,
            Self::Export,
            Self::Japanese,
            Self::JapanesePowerBaseConverter,
        ]
    }
}

impl EnumLabel for PceConsoleWiringPreference {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::PcEngine => "PC Engine",
            Self::TurboGrafx16 => "TurboGrafx-16",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Auto, Self::PcEngine, Self::TurboGrafx16]
    }
}

impl EnumLabel for PceControllerPreference {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::TwoButton => "2-button pad",
            Self::SixButton => "6-button pad",
            Self::Multitap => "5-port multitap",
            Self::Mouse => "Force mouse",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[
            Self::Auto,
            Self::TwoButton,
            Self::SixButton,
            Self::Multitap,
            Self::Mouse,
        ]
    }
}

impl EnumLabel for PceMemoryBasePreference {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Enabled => "Enabled",
            Self::Disabled => "Disabled",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Auto, Self::Enabled, Self::Disabled]
    }
}

impl EnumLabel for PceArcadeCardPreference {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Enabled => "Enabled",
            Self::Disabled => "Disabled",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Auto, Self::Enabled, Self::Disabled]
    }
}

impl EnumLabel for PceMouseCursorMode {
    fn label(self) -> &'static str {
        match self {
            Self::Free => "Free cursor",
            Self::Captured => "Captured cursor",
        }
    }

    fn all_variants() -> &'static [Self] {
        #[cfg(not(target_arch = "wasm32"))]
        {
            &[Self::Free, Self::Captured]
        }
        #[cfg(target_arch = "wasm32")]
        {
            &[Self::Free]
        }
    }
}

impl EnumLabel for PceCdArchiveMemoryLimit {
    fn label(self) -> &'static str {
        match self {
            Self::MiB64 => "64 MiB",
            Self::MiB128 => "128 MiB",
            Self::MiB256 => "256 MiB",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::MiB64, Self::MiB128, Self::MiB256]
    }
}

impl EnumLabel for RewindMode {
    fn label(self) -> &'static str {
        match self {
            Self::RealTime => "Real-time",
            Self::Fast => "Fast",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::RealTime, Self::Fast]
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
    ui.checkbox(
        &mut settings.emulation.slow_motion_enabled,
        "Start in slow-motion mode",
    );
    ui.add(
        egui::Slider::new(&mut settings.emulation.slow_motion_divisor, 2..=16)
            .text("Slow-motion divisor"),
    );
    ui.add(
        egui::Slider::new(&mut settings.emulation.uncapped_frames_per_tick, 1..=240)
            .text("Uncapped frames/tick"),
    )
    .on_hover_text("Higher values trade input latency for throughput.");
    ui.checkbox(
        &mut settings.emulation.uncapped_speed,
        "Start in uncapped mode",
    );
    ui.checkbox(&mut settings.emulation.frame_skip, "Frame skip when behind")
        .on_hover_text("Drops host timing debt; emulated frames still run.");
    let recovery_save = ui.checkbox(
        &mut settings.emulation.save_recovery_state,
        "Save recovery state when stopping",
    );
    #[cfg(target_arch = "wasm32")]
    recovery_save.on_hover_text(
        "Keep this page open until saving finishes; abruptly closing it can interrupt browser storage.",
    );
    #[cfg(not(target_arch = "wasm32"))]
    let _ = recovery_save;
    ui.checkbox(
        &mut settings.emulation.resume_recovery_state,
        "Resume fresh recovery state automatically",
    );
    if settings.emulation.recovery_migration_notice_pending {
        ui.group(|ui| {
            ui.label("Automatic save and resume are now separate settings.");
            ui.horizontal(|ui| {
                if ui.button("Keep automatic resume").clicked() {
                    settings.emulation.resume_recovery_state = true;
                    settings.emulation.recovery_migration_notice_pending = false;
                }
                if ui.button("Keep resume off").clicked() {
                    settings.emulation.resume_recovery_state = false;
                    settings.emulation.recovery_migration_notice_pending = false;
                }
            });
        });
    }
    ui.checkbox(
        &mut settings.emulation.pause_on_unfocus,
        "Pause when window loses focus",
    );

    ui.separator();
    ui.heading("Archives");
    enum_combo_box(
        ui,
        "7z decoder memory limit",
        &mut settings.emulation.pce_cd_archive_memory_limit,
    );

    ui.separator();
    ui.heading("Rewind");
    ui.checkbox(&mut settings.rewind.enabled, "Enable rewind")
        .on_hover_text("Hold the rewind key.");
    ui.horizontal(|ui| {
        ui.label("History (seconds):");
        ui.add(
            egui::DragValue::new(&mut settings.rewind.seconds)
                .range(1..=120)
                .speed(1),
        );
    });
    enum_combo_box(ui, "Playback", &mut settings.rewind.mode);
    if settings.rewind.mode == RewindMode::Fast {
        ui.horizontal(|ui| {
            ui.label("Fast rewind step:");
            ui.add(
                egui::DragValue::new(&mut settings.rewind.speed)
                    .range(1..=10)
                    .speed(1),
            );
            ui.label(format!("({} snapshots)", settings.rewind.speed));
        });
    }

    ui.separator();
    super::draw_console_section_header(ui, "Game Boy", active_system, ActiveSystem::GameBoy);
    enum_combo_box(
        ui,
        "Hardware mode",
        &mut settings.emulation.hardware_mode_preference,
    );
    ui.checkbox(
        &mut settings.emulation.sgb_border_enabled,
        "Enable SGB border rendering",
    );
    ui.horizontal(|ui| {
        ui.label("TCP link address");
        ui.text_edit_singleline(&mut settings.emulation.tcp_link_addr);
    });
    ui.checkbox(
        &mut settings.emulation.nes_zapper_enabled,
        "Enable NES Zapper (Light Gun)",
    );

    ui.separator();
    super::draw_console_section_header(ui, "PC Engine", active_system, ActiveSystem::Pce);
    enum_combo_box(
        ui,
        "Console wiring",
        &mut settings.emulation.pce_console_wiring,
    );
    enum_combo_box(ui, "Controller", &mut settings.emulation.pce_controller);
    ui.label(
        egui::RichText::new("Force mouse can break unsupported games.")
            .weak()
            .small(),
    );
    enum_combo_box(ui, "Arcade Card", &mut settings.emulation.pce_arcade_card);
    ui.label(
        egui::RichText::new("Enabled requires System Card v3.")
            .weak()
            .small(),
    );
    enum_combo_box(
        ui,
        "Memory Base 128",
        &mut settings.emulation.pce_memory_base,
    );
    enum_combo_box(
        ui,
        "Mouse cursor",
        &mut settings.emulation.pce_mouse_cursor_mode,
    );
    ui.add(
        egui::Slider::new(&mut settings.emulation.pce_mouse_sensitivity, 0.25..=4.0)
            .text("Mouse sensitivity"),
    );

    ui.separator();
    ui.horizontal(|ui| {
        ui.heading("Sega 8-bit");
        if matches!(
            active_system,
            Some(ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000)
        ) {
            ui.label(egui::RichText::new("(active)").weak().italics().small());
        }
    });
    enum_combo_box(
        ui,
        "Video standard",
        &mut settings.emulation.sega8_video_standard,
    );
    enum_combo_box(
        ui,
        "Console region",
        &mut settings.emulation.sega8_console_region,
    );
}
