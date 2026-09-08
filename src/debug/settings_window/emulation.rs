use crate::debug::ui_helpers::EnumLabel;
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
    use super::{
        layout::{checkbox, enum_combo, helper, row},
        search::{self, SettingId as Id},
    };

    helper(
        ui,
        egui::RichText::new("Timing, rewind, and console-specific behavior.")
            .small()
            .weak(),
    );
    ui.add_space(14.0);
    ui.heading("Speed");
    row(ui, Id::EmulationFastForwardMultiplier, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.emulation.fast_forward_multiplier, 1..=16),
        )
    });
    checkbox(
        ui,
        Id::EmulationStartInSlowMotionMode,
        None,
        &mut settings.emulation.slow_motion_enabled,
    );
    row(ui, Id::EmulationSlowMotionDivisor, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.emulation.slow_motion_divisor, 2..=16),
        )
    });
    row(ui, Id::EmulationUncappedFramesTick, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.emulation.uncapped_frames_per_tick, 1..=240),
        )
        .on_hover_text("Higher values trade input latency for throughput.")
    });
    checkbox(
        ui,
        Id::EmulationStartInUncappedMode,
        None,
        &mut settings.emulation.uncapped_speed,
    );
    checkbox(
        ui,
        Id::EmulationFrameSkipWhenBehind,
        None,
        &mut settings.emulation.frame_skip,
    )
    .on_hover_text("Drops host timing debt; emulated frames still run.");

    ui.add_space(22.0);
    ui.heading("Archives");
    enum_combo(
        ui,
        Id::Emulation7zDecoderMemoryLimit,
        "emulation_archive_memory",
        None,
        &mut settings.emulation.pce_cd_archive_memory_limit,
    );

    ui.add_space(22.0);
    ui.heading("Rewind");
    checkbox(
        ui,
        Id::EmulationEnableRewind,
        None,
        &mut settings.rewind.enabled,
    )
    .on_hover_text("Hold the rewind key.");
    row(
        ui,
        Id::EmulationRewindHistoryLength,
        Some("History (seconds)"),
        |ui| {
            ui.add(
                egui::DragValue::new(&mut settings.rewind.seconds)
                    .range(1..=120)
                    .speed(1),
            )
        },
    );
    enum_combo(
        ui,
        Id::EmulationRewindPlaybackMode,
        "emulation_rewind_mode",
        None,
        &mut settings.rewind.mode,
    );
    search::conditional(
        ui,
        Id::EmulationFastRewindStep,
        Id::EmulationRewindPlaybackMode,
        settings.rewind.mode == RewindMode::Fast,
        "Choose Fast rewind playback to edit its step size.",
    );
    if settings.rewind.mode == RewindMode::Fast {
        row(ui, Id::EmulationFastRewindStep, None, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::DragValue::new(&mut settings.rewind.speed)
                        .range(1..=10)
                        .speed(1),
                );
                ui.label(format!("({} snapshots)", settings.rewind.speed));
                response
            })
            .inner
        });
    }

    ui.add_space(22.0);
    super::draw_console_section_header(ui, "Game Boy", active_system, ActiveSystem::GameBoy);
    enum_combo(
        ui,
        Id::EmulationGameBoyHardwareMode,
        "emulation_gb_hardware",
        None,
        &mut settings.emulation.hardware_mode_preference,
    );
    checkbox(
        ui,
        Id::EmulationEnableSgbBorderRendering,
        None,
        &mut settings.emulation.sgb_border_enabled,
    );
    row(ui, Id::EmulationTcpLinkAddress, None, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut settings.emulation.tcp_link_addr).desired_width(240.0),
        )
    });

    ui.add_space(22.0);
    super::draw_console_section_header(ui, "NES", active_system, ActiveSystem::Nes);
    checkbox(
        ui,
        Id::EmulationEnableNesZapper,
        None,
        &mut settings.emulation.nes_zapper_enabled,
    );

    ui.add_space(22.0);
    super::draw_console_section_header(ui, "PC Engine", active_system, ActiveSystem::Pce);
    enum_combo(
        ui,
        Id::EmulationPcEngineConsoleWiring,
        "emulation_pce_wiring",
        None,
        &mut settings.emulation.pce_console_wiring,
    );
    enum_combo(
        ui,
        Id::EmulationPcEngineController,
        "emulation_pce_controller",
        None,
        &mut settings.emulation.pce_controller,
    );
    helper(
        ui,
        egui::RichText::new("Force mouse can break unsupported games.")
            .small()
            .weak(),
    );
    enum_combo(
        ui,
        Id::EmulationPcEngineArcadeCard,
        "emulation_pce_arcade_card",
        None,
        &mut settings.emulation.pce_arcade_card,
    );
    helper(
        ui,
        egui::RichText::new("Enabled requires System Card v3.")
            .small()
            .weak(),
    );
    enum_combo(
        ui,
        Id::EmulationMemoryBase128,
        "emulation_pce_memory_base",
        None,
        &mut settings.emulation.pce_memory_base,
    );
    enum_combo(
        ui,
        Id::EmulationPcEngineMouseCursor,
        "emulation_pce_mouse_cursor",
        None,
        &mut settings.emulation.pce_mouse_cursor_mode,
    );
    row(ui, Id::EmulationPcEngineMouseSensitivity, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.emulation.pce_mouse_sensitivity, 0.25..=4.0),
        )
    });

    ui.add_space(22.0);
    let sega_active = matches!(
        active_system,
        Some(ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000)
    );
    ui.horizontal_wrapped(|ui| {
        ui.heading("Sega 8-bit");
        if sega_active {
            ui.label(egui::RichText::new("(active)").weak().italics().small());
        }
    });
    enum_combo(
        ui,
        Id::EmulationSegaVideoStandard,
        "emulation_sega_video",
        None,
        &mut settings.emulation.sega8_video_standard,
    );
    enum_combo(
        ui,
        Id::EmulationSegaConsoleRegion,
        "emulation_sega_region",
        None,
        &mut settings.emulation.sega8_console_region,
    );
}
