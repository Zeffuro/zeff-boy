use crate::debug::ui_helpers::{EnumLabel, enum_combo_box};
use crate::emu_backend::ActiveSystem;
use crate::settings::{
    PceArcadeCardPreference, PceCdArchiveMemoryLimit, PceConsoleWiringPreference,
    PceControllerPreference, PceMemoryBasePreference, PceMouseCursorMode,
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
    ui.heading("Archives");
    enum_combo_box(
        ui,
        "7z decoder memory limit",
        &mut settings.emulation.pce_cd_archive_memory_limit,
    );
    ui.label(
        egui::RichText::new(
            "Higher limits admit archives made with larger LZMA dictionaries. The decoder still \
             retains only selected ROMs or the track files referenced by a CUE sheet.",
        )
        .weak()
        .small(),
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
    ui.horizontal(|ui| {
        ui.label("TCP link address");
        ui.text_edit_singleline(&mut settings.emulation.tcp_link_addr)
            .on_hover_text(
                "Host binds this address; Join connects to it. Change the port here when needed.",
            );
    });
    ui.checkbox(
        &mut settings.emulation.nes_zapper_enabled,
        "Enable NES Zapper (Light Gun)",
    )
    .on_hover_text(
        "When enabled, replaces Player 2 controller with a Zapper light gun. \
         Click the game screen to fire. Only works with NES games that support the Zapper.",
    );

    ui.separator();
    super::draw_console_section_header(ui, "PC Engine", active_system, ActiveSystem::Pce);
    enum_combo_box(
        ui,
        "Console wiring",
        &mut settings.emulation.pce_console_wiring,
    );
    ui.label(
        egui::RichText::new(
            "Auto uses exact cartridge metadata when known and otherwise defaults to PC Engine. \
             Force TurboGrafx-16 for unrecognized North American HuCards.",
        )
        .weak()
        .small(),
    );
    enum_combo_box(ui, "Controller", &mut settings.emulation.pce_controller);
    ui.label(
        egui::RichText::new(
            "Auto enables special controllers only for recognized disc images. Six-button is manual and maps A/B/L/R/X/Y to I/II/III/IV/V/VI. Force mouse may break games that do not support it.",
        )
        .weak()
        .small(),
    );
    enum_combo_box(ui, "Arcade Card", &mut settings.emulation.pce_arcade_card);
    ui.label(
        egui::RichText::new(
            "Applied when CD content is loaded. Auto enables the 2 MiB expansion only for cataloged normalized disc identities; Enabled requires System Card v3.",
        )
        .weak()
        .small(),
    );
    enum_combo_box(
        ui,
        "Memory Base 128",
        &mut settings.emulation.pce_memory_base,
    );
    ui.label(
        egui::RichText::new(
            "Auto connects the 128 KiB save accessory only for cataloged normalized disc identities. Enabled forces it for compatible software; Disabled leaves it disconnected.",
        )
        .weak()
        .small(),
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
    ui.label(
        egui::RichText::new(
            "Auto uses common filename region tags such as (Europe), [E], PAL, USA, Japan, or NTSC. \
             Force PAL for untagged PAL-only Master System ROMs.",
        )
        .weak()
        .small(),
    );
    enum_combo_box(
        ui,
        "Console region",
        &mut settings.emulation.sega8_console_region,
    );
    ui.label(
        egui::RichText::new(
            "Controls export, real Japanese SMS/GG, or Japanese Power Base Converter behavior used \
             by region-detection code. Auto uses header first, then filename hints, and defaults to export.",
        )
        .weak()
        .small(),
    );
}
