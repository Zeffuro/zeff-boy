use super::*;

impl GbBankedSession {
    pub(crate) fn new(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            prepared,
            CartridgeProfile::Banked,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_musyx(
        prepared: zeff_audio_discovery::gb_musyx::PreparedGbMusyx,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing: GbBankedTiming::Cgb,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::Musyx,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_tose(
        prepared: zeff_audio_discovery::gb_tose::PreparedGbTose,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing: GbBankedTiming::Dmg,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::Tose,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_quickthunder(
        prepared: zeff_audio_discovery::gb_quickthunder::PreparedGbQuickThunder,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let timing = match prepared.hardware {
            zeff_audio_discovery::gb_quickthunder::GbQuickThunderHardware::CgbDouble => {
                GbBankedTiming::CgbDouble
            }
        };
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::QuickThunder,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_ghx(
        prepared: zeff_audio_discovery::gb_ghx::PreparedGbGhx,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let timing = match prepared.hardware {
            zeff_audio_discovery::gb_ghx::GbGhxHardware::CgbDouble => GbBankedTiming::CgbDouble,
        };
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::Ghx,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_carillon(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            prepared,
            CartridgeProfile::Carillon,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_cosmigo(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            prepared,
            CartridgeProfile::Cosmigo,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_mplay(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(prepared, CartridgeProfile::Mplay, options, warnings, cancel)
    }

    pub(crate) fn new_imed(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(prepared, CartridgeProfile::Imed, options, warnings, cancel)
    }
    pub(crate) fn new_blackbox(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            prepared,
            CartridgeProfile::BlackBox,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_resident(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            prepared,
            CartridgeProfile::Resident,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_timer(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(prepared, CartridgeProfile::Timer, options, warnings, cancel)
    }

    pub(crate) fn new_cache(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(prepared, CartridgeProfile::Cache, options, warnings, cancel)
    }

    pub(crate) fn new_wave(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(prepared, CartridgeProfile::Wave, options, warnings, cancel)
    }

    pub(crate) fn new_channel(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            prepared,
            CartridgeProfile::Channel,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_page(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(prepared, CartridgeProfile::Page, options, warnings, cancel)
    }

    pub(crate) fn new_timed(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(prepared, CartridgeProfile::Timed, options, warnings, cancel)
    }

    pub(crate) fn new_sound_system(
        prepared: zeff_audio_discovery::gb_sound_system::PreparedGbSoundSystem,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let timing = match prepared.hardware {
            zeff_audio_discovery::gb_sound_system::GbSoundSystemHardware::CgbNormal => {
                GbBankedTiming::Cgb
            }
            zeff_audio_discovery::gb_sound_system::GbSoundSystemHardware::CgbDouble => {
                GbBankedTiming::CgbDouble
            }
        };
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::SoundSystem,
            options,
            warnings,
            cancel,
        )
    }
}
