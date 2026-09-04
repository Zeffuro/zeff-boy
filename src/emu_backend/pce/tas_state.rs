use anyhow::{Context, ensure};
use zeff_emu_common::save_state::StateReader;
use zeff_pce_core::hardware::PadButtons;

use super::{
    BACKEND_STATE_MAGIC, BACKEND_STATE_VERSION, MAX_CORE_STATE_BYTES, PceArcadeCardMode,
    PceBackend, PceControllerMode, PceMemoryBaseMode,
};
use crate::emu_backend::pce_display::project_presented_frame;
use crate::settings::{PceOverscanMode, PcePaletteMode};

pub(crate) type PceTasStateProjection =
    zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateProjection;

struct ParsedState {
    frame_count: u64,
    mouse_host_buttons: PadButtons,
    core_state: Vec<u8>,
}

impl PceBackend {
    pub(crate) fn tas_core_framebuffer(&self) -> &[u8] {
        self.machine.framebuffer()
    }

    pub(crate) fn tas_output_policy_is_exact(&self) -> bool {
        self.overscan_mode == PceOverscanMode::Full && self.palette_mode == PcePaletteMode::RawRgb
    }

    pub(crate) fn tas_presented_frame_is_current(&self) -> bool {
        let mut expected = vec![0; self.framebuffer.len()];
        project_presented_frame(
            self.machine.presented_frame(),
            self.machine.hardware_topology(),
            self.overscan_mode,
            self.palette_mode,
            &mut expected,
        );
        expected.as_slice() == self.framebuffer.as_ref()
    }

    pub(crate) fn inspect_current_native_tas_state(
        &self,
        bytes: &[u8],
    ) -> anyhow::Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateInspection>
    {
        let parsed = parse_state(bytes)?;
        ensure!(
            parsed.mouse_host_buttons.is_empty(),
            "PC Engine TAS state contains host mouse input"
        );
        let inspection =
            zeff_pce_core::hardware::save_state::tas::inspect_current_native_direct_hucard_tas_state_for_profile_and_controller(
                &self.machine,
                &parsed.core_state,
                self.machine.hucard_board(),
                self.machine.hardware_topology(),
                self.pce_controller_mode,
            )?;
        ensure!(
            parsed.frame_count == inspection.projection.frame_count,
            "PC Engine backend and core frame counters differ"
        );
        Ok(inspection)
    }

    pub(crate) fn inspect_current_native_cd_tas_state_for_profile(
        &self,
        bytes: &[u8],
        arcade_card: bool,
        memory_base: bool,
    ) -> anyhow::Result<
        zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection,
    > {
        self.inspect_current_native_cd_tas_state_for_profile_and_controller(
            bytes,
            arcade_card,
            memory_base,
            PceControllerMode::TwoButton,
        )
    }

    pub(crate) fn inspect_current_native_cd_tas_state_for_profile_and_controller(
        &self,
        bytes: &[u8],
        arcade_card: bool,
        memory_base: bool,
        controller_mode: PceControllerMode,
    ) -> anyhow::Result<
        zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection,
    > {
        let parsed = parse_state(bytes)?;
        ensure!(
            parsed.mouse_host_buttons.is_empty(),
            "PC Engine TAS state contains host mouse input"
        );
        let inspection =
            zeff_pce_core::hardware::save_state::tas::inspect_current_native_pce_cd_tas_state_for_profile_and_controller(
                &self.machine,
                &parsed.core_state,
                arcade_card,
                memory_base,
                controller_mode,
            )?;
        ensure!(
            parsed.frame_count == inspection.projection.frame_count,
            "PC Engine backend and core frame counters differ"
        );
        Ok(inspection)
    }

    pub(crate) fn validate_and_load_current_native_tas_state(
        &mut self,
        bytes: &[u8],
    ) -> anyhow::Result<PceTasStateProjection> {
        let parsed = parse_state(bytes)?;
        let inspection = self.inspect_current_native_tas_state(bytes)?;
        let board = self.machine.hucard_board();
        let topology = self.machine.hardware_topology();
        let controller_mode = self.pce_controller_mode;
        let projection = zeff_pce_core::hardware::save_state::tas::validate_and_load_current_native_direct_hucard_tas_state_for_profile_and_controller(
            &mut self.machine,
            &parsed.core_state,
            board,
            topology,
            controller_mode,
        )
        .context("failed to restore strict PC Engine TAS state")?;
        ensure!(
            projection == inspection.projection,
            "PC Engine TAS state changed during restoration"
        );
        self.frame_count = parsed.frame_count;
        self.mouse_host_buttons = parsed.mouse_host_buttons;
        self.pce_controller_mode = controller_mode;
        self.pce_memory_base_mode = PceMemoryBaseMode::Disabled;
        self.pending_runtime_fault = None;
        self.memory_base_force_flush = false;
        self.project_presented_frame();
        ensure!(
            projection.framebuffer.as_ref() == self.machine.framebuffer()
                && self.tas_presented_frame_is_current(),
            "PC Engine TAS state output was not restored exactly"
        );
        Ok(projection)
    }

