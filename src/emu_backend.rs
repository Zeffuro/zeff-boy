use std::path::{Path, PathBuf};

use anyhow::Context;
use zeff_emu_common::memory::MemoryRegionDescriptor;
use zeff_emu_common::replay::ReplayJoypadFrame;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::system::CoreFamily;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};

pub(crate) use self::capabilities::{CheatCapabilities, CoreCapabilities, InputCapabilities};
pub(crate) use self::gb::GbBackend;
pub(crate) use self::gba::GbaBackend;
pub(crate) use self::loader::{BackendLoadConfig, load_backend_from_rom_source};
pub(crate) use self::nes::NesBackend;
pub(crate) use self::pce::PceBackend;
pub(crate) use self::runtime::BackendRuntimeConfig;
pub(crate) use self::sega8::Sega8Backend;
pub(crate) use self::system::{
    ActiveSystem, ROM_AND_ARCHIVE_EXTENSIONS, ROM_EXTENSIONS, archive_extensions, system_specs,
};
pub(crate) use self::ws::WsBackend;

use crate::emu_core_trait::EmulatorCore;

#[cfg(not(target_arch = "wasm32"))]
const WONDER_SWAN_REMOTE_LINK_WAIT_SPINS: usize = 2048;
#[cfg(not(target_arch = "wasm32"))]
const WONDER_SWAN_REMOTE_LINK_POLL_INTERVAL_CYCLES: u64 = 800;

pub(crate) mod capabilities;
pub(crate) mod cheats;
pub(crate) mod firmware;
pub(crate) mod gb;
pub(crate) mod gba;
pub(crate) mod loader;
pub(crate) mod nes;
pub(crate) mod paths;
pub(crate) mod pce;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd_archive;
mod pce_display;
mod pce_palette;
pub(crate) mod runtime;
pub(crate) mod sega8;
pub(crate) mod system;
pub(crate) mod ws;

pub(crate) enum EmuBackend {
    Gb(Box<GbBackend>),
    Gba(Box<GbaBackend>),
    Nes(Box<NesBackend>),
    Pce(Box<PceBackend>),
    Sega8(Box<Sega8Backend>),
    Ws(Box<WsBackend>),
}

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            EmuBackend::Gb(b) => b.$method($($arg),*),
            EmuBackend::Gba(b) => b.$method($($arg),*),
            EmuBackend::Nes(b) => b.$method($($arg),*),
            EmuBackend::Pce(b) => b.$method($($arg),*),
            EmuBackend::Sega8(b) => b.$method($($arg),*),
            EmuBackend::Ws(b) => b.$method($($arg),*),
        }
    };
}

impl EmuBackend {
    pub(crate) fn from_gb(emu: zeff_gb_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Gb(Box::new(GbBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_gb_with_source(
        emu: zeff_gb_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Gb(Box::new(GbBackend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn from_gba(emu: zeff_gba_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Gba(Box::new(GbaBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_gba_with_source(
        emu: zeff_gba_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Gba(Box::new(GbaBackend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn from_nes(emu: zeff_nes_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Nes(Box::new(NesBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_pce(backend: PceBackend) -> Self {
        Self::Pce(Box::new(backend))
    }

    pub(crate) fn from_nes_with_source(
        emu: zeff_nes_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Nes(Box::new(NesBackend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn from_sega8(emu: zeff_sega8_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Sega8(Box::new(Sega8Backend::new(emu, rom_path)))
    }

    pub(crate) fn from_sega8_with_source(
        emu: zeff_sega8_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Sega8(Box::new(Sega8Backend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn from_ws(emu: zeff_ws_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Ws(Box::new(WsBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_ws_with_source(
        emu: zeff_ws_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Ws(Box::new(WsBackend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn system(&self) -> ActiveSystem {
        match self {
            Self::Gb(..) => ActiveSystem::GameBoy,
            Self::Gba(..) => ActiveSystem::GameBoyAdvance,
            Self::Nes(..) => ActiveSystem::Nes,
            Self::Pce(..) => ActiveSystem::Pce,
            Self::Sega8(b) => b.system(),
            Self::Ws(..) => ActiveSystem::WonderSwan,
        }
    }

    pub(crate) fn core_family(&self) -> CoreFamily {
        self.system().core_family()
    }

    fn state_extension(&self) -> &'static str {
        self.system().state_extension()
    }

    pub(crate) fn gb(&self) -> Option<&GbBackend> {
        match self {
            Self::Gb(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn gba(&self) -> Option<&GbaBackend> {
        match self {
            Self::Gba(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn nes(&self) -> Option<&NesBackend> {
        match self {
            Self::Nes(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn sega8(&self) -> Option<&Sega8Backend> {
        match self {
            Self::Sega8(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn ws(&self) -> Option<&WsBackend> {
        match self {
            Self::Ws(b) => Some(b),
            _ => None,
        }
    }
}

impl EmuBackend {
    #[inline]
    pub(crate) fn framebuffer(&self) -> &[u8] {
        dispatch!(self, framebuffer())
    }

    #[inline]
    pub(crate) fn is_suspended(&self) -> bool {
        dispatch!(self, is_suspended())
    }

    pub(crate) fn debug_suspend(&mut self) {
        if !self.supports_execution_controls() {
            return;
        }
        dispatch!(self, debug_suspend())
    }

    pub(crate) fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        if !self.supports_state_capture() {
            anyhow::bail!("state capture is not supported by this core");
        }
        dispatch!(self, encode_state_bytes())
    }

    pub(crate) fn encode_replay_hash_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let mut bytes = self.encode_state_bytes()?;
        canonicalize_state_bytes_for_replay_hash(self.system(), &mut bytes);
        Ok(bytes)
    }

    pub(crate) fn rom_path(&self) -> &Path {
        dispatch!(self, rom_path())
    }

    pub(crate) fn source_path(&self) -> &Path {
        dispatch!(self, source_path())
    }

    pub(crate) fn replay_metadata(&self) -> zeff_emu_common::replay::ReplayMetadata {
        zeff_emu_common::replay::ReplayMetadata {
            system: Some(self.system().code().to_owned()),
            core_family: Some(format!("{:?}", self.core_family())),
            rom_sha256: Some(self.rom_hash()),
            firmware: dispatch!(self, firmware_manifests()).to_vec(),
            events: Vec::new(),
            cheat_sha256: None,
            final_state_sha256: None,
            game_boy_link_start_state: None,
            game_boy_link_coordinator_start_state: None,
            game_boy_link_start_tick: None,
            wonder_swan_link_start_tick: None,
            checkpoints: Vec::new(),
        }
    }

    pub(crate) fn set_firmware_manifests(
        &mut self,
        firmware_manifests: Vec<zeff_emu_common::replay::ReplayFirmwareManifest>,
    ) {
        dispatch!(self, set_firmware_manifests(firmware_manifests))
    }

    pub(crate) fn rom_hash(&self) -> [u8; 32] {
        dispatch!(self, rom_hash())
    }

    pub(crate) fn save_ram_kind(&self) -> SaveRamKind {
        dispatch!(self, save_ram_kind())
    }

    pub(crate) fn has_battery(&self) -> bool {
        self.save_ram_kind().is_battery_backed()
    }

    pub(crate) fn system_ram_len(&self) -> usize {
        dispatch!(self, system_ram_len())
    }

    pub(crate) fn video_ram_len(&self) -> usize {
        dispatch!(self, video_ram_len())
    }

    pub(crate) fn supports_debugger(&self) -> bool {
        dispatch!(self, supports_debugger())
    }

    pub(crate) fn supports_execution_controls(&self) -> bool {
        dispatch!(self, supports_execution_controls())
    }

    pub(crate) fn supports_symbol_loading(&self) -> bool {
        dispatch!(self, supports_symbol_loading())
    }

    pub(crate) fn supports_opcode_history(&self) -> bool {
        dispatch!(self, supports_opcode_history())
    }

    pub(crate) fn supports_save_states(&self) -> bool {
        dispatch!(self, supports_save_states())
    }

    pub(crate) fn supports_state_capture(&self) -> bool {
        dispatch!(self, supports_state_capture())
    }

    pub(crate) fn supports_rewind(&self) -> bool {
        dispatch!(self, supports_rewind())
    }

    pub(crate) fn supports_replay(&self) -> bool {
        dispatch!(self, supports_replay())
    }

    pub(crate) fn supports_audio(&self) -> bool {
        dispatch!(self, supports_audio())
    }

    pub(crate) fn supports_cheats(&self) -> bool {
        dispatch!(self, supports_cheats())
    }

    pub(crate) fn supports_guest_calls(&self) -> bool {
        dispatch!(self, supports_guest_calls())
    }

    pub(crate) fn take_runtime_fault(&mut self) -> Option<String> {
        dispatch!(self, take_runtime_fault())
    }

    pub(crate) fn capabilities(&self) -> CoreCapabilities {
        CoreCapabilities {
            core_family: self.core_family(),
            save_ram_kind: self.save_ram_kind(),
            has_battery: self.has_battery(),
            system_ram_len: self.system_ram_len(),
            video_ram_len: self.video_ram_len(),
            memory_regions: self.memory_regions(),
            input_features: capabilities::InputCapabilities::for_system(self.system()),
            cheat_features: CheatCapabilities::for_system(self.system()),
            supports_save_states: self.supports_save_states(),
            supports_state_capture: self.supports_state_capture(),
            supports_rewind: self.supports_rewind(),
            supports_replay: self.supports_replay(),
            supports_audio: self.supports_audio(),
            supports_cheats: self.supports_cheats(),
            supports_guest_calls: self.supports_guest_calls(),
            supports_debugger: self.supports_debugger(),
            supports_execution_controls: self.supports_execution_controls(),
            supports_opcode_history: self.supports_opcode_history(),
        }
    }

    pub(crate) fn memory_regions(&self) -> Vec<MemoryRegionDescriptor> {
        dispatch!(self, memory_regions())
    }

    #[allow(dead_code)]
    pub(crate) fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        dispatch!(self, copy_memory_region(id_or_alias, out))
    }

    #[inline]
    pub(crate) fn rumble_active(&self) -> bool {
        dispatch!(self, rumble_active())
    }

    #[inline]
    pub(crate) fn is_mbc7(&self) -> bool {
        dispatch!(self, is_mbc7())
    }

    #[inline]
    pub(crate) fn is_pocket_camera(&self) -> bool {
        dispatch!(self, is_pocket_camera())
    }

    pub(crate) fn audio_semantic_frame(&self) -> Option<crate::audio_tooling::AudioSemanticFrame> {
        if !self.supports_audio() {
            return None;
        }
        let frame = dispatch!(self, audio_semantic_frame());
        if let (Some(topology), Some(frame)) = (self.audio_topology(), frame.as_ref()) {
            crate::audio_tooling::debug_assert_frame_matches_topology(topology, frame);
        }
        frame
    }

    pub(crate) fn audio_topology(&self) -> Option<crate::audio_tooling::AudioTopology> {
        self.supports_audio()
            .then(|| dispatch!(self, audio_topology()))
            .flatten()
    }

    #[inline]
    pub(crate) fn step_frame(&mut self) {
        FrameLifecycle::step_frame(self)
    }

    #[inline]
    pub(crate) fn frame_count(&self) -> u64 {
        FrameLifecycle::frame_count(self)
    }

    pub(crate) fn game_boy_cpu_cycles(&self) -> Option<u64> {
        match self {
            Self::Gb(gb) => Some(gb.emu.cpu_cycles()),
            _ => None,
        }
    }

    pub(crate) fn wonder_swan_cpu_cycles(&self) -> Option<u64> {
        match self {
            Self::Ws(ws) => Some(ws.emu.cpu_cycles()),
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn begin_game_boy_frame_slice(
        &self,
    ) -> Result<zeff_gb_core::emulator::FrameSliceCursor, crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };
        Ok(gb.emu.begin_frame_slice())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_slice_until(
        &mut self,
        cursor: &mut zeff_gb_core::emulator::FrameSliceCursor,
        target_tick: Option<u64>,
        stop_on_link_action: bool,
    ) -> Result<zeff_gb_core::emulator::FrameSliceProgress, crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };
        if !cursor.is_complete()
            && (target_tick.is_some_and(|tick| gb.emu.cpu_cycles() >= tick)
                || (stop_on_link_action
                    && gb
                        .emu
                        .game_boy_link_replay_state()
                        .queued_master_action
                        .is_some()))
        {
            return Ok(zeff_gb_core::emulator::FrameSliceProgress {
                outcome: zeff_gb_core::emulator::FrameSliceOutcome::Boundary,
                boundary_reached: true,
            });
        }
        Ok(gb.emu.step_frame_slice_until(cursor, |emulator| {
            target_tick.is_some_and(|tick| emulator.cpu_cycles() >= tick)
                || (stop_on_link_action
                    && emulator
                        .game_boy_link_replay_state()
                        .queued_master_action
                        .is_some())
        }))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_with_remote_link<T: crate::link::LinkTransport>(
        &mut self,
        link: &mut crate::link::gb::GameBoyRemoteLink<T>,
    ) -> Result<(), crate::link::LinkSessionError> {
        let mut cursor = self.begin_game_boy_frame_slice()?;
        let _ = self.step_game_boy_frame_slice_with_remote_link(link, &mut cursor, None)?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_slice_with_remote_link<T: crate::link::LinkTransport>(
        &mut self,
        link: &mut crate::link::gb::GameBoyRemoteLink<T>,
        cursor: &mut zeff_gb_core::emulator::FrameSliceCursor,
        activation_tick: Option<u64>,
    ) -> Result<zeff_gb_core::emulator::FrameSliceOutcome, crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        let link_is_active = activation_tick.is_none_or(|tick| gb.emu.cpu_cycles() >= tick);
        if link_is_active {
            link.poll_emulator(&mut gb.emu)?;
            if gb.emu.game_boy_link_pending_master_response() {
                link.trace_wait_pending_master(gb.emu.cpu_cycles(), "frame_start");
                return Ok(zeff_gb_core::emulator::FrameSliceOutcome::Boundary);
            }
        } else {
            gb.emu
                .restore_game_boy_link_peer_present_without_action(false);
        }

        let mut link_error = None;
        let outcome = gb.emu.step_frame_slice_or(cursor, |emulator| {
            if activation_tick.is_some_and(|tick| emulator.cpu_cycles() < tick) {
                return false;
            }
            if let Err(err) = link.poll_emulator(emulator) {
                link_error = Some(err);
                return true;
            }
            if emulator.game_boy_link_pending_master_response() {
                link.trace_wait_pending_master(emulator.cpu_cycles(), "mid_frame_activation");
                return true;
            }
            false
        });

        if let Some(err) = link_error {
            return Err(err);
        }
        link.poll_emulator(&mut gb.emu)?;
        Ok(outcome)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_with_replay_link(
        &mut self,
        link: &mut crate::link::gb::GameBoyReplayLink,
    ) -> Result<(), crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        link.poll_emulator(&mut gb.emu)?;
        if gb.emu.game_boy_link_waiting_at_completion_boundary() {
            link.trace_wait_boundary(gb.emu.cpu_cycles(), "frame_start");
            return Ok(());
        }

        let mut link_error = None;
        gb.emu.step_until_frame_or(|emulator| {
            if let Err(err) = link.poll_emulator(emulator) {
                link_error = Some(err);
                return true;
            }
            if emulator.game_boy_link_waiting_at_completion_boundary() {
                link.trace_wait_boundary(emulator.cpu_cycles(), "mid_frame");
                return true;
            }
            false
        });

        if let Some(err) = link_error {
            return Err(err);
        }
        link.poll_emulator(&mut gb.emu)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_wonder_swan_frame_with_remote_link(
        &mut self,
        link: &mut crate::link::ws::WonderSwanRemoteLink<crate::link::transport::TcpLinkTransport>,
    ) -> Result<(), crate::link::LinkSessionError> {
        let Self::Ws(ws) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        link.poll_emulator(&mut ws.emu)?;
        if ws.emu.is_cpu_suspended() {
            return Ok(());
        }
        if !wait_for_wonder_swan_remote_link_window(&mut ws.emu, link)? {
            return Ok(());
        }

        ws.emu.clear_frame_ready();
        let mut next_link_poll_cycle = ws
            .emu
            .cpu_cycles()
            .saturating_add(WONDER_SWAN_REMOTE_LINK_POLL_INTERVAL_CYCLES);
        let guard = ws
            .emu
            .cpu_cycles()
            .wrapping_add(u64::from(zeff_ws_core::hardware::constants::CYCLES_PER_FRAME) * 2);
        while !ws.emu.frame_ready() && ws.emu.cpu_cycles() < guard {
            if ws.emu.cpu_cycles() >= next_link_poll_cycle {
                link.poll_emulator(&mut ws.emu)?;
                if !wait_for_wonder_swan_remote_link_window(&mut ws.emu, link)? {
                    return Ok(());
                }
                next_link_poll_cycle = ws
                    .emu
                    .cpu_cycles()
                    .saturating_add(WONDER_SWAN_REMOTE_LINK_POLL_INTERVAL_CYCLES);
            }
            let fetched = if link.trace_enabled() {
                let (fetched, bus_events) = ws.emu.step_instruction_with_io_trace();
                link.trace_serial_io_events(
                    &ws.emu,
                    fetched.or_else(|| ws.emu.last_fetch()),
                    &bus_events,
                );
                fetched
            } else {
                ws.emu.step_instruction()
            };
            if fetched.is_none() && ws.emu.is_cpu_suspended() {
                break;
            }
        }
        ws.emu.finish_frame();
        link.poll_emulator(&mut ws.emu)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_wonder_swan_frame_with_replay_link(
        &mut self,
        link: &mut crate::link::ws_replay::WonderSwanReplayLink,
    ) -> Result<(), crate::link::LinkSessionError> {
        let Self::Ws(ws) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        link.poll_emulator(&mut ws.emu)?;
        if ws.emu.is_cpu_suspended() {
            return Ok(());
        }

        ws.emu.clear_frame_ready();
        let guard = ws
            .emu
            .cpu_cycles()
            .wrapping_add(u64::from(zeff_ws_core::hardware::constants::CYCLES_PER_FRAME) * 2);
        while !ws.emu.frame_ready() && ws.emu.cpu_cycles() < guard {
            link.poll_emulator(&mut ws.emu)?;
            let fetched = ws.emu.step_instruction();
            if fetched.is_none() && ws.emu.is_cpu_suspended() {
                break;
            }
        }
        ws.emu.finish_frame();
        link.poll_emulator(&mut ws.emu)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_frame_with_remote_link(
        &mut self,
        link: &mut crate::link::RemoteLink<crate::link::transport::TcpLinkTransport>,
    ) -> Result<(), crate::link::LinkSessionError> {
        match link {
            crate::link::RemoteLink::GameBoy(link) => {
                self.step_game_boy_frame_with_remote_link(link)
            }
            crate::link::RemoteLink::WonderSwan(link) => {
                self.step_wonder_swan_frame_with_remote_link(link)
            }
        }
    }

    #[inline]
    pub(crate) fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        if self.supports_audio() {
            dispatch!(self, drain_audio_samples_into(buf))
        }
    }

    pub(crate) fn set_sample_rate(&mut self, rate: u32) {
        if self.supports_audio() {
            dispatch!(self, set_sample_rate(rate))
        }
    }

    pub(crate) fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        if self.supports_audio() {
            dispatch!(self, set_apu_sample_generation_enabled(enabled))
        }
    }

    pub(crate) fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        if self.supports_audio() {
            dispatch!(self, set_apu_channel_mutes(mutes))
        }
    }

    #[inline]
    pub(crate) fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input(buttons_pressed, dpad_pressed))
    }

    pub(crate) fn set_pce_mouse_state(
        &mut self,
        mode: zeff_pce_core::hardware::PceControllerMode,
        delta_x: i16,
        delta_y: i16,
        buttons_pressed: u8,
    ) {
        dispatch!(
            self,
            set_pce_mouse_state(mode, delta_x, delta_y, buttons_pressed)
        )
    }

    pub(crate) fn set_pce_memory_base_mode(
        &mut self,
        mode: zeff_pce_core::hardware::PceMemoryBaseMode,
    ) {
        dispatch!(self, set_pce_memory_base_mode(mode))
    }

    pub(crate) fn apply_replay_input(&mut self, frame: &ReplayJoypadFrame) {
        self.set_input(frame.buttons, frame.dpad);
        self.set_input_p2(frame.buttons_p2, frame.dpad_p2);
        self.set_zapper_state(
            frame.zapper.enabled,
            frame.zapper.trigger,
            frame.zapper.hit,
            frame.zapper.screen_pos,
        );
        self.set_replay_host_tilt(frame.host_tilt);
        if let Some(camera_frame) = frame.camera_frame.as_deref() {
            self.set_replay_camera_frame(camera_frame);
        }
    }

    #[inline]
    pub(crate) fn set_zapper_state(
        &mut self,
        enabled: bool,
        trigger: bool,
        hit: bool,
        screen_pos: Option<(u16, u16)>,
    ) {
        dispatch!(self, set_zapper_state(enabled, trigger, hit, screen_pos))
    }

    pub(crate) fn set_replay_host_tilt(&mut self, host_tilt: (f32, f32)) {
        if let Self::Gb(gb) = self {
            gb.emu.set_mbc7_host_tilt(host_tilt.0, host_tilt.1);
        }
    }

    pub(crate) fn set_replay_camera_frame(&mut self, camera_frame: &[u8]) {
        if let Self::Gb(gb) = self {
            gb.emu.set_camera_host_frame(camera_frame);
        }
    }

    pub(crate) fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input_p2(buttons_pressed, dpad_pressed))
    }

    pub(crate) fn set_fds_disk_side(&mut self, side: u8) -> anyhow::Result<()> {
        match self {
            Self::Nes(nes) => nes.set_fds_disk_side(side),
            _ => anyhow::bail!("FDS disk side selection is only available for NES/FDS content"),
        }
    }

    #[cfg(test)]
    pub(crate) fn fds_disk_side(&self) -> Option<u8> {
        match self {
            Self::Nes(nes) => nes.fds_disk_side(),
            _ => None,
        }
    }

    pub(crate) fn media_slot_snapshot(&self) -> Option<zeff_emu_common::media::MediaSlotSnapshot> {
        match self {
            Self::Nes(nes) => nes.media_slot_snapshot(),
            _ => None,
        }
    }

    pub(crate) fn apply_media_event(
        &mut self,
        event: &zeff_emu_common::media::MediaEvent,
    ) -> anyhow::Result<()> {
        match self {
            Self::Nes(nes) => nes.apply_media_event(event),
            _ => anyhow::bail!("current system has no removable media slot"),
        }
    }

    pub(crate) fn game_boy_serial_device(
        &self,
    ) -> Option<zeff_gb_core::hardware::GameBoySerialDevice> {
        match self {
            Self::Gb(gb) => Some(gb.emu.game_boy_serial_device()),
            _ => None,
        }
    }

    pub(crate) fn set_game_boy_serial_device(
        &mut self,
        device: zeff_gb_core::hardware::GameBoySerialDevice,
    ) -> bool {
        match self {
            Self::Gb(gb) => {
                gb.emu.set_game_boy_serial_device(device);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn queue_bardigun_barcode_scan(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        match self {
            Self::Gb(gb) => gb.emu.queue_bardigun_barcode_scan(bytes),
            _ => anyhow::bail!("current system has no Game Boy serial device"),
        }
    }

    pub(crate) fn trigger_barcode_boy_scan(&mut self, digits: &str) -> anyhow::Result<()> {
        match self {
            Self::Gb(gb) => gb.emu.trigger_barcode_boy_scan(digits),
            _ => anyhow::bail!("current system has no Game Boy serial device"),
        }
    }

    pub(crate) fn take_game_boy_printer_jobs(
        &mut self,
    ) -> Vec<zeff_gb_core::hardware::GameBoyPrinterJob> {
        match self {
            Self::Gb(gb) => gb.emu.take_printer_jobs(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn discard_game_boy_printer_jobs(&mut self) {
        if let Self::Gb(gb) = self {
            gb.emu.clear_printer_jobs();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn set_link_peer_present(&mut self, present: bool) -> bool {
        match self {
            Self::Gb(gb) => {
                gb.emu.set_game_boy_link_peer_present(present);
                true
            }
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn game_boy_link_state(
        &self,
    ) -> Option<zeff_gb_core::hardware::bus::GameBoyLinkState> {
        match self {
            Self::Gb(gb) => Some(gb.emu.game_boy_link_state()),
            _ => None,
        }
    }

    pub(crate) fn game_boy_link_replay_state(
        &self,
    ) -> Option<zeff_emu_common::replay::ReplayGameBoyLinkState> {
        match self {
            Self::Gb(gb) => Some(gb.emu.game_boy_link_replay_state()),
            _ => None,
        }
    }

    pub(crate) fn restore_game_boy_link_replay_state(
        &mut self,
        state: zeff_emu_common::replay::ReplayGameBoyLinkState,
    ) -> bool {
        match self {
            Self::Gb(gb) => gb.emu.restore_game_boy_link_replay_state(state),
            _ => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn apply_game_boy_link_reply(
        &mut self,
        reply: zeff_emu_common::replay::ReplayGameBoyLinkReply,
    ) -> bool {
        match self {
            Self::Gb(gb) => {
                gb.emu
                    .apply_game_boy_link_reply(zeff_gb_core::hardware::bus::GameBoyLinkReply {
                        out_byte: reply.out_byte,
                        passive: reply.passive,
                        serial_generation: reply.serial_generation,
                    })
            }
            _ => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn preview_game_boy_link_peer(
        &self,
        peer: &Self,
    ) -> Option<zeff_gb_core::hardware::bus::GameBoyLinkExchangePreview> {
        match (self, peer) {
            (Self::Gb(left), Self::Gb(right)) => {
                Some(left.emu.preview_game_boy_link_peer(&right.emu))
            }
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn try_prepare_game_boy_link_peer(
        &mut self,
        peer: &mut Self,
    ) -> Result<
        zeff_gb_core::hardware::bus::GameBoyLinkPreparedExchange,
        zeff_gb_core::hardware::bus::GameBoyLinkExchangeError,
    > {
        match (self, peer) {
            (Self::Gb(left), Self::Gb(right)) => {
                left.emu.try_prepare_game_boy_link_peer(&mut right.emu)
            }
            _ => unreachable!("paired Game Boy exchange requires two GB backends"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn try_apply_prepared_game_boy_link_reply(
        &mut self,
        transfer: zeff_gb_core::hardware::bus::GameBoyLinkPreparedTransfer,
    ) -> Result<
        zeff_gb_core::hardware::bus::GameBoyLinkTransferExchange,
        zeff_gb_core::hardware::bus::GameBoyLinkExchangeError,
    > {
        match self {
            Self::Gb(gb) => gb.emu.try_apply_prepared_game_boy_link_reply(transfer),
            _ => unreachable!("paired Game Boy reply requires a GB backend"),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn sync_game_boy_remote_link_state(
        &mut self,
        peer_state: zeff_gb_core::hardware::bus::GameBoyLinkState,
        idle_master_response: Option<u8>,
    ) -> bool {
        match self {
            Self::Gb(gb) => gb.emu.sync_game_boy_remote_link_peer_with_idle_response(
                peer_state,
                idle_master_response,
            ),
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn sync_link_peer(&mut self, peer: &mut Self) -> bool {
        match (self, peer) {
            (Self::Gb(left), Self::Gb(right)) => {
                left.emu.sync_game_boy_link_peer(&mut right.emu);
                true
            }
            (Self::Sega8(left), Self::Sega8(right))
                if left.system() == ActiveSystem::GameGear
                    && right.system() == ActiveSystem::GameGear =>
            {
                left.emu.sync_game_gear_link_peer(&mut right.emu);
                true
            }
            (Self::Ws(left), Self::Ws(right)) => {
                left.emu.sync_wonder_swan_link_peer(&mut right.emu);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        dispatch!(self, flush_battery_sram())
    }

    pub(crate) fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        if !self.supports_state_capture() {
            anyhow::bail!("state restore is not supported by this core");
        }
        dispatch!(self, load_state_from_bytes(bytes))
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.is_suspended()
    }

    pub(crate) fn slot_path(&self, slot: u8) -> anyhow::Result<PathBuf> {
        if !self.supports_save_states() {
            anyhow::bail!("save states are not supported by this core");
        }
        crate::save_paths::slot_path(
            self.system().storage_subdir(),
            self.state_extension(),
            self.rom_hash(),
            slot,
        )
    }

    pub(crate) fn auto_save_path(&self) -> Option<PathBuf> {
        self.supports_save_states().then(|| {
            crate::save_paths::auto_save_path(
                self.system().storage_subdir(),
                self.state_extension(),
                self.rom_hash(),
            )
        })
    }

    pub(crate) fn load_state(&mut self, slot: u8) -> anyhow::Result<String> {
        let path = self.slot_path(slot)?;
        let bytes = crate::platform::read_save_data(&path)
            .with_context(|| format!("failed to read save state: {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("save state not found: {}", path.display()))?;
        self.load_state_from_bytes(bytes)?;
        Ok(path.display().to_string())
    }

    pub(crate) fn load_state_from_path(&mut self, path: &Path) -> anyhow::Result<()> {
        if !self.supports_save_states() {
            anyhow::bail!("save states are not supported by this core");
        }
        let bytes = crate::platform::read_save_data(path)
            .with_context(|| format!("failed to read save state: {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("save state not found: {}", path.display()))?;
        self.load_state_from_bytes(bytes)
    }
}

impl MachineTiming for EmuBackend {
    #[inline]
    fn timing_snapshot(&self) -> TimingSnapshot {
        dispatch!(self, timing_snapshot())
    }
}

impl Reset for EmuBackend {
    #[inline]
    fn reset(&mut self) {
        dispatch!(self, reset())
    }
}

impl FrameLifecycle for EmuBackend {
    #[inline]
    fn step_frame(&mut self) {
        dispatch!(self, step_frame())
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        dispatch!(self, frame_count())
    }
}

pub(crate) fn canonicalize_state_bytes_for_replay_hash(system: ActiveSystem, bytes: &mut [u8]) {
    if system == ActiveSystem::GameBoy {
        zeff_gb_core::save_state::canonicalize_replay_hash_bytes(bytes);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wait_for_wonder_swan_remote_link_window(
    emulator: &mut zeff_ws_core::emulator::Emulator,
    link: &mut crate::link::ws::WonderSwanRemoteLink<crate::link::transport::TcpLinkTransport>,
) -> Result<bool, crate::link::LinkSessionError> {
    for _ in 0..WONDER_SWAN_REMOTE_LINK_WAIT_SPINS {
        link.poll_emulator(emulator)?;
        if link.can_advance(emulator) {
            return Ok(true);
        }
        std::thread::yield_now();
    }
    Ok(false)
}

#[cfg(test)]
mod tests;