    pub(crate) fn validate_and_load_current_native_cd_tas_state_for_profile(
        &mut self,
        bytes: &[u8],
        arcade_card: bool,
        memory_base: bool,
    ) -> anyhow::Result<PceTasStateProjection> {
        self.validate_and_load_current_native_cd_tas_state_for_profile_and_controller(
            bytes,
            arcade_card,
            memory_base,
            PceControllerMode::TwoButton,
        )
    }

    pub(crate) fn validate_and_load_current_native_cd_tas_state_for_profile_and_controller(
        &mut self,
        bytes: &[u8],
        arcade_card: bool,
        memory_base: bool,
        controller_mode: PceControllerMode,
    ) -> anyhow::Result<PceTasStateProjection> {
        let parsed = parse_state(bytes)?;
        let inspection = self.inspect_current_native_cd_tas_state_for_profile_and_controller(
            bytes,
            arcade_card,
            memory_base,
            controller_mode,
        )?;
        let projection =
            zeff_pce_core::hardware::save_state::tas::validate_and_load_current_native_pce_cd_tas_state_for_profile_and_controller(
                &mut self.machine,
                &parsed.core_state,
                arcade_card,
                memory_base,
                controller_mode,
            )
            .context("failed to restore strict PC Engine CD TAS state")?;
        ensure!(
            projection == inspection.projection,
            "PC Engine CD TAS state changed during restoration"
        );
        self.frame_count = parsed.frame_count;
        self.mouse_host_buttons = parsed.mouse_host_buttons;
        self.pce_controller_mode = controller_mode;
        self.pce_memory_base_mode = if memory_base {
            PceMemoryBaseMode::Enabled
        } else {
            PceMemoryBaseMode::Disabled
        };
        self.pce_arcade_card_mode = if arcade_card {
            PceArcadeCardMode::Enabled
        } else {
            PceArcadeCardMode::Disabled
        };
        self.pending_runtime_fault = None;
        self.memory_base_force_flush = false;
        self.project_presented_frame();
        ensure!(
            projection.framebuffer.as_ref() == self.machine.framebuffer()
                && self.tas_presented_frame_is_current(),
            "PC Engine CD TAS state output was not restored exactly"
        );
        Ok(projection)
    }

    pub(crate) fn tas_frame_counters_match(&self) -> bool {
        self.frame_count == self.machine.frame_count()
    }
}

fn parse_state(bytes: &[u8]) -> anyhow::Result<ParsedState> {
    let mut reader = StateReader::new(bytes);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    ensure!(
        &magic == BACKEND_STATE_MAGIC,
        "not a valid PC Engine backend save-state"
    );
    let version = reader.read_u32()?;
    ensure!(
        version == BACKEND_STATE_VERSION,
        "unsupported PC Engine backend save-state version {version}"
    );
    let frame_count = reader.read_u64()?;
    let mouse_host_buttons = PadButtons::from_bits_retain(reader.read_u8()?);
    let core_state = reader.read_vec(MAX_CORE_STATE_BYTES)?;
    ensure!(
        reader.is_exhausted(),
        "PC Engine backend save-state has unexpected trailing data"
    );
    Ok(ParsedState {
        frame_count,
        mouse_host_buttons,
        core_state,
    })
}

pub(crate) fn inspect_pce_tas_state_identity(
    bytes: &[u8],
) -> anyhow::Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateIdentity> {
    let parsed = parse_state(bytes)?;
    ensure!(
        parsed.mouse_host_buttons.is_empty(),
        "PC Engine TAS state contains host mouse input"
    );
    zeff_pce_core::hardware::save_state::tas::inspect_current_native_supported_hucard_tas_state_identity(
        &parsed.core_state,
    )
}

pub(crate) fn inspect_pce_cd_tas_state_identity_for_arcade_card(
    bytes: &[u8],
    arcade_card: bool,
) -> anyhow::Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateIdentity> {
    let parsed = parse_state(bytes)?;
    ensure!(
        parsed.mouse_host_buttons.is_empty(),
        "PC Engine TAS state contains host mouse input"
    );
    zeff_pce_core::hardware::save_state::tas::inspect_current_native_pce_cd_tas_state_identity_for_arcade_card(
        &parsed.core_state,
        arcade_card,
    )
}
