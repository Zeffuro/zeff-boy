use std::path::{Path, PathBuf};

use anyhow::Context;
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{
    AddressWatchHit, AddressWatchpoint, BreakpointHitCondition, DebugEvent, InstructionTraceStore,
    WatchType,
};
use zeff_emu_common::memory::{
    MemoryRegionDescriptor, MemoryRegionKind, MemoryRegionView, resolve_memory_region,
};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_emu_common::time::{
    ClockRate, FrameLifecycle, MachineTiming, MasterTicks, Reset, TimingSnapshot,
};
use zeff_pce_core::hardware::{
    ARCADE_CARD_RAM_LEN, CDROM2_BRAM_LEN, CdDisc, ControllerPort, FivePortMultitap,
    HUCARD_BANK_LEN, MEMORY_BASE128_RAM_LEN, PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR,
    PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR, POPULOUS_HUCARD_RAM_LEN, PSG_CLOCK_DENOMINATOR,
    PSG_CLOCK_NUMERATOR, PSG_ZERO_FREQUENCY_PERIOD, PadButtons, PceArcadeCardMode,
    PceCartridgeDescriptor, PceConsoleWiring, PceControllerMode, PceCpuDebugSnapshot,
    PceHardwareDebugSnapshot, PceHardwareTopology, PceHuCardBoard, PceMachine, PceMemoryBaseMode,
    SixButtonExtraButtons, VCE_PALETTE_COLORS, VDC_SATB_WORDS, VDC_VRAM_BYTES, VceColor,
    normalize_hucard_image,
};
#[cfg(test)]
use zeff_pce_core::hardware::{PCE_ACTIVE_FRAME_WIDTH, PCEAS_HEADER_LEN};

use super::pce_display::project_presented_frame;
#[cfg(test)]
use super::pce_display::{
    OPAQUE_BLACK, ProjectionRow, project_base_rgba_rows, project_sgx_rgba_rows,
};
#[cfg(test)]
use super::pce_profiles::{
    LEMMINGS_JAPAN_CANONICAL_DISC_SHA256, TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256,
};
use super::pce_profiles::{
    PceCdTitleMetadata, automatic_arcade_card_enabled, automatic_controller_mode,
    automatic_memory_base_enabled, canonical_title_metadata,
};
use crate::audio_tooling::{
    AudioChannelDescriptor, AudioChannelId, AudioSemanticCaps, AudioSemanticFrame, AudioTopology,
    AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT,
};
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{DebuggableEmulator, EmulatorCore};
use crate::settings::{PceOverscanMode, PcePaletteMode};

pub(crate) const PCE_PRESENTED_WIDTH: usize = zeff_pce_core::hardware::PCE_HOST_FRAME_WIDTH;
pub(crate) const PCE_PRESENTED_HEIGHT: usize = zeff_pce_core::hardware::PCE_HOST_FRAME_HEIGHT;
pub(crate) const PCE_PRESENTED_RGBA_BYTES: usize =
    zeff_pce_core::hardware::PCE_HOST_FRAME_RGBA_BYTES;
const BACKEND_STATE_MAGIC: &[u8; 8] = b"ZBPCEBE\0";
const BACKEND_STATE_VERSION: u32 = 1;
const MAX_CORE_STATE_BYTES: usize = 8 * 1024 * 1024;
const MEMORY_BASE128_ALIASES: &[&str] = &["mb128", "memorybase128", "memory_base"];
const MEMORY_BASE128_REGION: MemoryRegionDescriptor = MemoryRegionDescriptor {
    id: "memory_base_128",
    label: "Memory Base 128 RAM",
    kind: MemoryRegionKind::SaveRam,
    size: Some(MEMORY_BASE128_RAM_LEN),
    address_bits: None,
    readable: true,
    writable: false,
    side_effect_free: true,
    copyable: true,
    view: MemoryRegionView::Physical,
    aliases: MEMORY_BASE128_ALIASES,
};
const ARCADE_CARD_RAM_ALIASES: &[&str] = &["arcade", "acram", "arcade_card"];
const ARCADE_CARD_RAM_REGION: MemoryRegionDescriptor = MemoryRegionDescriptor {
    id: "arcade_card_ram",
    label: "Arcade Card RAM",
    kind: MemoryRegionKind::ExternalWorkRam,
    size: Some(ARCADE_CARD_RAM_LEN),
    address_bits: Some(21),
    readable: true,
    writable: false,
    side_effect_free: true,
    copyable: true,
    view: MemoryRegionView::Physical,
    aliases: ARCADE_CARD_RAM_ALIASES,
};

pub(crate) struct PceGraphicsSnapshot {
    pub(crate) vdc1: PceVdcGraphicsSnapshot,
    pub(crate) vdc2: Option<PceVdcGraphicsSnapshot>,
    pub(crate) palette: [VceColor; 512],
}

pub(crate) struct PceVdcGraphicsSnapshot {
    pub(crate) vram: Vec<u16>,
    pub(crate) registers: [u16; 0x14],
}
const PCE_AUDIO_CHANNELS: &[AudioChannelDescriptor] = &[
    AudioChannelDescriptor {
        id: AudioChannelId(0),
        name: "PCE PSG Wave 0",
        group: "HuC6280 PSG",
        class: AudioVoiceClass::Wavetable,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(1),
        name: "PCE PSG Wave 1",
        group: "HuC6280 PSG",
        class: AudioVoiceClass::Wavetable,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(2),
        name: "PCE PSG Wave 2",
        group: "HuC6280 PSG",
        class: AudioVoiceClass::Wavetable,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(3),
        name: "PCE PSG Wave 3",
        group: "HuC6280 PSG",
        class: AudioVoiceClass::Wavetable,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(4),
        name: "PCE PSG Wave/Noise 4",
        group: "HuC6280 PSG",
        class: AudioVoiceClass::WavetableNoise,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(5),
        name: "PCE PSG Wave/Noise 5",
        group: "HuC6280 PSG",
        class: AudioVoiceClass::WavetableNoise,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
];

pub(crate) struct PceBackend {
    machine: PceMachine,
    paths: BackendPaths,
    rom_hash: [u8; 32],
    source_crc32: Option<u32>,
    source_disc_hash: Option<[u8; 32]>,
    framebuffer: Box<[u8]>,
    frame_count: u64,
    pending_runtime_fault: Option<String>,
    overscan_mode: PceOverscanMode,
    palette_mode: PcePaletteMode,
    pce_controller_mode: PceControllerMode,
    pce_memory_base_mode: PceMemoryBaseMode,
    pce_arcade_card_mode: PceArcadeCardMode,
    mouse_host_buttons: PadButtons,
    sram_recovery: crate::save_paths::SramRecoverySession,
    memory_base_force_flush: bool,
}

pub(crate) struct PceCdBackendConfig {
    pub(crate) system_card_board: PceHuCardBoard,
    pub(crate) cue_path: PathBuf,
    pub(crate) source_path: PathBuf,
    pub(crate) content_hash: [u8; 32],
    pub(crate) content_crc32: u32,
    pub(crate) source_disc_hash: [u8; 32],
    pub(crate) console_wiring: PceConsoleWiring,
    pub(crate) arcade_card_mode: PceArcadeCardMode,
}

impl PceBackend {
    pub(crate) fn battery_components(&self) -> Vec<(&'static str, Vec<u8>)> {
        let mut components = Vec::with_capacity(2);
        if let Some(cdrom2) = self.cdrom2() {
            components.push((crate::save_paths::SRAM_COMPONENT, cdrom2.bram().to_vec()));
        }
        components.push((
            "memory-base-128",
            self.machine
                .devices()
                .controller()
                .memory_base128()
                .ram()
                .to_vec(),
        ));
        components
    }

    pub(crate) fn new_cdrom2(
        system_card_rom: Vec<u8>,
        disc: CdDisc,
        config: PceCdBackendConfig,
    ) -> anyhow::Result<Self> {
        let recovery_identity = disc.content_hash();
        let arcade_card_enabled = match config.arcade_card_mode {
            PceArcadeCardMode::Automatic => {
                automatic_arcade_card_enabled(Some(config.source_disc_hash))
            }
            PceArcadeCardMode::Enabled => true,
            PceArcadeCardMode::Disabled => false,
        };
        anyhow::ensure!(
            !arcade_card_enabled || config.system_card_board == PceHuCardBoard::SystemCardV3,
            "Arcade Card requires a System Card v3 CD environment"
        );
        let machine = PceMachine::with_cdrom2_system_card_controller_and_arcade_card(
            system_card_rom,
            config.system_card_board,
            disc,
            config.console_wiring,
            ControllerPort::two_button(),
            arcade_card_enabled,
        )?;
        let paths = BackendPaths::with_source_path(config.cue_path, config.source_path);
        let mut sram_recovery =
            crate::save_paths::battery_sram_session(paths.rom_path(), "pce", recovery_identity);
        sram_recovery.begin(
            &memory_base128_path(),
            "pce",
            recovery_identity,
            "memory-base-128",
        );
        let mut backend = Self {
            machine,
            paths,
            rom_hash: config.content_hash,
            source_crc32: Some(config.content_crc32),
            source_disc_hash: Some(config.source_disc_hash),
            framebuffer: vec![0; PCE_PRESENTED_RGBA_BYTES].into_boxed_slice(),
            frame_count: 0,
            pending_runtime_fault: None,
            overscan_mode: PceOverscanMode::default(),
            palette_mode: PcePaletteMode::default(),
            pce_controller_mode: PceControllerMode::Automatic,
            pce_memory_base_mode: PceMemoryBaseMode::Automatic,
            pce_arcade_card_mode: if arcade_card_enabled {
                PceArcadeCardMode::Enabled
            } else {
                PceArcadeCardMode::Disabled
            },
            mouse_host_buttons: PadButtons::empty(),
            sram_recovery,
            memory_base_force_flush: false,
        };
        backend.project_presented_frame();
        backend.update_controller_mode(PceControllerMode::Automatic);
        backend.update_memory_base_mode(PceMemoryBaseMode::Automatic);
        Ok(backend)
    }

    #[cfg(test)]
    pub(crate) fn new(hucard_rom: Vec<u8>, rom_path: PathBuf) -> anyhow::Result<Self> {
        Self::with_paths(hucard_rom, BackendPaths::new(rom_path), None, None)
    }

    #[cfg(test)]
    pub(crate) fn new_with_console_wiring(
        hucard_rom: Vec<u8>,
        rom_path: PathBuf,
        console_wiring: PceConsoleWiring,
    ) -> anyhow::Result<Self> {
        Self::with_paths(
            hucard_rom,
            BackendPaths::new(rom_path),
            Some(console_wiring),
            None,
        )
    }

    pub(crate) fn new_with_overrides(
        hucard_rom: Vec<u8>,
        rom_path: PathBuf,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
    ) -> anyhow::Result<Self> {
        Self::with_paths(
            hucard_rom,
            BackendPaths::new(rom_path),
            console_wiring,
            hucard_board,
        )
    }

    pub(crate) fn with_source_path_and_overrides(
        hucard_rom: Vec<u8>,
        rom_path: PathBuf,
        source_path: PathBuf,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
    ) -> anyhow::Result<Self> {
        Self::with_paths(
            hucard_rom,
            BackendPaths::with_source_path(rom_path, source_path),
            console_wiring,
            hucard_board,
        )
    }

    fn with_paths(
        hucard_rom: Vec<u8>,
        paths: BackendPaths,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
    ) -> anyhow::Result<Self> {
        let hucard_rom = normalize_hucard_image(hucard_rom)?;
        anyhow::ensure!(!hucard_rom.is_empty(), "PC Engine HuCard image is empty");
        anyhow::ensure!(
            hucard_rom.len().is_multiple_of(HUCARD_BANK_LEN),
            "PC Engine HuCard image length must be a multiple of {HUCARD_BANK_LEN} bytes"
        );
        let rom_hash = zeff_firmware::sha256_bytes(&hucard_rom);
        Self::with_validated_paths_and_hash(
            hucard_rom,
            paths,
            console_wiring,
            hucard_board,
            rom_hash,
        )
    }

    fn with_validated_paths_and_hash(
        hucard_rom: Vec<u8>,
        paths: BackendPaths,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
        rom_hash: [u8; 32],
    ) -> anyhow::Result<Self> {
        let mut cartridge = PceCartridgeDescriptor::from_sha256(rom_hash);
        if let Some(console_wiring) = console_wiring {
            cartridge = cartridge.with_console_wiring(console_wiring);
        }
        if let Some(hucard_board) = hucard_board {
            cartridge = cartridge.with_hucard_board(hucard_board);
        }
        let machine = PceMachine::with_cartridge_and_controller(
            hucard_rom,
            cartridge,
            ControllerPort::two_button(),
        )?;
        let mut sram_recovery =
            crate::save_paths::battery_sram_session(paths.rom_path(), "pce", rom_hash);
        sram_recovery.begin(&memory_base128_path(), "pce", rom_hash, "memory-base-128");
        let mut backend = Self {
            machine,
            paths,
            rom_hash,
            source_crc32: None,
            source_disc_hash: None,
            framebuffer: vec![0; PCE_PRESENTED_RGBA_BYTES].into_boxed_slice(),
            frame_count: 0,
            pending_runtime_fault: None,
            overscan_mode: PceOverscanMode::default(),
            palette_mode: PcePaletteMode::default(),
            pce_controller_mode: PceControllerMode::Automatic,
            pce_memory_base_mode: PceMemoryBaseMode::Automatic,
            pce_arcade_card_mode: PceArcadeCardMode::Disabled,
            mouse_host_buttons: PadButtons::empty(),
            sram_recovery,
            memory_base_force_flush: false,
        };
        backend.project_presented_frame();
        backend.update_controller_mode(PceControllerMode::Automatic);
        backend.update_memory_base_mode(PceMemoryBaseMode::Automatic);
        Ok(backend)
    }

    pub(crate) fn source_path(&self) -> &Path {
        self.paths.source_path()
    }

    pub(crate) fn debug_cpu_snapshot(&self) -> PceCpuDebugSnapshot {
        self.machine.debug_snapshot()
    }

    pub(crate) fn debug_presented_frame(&self) -> zeff_pce_core::hardware::PcePresentedFrame<'_> {
        self.machine.presented_frame()
    }

    pub(crate) fn debug_suspend(&mut self) {
        self.machine.debug_suspend();
    }

    pub(crate) fn debug_continue(&mut self) {
        self.machine.debug_continue();
    }

    pub(crate) fn debug_step(&mut self) {
        self.machine.debug_step();
    }

    pub(crate) fn is_cpu_suspended(&self) -> bool {
        self.machine.is_cpu_suspended()
    }

    pub(crate) fn set_opcode_history_enabled(&mut self, enabled: bool) {
        self.machine.set_opcode_history_enabled(enabled);
    }

    pub(crate) fn recent_opcodes(
        &self,
        count: usize,
    ) -> Vec<zeff_pce_core::hardware::PceOpcodeHistoryEntry> {
        self.machine.recent_opcodes(count)
    }

    pub(crate) fn iter_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.machine.iter_breakpoints()
    }

    pub(crate) fn iter_one_shot_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.machine.iter_one_shot_breakpoints()
    }

    pub(crate) fn iter_breakpoint_hit_conditions(
        &self,
    ) -> impl Iterator<Item = BreakpointHitCondition> + '_ {
        self.machine.iter_breakpoint_hit_conditions()
    }

    pub(crate) fn debug_watchpoints(&self) -> &[AddressWatchpoint] {
        self.machine.debug_watchpoints()
    }

    pub(crate) fn debug_hit_breakpoint(&self) -> Option<Address> {
        self.machine.debug_hit_breakpoint()
    }

    pub(crate) fn debug_hit_watchpoint(&self) -> Option<&AddressWatchHit> {
        self.machine.debug_hit_watchpoint()
    }

    pub(crate) fn iter_event_breakpoints(&self) -> impl Iterator<Item = DebugEvent> + '_ {
        self.machine.iter_event_breakpoints()
    }

    pub(crate) fn debug_hit_event(&self) -> Option<DebugEvent> {
        self.machine.debug_hit_event()
    }

    pub(crate) fn instruction_trace(&self) -> &InstructionTraceStore {
        self.machine.instruction_trace()
    }

    pub(crate) fn debug_hardware_snapshot(&self) -> PceHardwareDebugSnapshot {
        self.machine.devices().debug_snapshot()
    }

    pub(crate) fn set_apu_debug_capture_enabled(&mut self, enabled: bool) {
        self.machine
            .devices_mut()
            .psg_mut()
            .set_debug_capture_enabled(enabled);
    }

    pub(crate) fn psg_master_debug_samples_ordered(&self) -> Vec<f32> {
        self.machine.devices().psg().master_debug_samples_ordered()
    }

    pub(crate) fn psg_channel_debug_samples_ordered(&self, channel: usize) -> Vec<f32> {
        self.machine
            .devices()
            .psg()
            .channel_debug_samples_ordered(channel)
    }

    pub(crate) fn debug_graphics_snapshot(&self) -> PceGraphicsSnapshot {
        let devices = self.machine.devices();
        let vdc_snapshot = |vdc: &zeff_pce_core::hardware::HuC6270| PceVdcGraphicsSnapshot {
            vram: vdc.vram().to_vec(),
            registers: vdc.debug_snapshot().registers,
        };
        PceGraphicsSnapshot {
            vdc1: vdc_snapshot(devices.vdc()),
            vdc2: devices
                .supergrafx_video()
                .map(|video| vdc_snapshot(video.vdc2())),
            palette: *devices.vce().palette(),
        }
    }

    pub(crate) fn debug_peek8(&self, address: Address) -> u8 {
        u16::try_from(address)
            .ok()
            .map_or(0xFF, |address| self.machine.debug_peek_cpu8(address))
    }

    pub(crate) fn debug_peek_physical8(&self, address: u32) -> u8 {
        self.machine.debug_peek_physical8(address)
    }

    pub(crate) fn debug_write8(&mut self, address: Address, value: u8) {
        if let Ok(address) = u16::try_from(address) {
            self.machine.debug_write_cpu8(address, value);
        }
    }

    pub(crate) fn rom_offset_for_cpu_address(&self, address: u16) -> Option<u32> {
        self.machine.rom_offset_for_cpu_address(address)
    }

    pub(crate) fn debug_execute_guest_call(
        &mut self,
        target: u16,
        instruction_budget: u64,
    ) -> Result<u64, String> {
        self.machine
            .debug_execute_guest_call(target, instruction_budget)
    }

    pub(crate) fn rom_mapping_token(&self) -> u64 {
        self.machine.rom_mapping_token()
    }

    pub(crate) fn hucard_rom(&self) -> &[u8] {
        self.machine.hucard_rom()
    }

    pub(crate) fn hucard_board(&self) -> PceHuCardBoard {
        self.machine.hucard_board()
    }

    pub(crate) fn hardware_topology(&self) -> PceHardwareTopology {
        self.machine.hardware_topology()
    }

    pub(crate) fn console_wiring(&self) -> PceConsoleWiring {
        self.machine.devices().console_wiring()
    }

    pub(crate) const fn controller_mode(&self) -> PceControllerMode {
        self.pce_controller_mode
    }

    pub(crate) const fn memory_base_mode(&self) -> PceMemoryBaseMode {
        self.pce_memory_base_mode
    }

    pub(crate) const fn arcade_card_mode(&self) -> PceArcadeCardMode {
        self.pce_arcade_card_mode
    }

    pub(crate) fn normalized_disc_hash(&self) -> Option<[u8; 32]> {
        self.cdrom2().map(|cdrom| cdrom.disc().content_hash())
    }

    pub(crate) fn controller_profile_hash(&self) -> [u8; 32] {
        self.source_disc_hash.unwrap_or(self.rom_hash)
    }

    pub(crate) const fn source_crc32(&self) -> Option<u32> {
        self.source_crc32
    }

    pub(crate) const fn source_disc_hash(&self) -> Option<[u8; 32]> {
        self.source_disc_hash
    }

    pub(crate) fn canonical_title_metadata(&self) -> Option<&'static PceCdTitleMetadata> {
        self.source_disc_hash.and_then(canonical_title_metadata)
    }

    pub(crate) fn cdrom2(&self) -> Option<&zeff_pce_core::hardware::CdRom2> {
        self.machine.devices().cdrom2()
    }

    pub(crate) fn step_frame_bounded(&mut self) -> anyhow::Result<u64> {
        const MAX_FRAME_MASTER_TICKS: u64 =
            zeff_pce_core::hardware::PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE * 263 * 2;
        const MAX_CPU_BOUNDARIES: u64 = 250_000;

        anyhow::ensure!(!self.machine.faulted(), "PC Engine machine is faulted");
        let starting_ticks = self.machine.master_ticks();
        for cpu_boundaries in 1..=MAX_CPU_BOUNDARIES {
            let step = self.machine.step_boundary()?;
            if step.frames_published() != 0 {
                self.frame_count = self.frame_count.saturating_add(step.frames_published());
                self.project_presented_frame();
                return Ok(cpu_boundaries);
            }
            let elapsed_ticks = self.machine.master_ticks().saturating_sub(starting_ticks);
            if elapsed_ticks > MAX_FRAME_MASTER_TICKS {
                let snapshot = self.machine.debug_snapshot();
                anyhow::bail!(
                    "PC Engine produced no frame after {cpu_boundaries} CPU boundaries and {elapsed_ticks} master ticks (PC={:04X}, VCE line={})",
                    snapshot.registers().pc,
                    snapshot.vce_line_index(),
                );
            }
        }
        let snapshot = self.machine.debug_snapshot();
        anyhow::bail!(
            "PC Engine produced no frame after {MAX_CPU_BOUNDARIES} CPU boundaries (PC={:04X}, master ticks={}, VCE line={})",
            snapshot.registers().pc,
            snapshot.master_ticks(),
            snapshot.vce_line_index(),
        )
    }

    pub(crate) fn load_cd_bram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(
            bytes.len() == CDROM2_BRAM_LEN,
            "PC Engine CD backup RAM is {} bytes, expected {CDROM2_BRAM_LEN}",
            bytes.len()
        );
        let cdrom =
            self.machine.devices_mut().cdrom2_mut().ok_or_else(|| {
                anyhow::anyhow!("PC Engine CD backup RAM requires a CD-ROM2 unit")
            })?;
        cdrom.bram_mut().copy_from_slice(bytes);
        Ok(())
    }

    pub(crate) fn load_memory_base128(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.machine
            .devices_mut()
            .controller_mut()
            .memory_base128_mut()
            .load_ram(bytes)
    }

    pub(crate) fn try_load_memory_base128(&mut self) -> anyhow::Result<Option<String>> {
        let path = memory_base128_path();
        self.try_load_memory_base128_from_path(&path)
    }

    pub(crate) fn firmware_manifests(&self) -> &[zeff_emu_common::replay::ReplayFirmwareManifest] {
        self.paths.firmware_manifests()
    }

    pub(crate) fn set_firmware_manifests(
        &mut self,
        firmware_manifests: Vec<zeff_emu_common::replay::ReplayFirmwareManifest>,
    ) {
        self.paths.set_firmware_manifests(firmware_manifests);
    }

    fn project_presented_frame(&mut self) {
        let presented = self.machine.presented_frame();
        project_presented_frame(
            presented,
            self.machine.hardware_topology(),
            self.overscan_mode,
            self.palette_mode,
            &mut self.framebuffer,
        );
    }

    pub(crate) fn set_display_config(
        &mut self,
        overscan_mode: PceOverscanMode,
        palette_mode: PcePaletteMode,
    ) {
        if self.overscan_mode == overscan_mode && self.palette_mode == palette_mode {
            return;
        }
        self.overscan_mode = overscan_mode;
        self.palette_mode = palette_mode;
        self.project_presented_frame();
    }

    fn set_pad_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        if let Some(mouse) = self.machine.devices_mut().controller_mut().mouse_mut() {
            let pad = map_pad_buttons(buttons_pressed, dpad_pressed);
            mouse.set_buttons(self.mouse_host_buttons | pad);
            let horizontal = i16::from(pad.contains(PadButtons::LEFT))
                - i16::from(pad.contains(PadButtons::RIGHT));
            let vertical =
                i16::from(pad.contains(PadButtons::UP)) - i16::from(pad.contains(PadButtons::DOWN));
            mouse.accumulate_motion(horizontal * 4, vertical * 4);
            return;
        }
        if self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::One,
            buttons_pressed,
            dpad_pressed,
        ) {
            return;
        }
        let controller = self.machine.devices_mut().controller_mut();
        if let Some(pad) = controller.six_button_pad_mut() {
            pad.standard_pad_mut()
                .set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
            pad.set_extra_buttons(map_six_button_extra_buttons(buttons_pressed));
        } else if let Some(pad) = controller.two_button_pad_mut() {
            pad.set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
        }
    }

    fn set_multitap_pad_input(
        &mut self,
        port: zeff_pce_core::hardware::MultitapPort,
        buttons_pressed: u8,
        dpad_pressed: u8,
    ) -> bool {
        let Some(multitap) = self.machine.devices_mut().controller_mut().multitap_mut() else {
            return false;
        };
        match multitap.port_mut(port) {
            zeff_pce_core::hardware::MultitapDevice::TwoButton(pad) => {
                pad.set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
            }
            zeff_pce_core::hardware::MultitapDevice::SixButton(pad) => {
                pad.standard_pad_mut()
                    .set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
                pad.set_extra_buttons(map_six_button_extra_buttons(buttons_pressed));
            }
            zeff_pce_core::hardware::MultitapDevice::Disconnected => {}
        }
        true
    }

    fn effective_controller_mode(&self, requested: PceControllerMode) -> PceControllerMode {
        match requested {
            PceControllerMode::Automatic => {
                automatic_controller_mode(self.controller_profile_hash())
            }
            explicit => explicit,
        }
    }

    fn update_controller_mode(&mut self, requested: PceControllerMode) {
        let effective = self.effective_controller_mode(requested);
        if self.pce_controller_mode == effective {
            return;
        }
        let device = match effective {
            PceControllerMode::Mouse => zeff_pce_core::hardware::ControllerDevice::Mouse(
                zeff_pce_core::hardware::PceMouse::new(),
            ),
            PceControllerMode::SixButton => zeff_pce_core::hardware::ControllerDevice::SixButton(
                zeff_pce_core::hardware::SixButtonPad::new(),
            ),
            PceControllerMode::Multitap => {
                zeff_pce_core::hardware::ControllerDevice::Multitap(FivePortMultitap::new([
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                ]))
            }
            PceControllerMode::Automatic | PceControllerMode::TwoButton => {
                zeff_pce_core::hardware::ControllerDevice::TwoButton(
                    zeff_pce_core::hardware::TwoButtonPad::new(),
                )
            }
        };
        self.machine.devices_mut().set_controller_device(device);
        self.pce_controller_mode = effective;
    }

    fn update_memory_base_mode(&mut self, requested: PceMemoryBaseMode) {
        let enabled = match requested {
            PceMemoryBaseMode::Automatic => automatic_memory_base_enabled(self.source_disc_hash()),
            PceMemoryBaseMode::Enabled => true,
            PceMemoryBaseMode::Disabled => false,
        };
        self.machine
            .devices_mut()
            .controller_mut()
            .set_memory_base128_connected(enabled);
        self.pce_memory_base_mode = if enabled {
            PceMemoryBaseMode::Enabled
        } else {
            PceMemoryBaseMode::Disabled
        };
    }

    fn try_load_memory_base128_from_path(&mut self, path: &Path) -> anyhow::Result<Option<String>> {
        #[cfg(not(target_arch = "wasm32"))]
        let bytes = crate::platform::read_save_data(path);
        #[cfg(target_arch = "wasm32")]
        let bytes = crate::platform::read_sram_data(
            path,
            "pce",
            self.normalized_disc_hash().unwrap_or(self.rom_hash),
            "memory-base-128",
        );
        let Some(bytes) = bytes
            .with_context(|| format!("failed to read Memory Base 128 save {}", path.display()))?
        else {
            return Ok(None);
        };
        self.load_memory_base128(&bytes)?;
        Ok(Some(path.display().to_string()))
    }

    fn flush_memory_base128_to_path(&mut self, path: &Path) -> anyhow::Result<Option<String>> {
        let media_identity = self.normalized_disc_hash().unwrap_or(self.rom_hash);
        let force_flush = self.memory_base_force_flush;
        let bytes = {
            let memory_base = self
                .machine
                .devices_mut()
                .controller_mut()
                .memory_base128_mut();
            if !memory_base.is_dirty() && !force_flush {
                return Ok(None);
            }
            memory_base.ram().to_vec()
        };
        crate::save_paths::write_recoverable_sram_file(
            &mut self.sram_recovery,
            path,
            "pce",
            media_identity,
            "memory-base-128",
            &bytes,
        )
        .with_context(|| format!("failed to write Memory Base 128 save {}", path.display()))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.machine
                .devices_mut()
                .controller_mut()
                .memory_base128_mut()
                .clear_dirty();
            self.memory_base_force_flush = false;
        }
        Ok(Some(path.display().to_string()))
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn acknowledge_battery_commit(&mut self, snapshot_still_matches: bool) {
        if !snapshot_still_matches {
            return;
        }
        self.machine
            .devices_mut()
            .controller_mut()
            .memory_base128_mut()
            .clear_dirty();
        self.memory_base_force_flush = false;
    }

    fn flush_persistent_data(&mut self, memory_base_path: &Path) -> anyhow::Result<Option<String>> {
        let media_identity = self.normalized_disc_hash().unwrap_or(self.rom_hash);
        let bram = self.cdrom2().map(|cdrom| cdrom.bram().to_vec());
        let bram_result = crate::save_paths::flush_battery_sram(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            "pce",
            media_identity,
            bram,
        );
        let memory_base_result = self.flush_memory_base128_to_path(memory_base_path);
        let (bram_path, memory_base_path) = match (bram_result, memory_base_result) {
            (Ok(bram), Ok(memory_base)) => (bram, memory_base),
            (Err(bram), Err(memory_base)) => {
                anyhow::bail!(
                    "failed to save PC Engine BRAM ({bram:#}) and Memory Base 128 ({memory_base:#})"
                )
            }
            (Err(error), Ok(_)) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
        };
        Ok(match (bram_path, memory_base_path) {
            (None, None) => None,
            (Some(path), None) | (None, Some(path)) => Some(path),
            (Some(bram), Some(memory_base)) => Some(format!("{bram}, {memory_base}")),
        })
    }
}

fn memory_base128_path() -> PathBuf {
    crate::platform::save_dir("pce").join("mb128.sav")
}

impl DebuggableEmulator for PceBackend {
    fn add_breakpoint(&mut self, addr: Address) {
        if let Ok(addr) = u16::try_from(addr) {
            self.machine.add_breakpoint(addr);
        }
    }

    fn add_one_shot_breakpoint(&mut self, addr: Address) {
        if let Ok(addr) = u16::try_from(addr) {
            self.machine.add_one_shot_breakpoint(addr);
        }
    }

    fn add_breakpoint_after(&mut self, addr: Address, target_hits: u64) {
        if let Ok(addr) = u16::try_from(addr) {
            self.machine.add_breakpoint_after(addr, target_hits);
        }
    }

    fn set_event_breakpoint(&mut self, event: zeff_emu_common::debug::DebugEvent, enabled: bool) {
        if matches!(event, DebugEvent::Interrupt | DebugEvent::Dma) {
            self.machine.set_event_breakpoint(event, enabled);
        }
    }

    fn add_watchpoint_range(&mut self, start: Address, end: Address, watch_type: WatchType) {
        if let (Ok(start), Ok(end)) = (u16::try_from(start), u16::try_from(end)) {
            self.machine.add_watchpoint_range(start, end, watch_type);
        }
    }

    fn remove_watchpoint(&mut self, start: Address, end: Address, watch_type: WatchType) {
        if let (Ok(start), Ok(end)) = (u16::try_from(start), u16::try_from(end)) {
            self.machine.remove_watchpoint(start, end, watch_type);
        }
    }

    fn remove_breakpoint(&mut self, addr: Address) {
        if let Ok(addr) = u16::try_from(addr) {
            self.machine.remove_breakpoint(addr);
        }
    }

    fn toggle_breakpoint(&mut self, addr: Address) {
        if let Ok(addr) = u16::try_from(addr) {
            self.machine.toggle_breakpoint(addr);
        }
    }

    fn cpu_peek8(&self, addr: Address) -> u8 {
        self.debug_peek8(addr)
    }

    fn cpu_write8(&mut self, addr: Address, val: u8) {
        self.debug_write8(addr, val);
    }

    fn is_cpu_suspended(&self) -> bool {
        self.is_cpu_suspended()
    }

    fn debug_continue(&mut self) {
        self.debug_continue();
    }

    fn debug_step(&mut self) {
        self.debug_step();
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.set_opcode_history_enabled(enabled);
    }

    fn set_instruction_trace_enabled(&mut self, enabled: bool) {
        self.machine.set_instruction_trace_enabled(enabled);
    }

    fn set_instruction_trace_capacity(&mut self, capacity: usize) {
        self.machine.set_instruction_trace_capacity(capacity);
    }

    fn clear_instruction_trace(&mut self) {
        self.machine.clear_instruction_trace();
    }
}

impl EmulatorCore for PceBackend {
    fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    fn state_restores_framebuffer(&self) -> bool {
        true
    }

    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.machine.drain_audio_samples_into(buf);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.machine.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.machine.set_sample_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        self.machine.set_channel_mutes(mutes);
    }

    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_pad_input(buttons_pressed, dpad_pressed);
    }

    fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::Two,
            buttons_pressed,
            dpad_pressed,
        );
    }

    fn set_input_p3(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::Three,
            buttons_pressed,
            dpad_pressed,
        );
    }

    fn set_input_p4(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::Four,
            buttons_pressed,
            dpad_pressed,
        );
    }

    fn set_input_p5(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::Five,
            buttons_pressed,
            dpad_pressed,
        );
    }

    fn set_pce_mouse_state(
        &mut self,
        mode: PceControllerMode,
        delta_x: i16,
        delta_y: i16,
        buttons_pressed: u8,
    ) {
        self.update_controller_mode(mode);
        self.mouse_host_buttons = map_pad_buttons(buttons_pressed, 0);
        if let Some(mouse) = self.machine.devices_mut().controller_mut().mouse_mut() {
            mouse.set_buttons(self.mouse_host_buttons);
            mouse.accumulate_motion(delta_x, delta_y);
        }
    }

    fn set_pce_memory_base_mode(&mut self, mode: PceMemoryBaseMode) {
        self.update_memory_base_mode(mode);
    }

    fn is_suspended(&self) -> bool {
        self.machine.faulted() || self.machine.is_cpu_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        let memory_base_path = memory_base128_path();
        self.flush_persistent_data(&memory_base_path)
    }

    fn save_ram_kind(&self) -> SaveRamKind {
        match self.machine.hucard_board() {
            PceHuCardBoard::Populous => SaveRamKind::mapper_ram_unknown(POPULOUS_HUCARD_RAM_LEN),
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3
                if self.cdrom2().is_some() =>
            {
                SaveRamKind::known_battery_backed(CDROM2_BRAM_LEN)
            }
            PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce
                if self
                    .machine
                    .devices()
                    .controller()
                    .memory_base128()
                    .is_connected() =>
            {
                SaveRamKind::known_battery_backed(MEMORY_BASE128_RAM_LEN)
            }
            PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce => SaveRamKind::none(),
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3 => SaveRamKind::none(),
        }
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            self.pending_runtime_fault.is_none(),
            "faulted PC Engine backends cannot be saved"
        );
        let core_state = zeff_pce_core::hardware::save_state::encode_state(&self.machine)
            .context("failed to encode PC Engine core state")?;
        let mut writer = StateWriter::with_capacity(core_state.len() + 32);
        writer.write_bytes(BACKEND_STATE_MAGIC);
        writer.write_u32(BACKEND_STATE_VERSION);
        writer.write_u64(self.frame_count);
        writer.write_u8(self.mouse_host_buttons.bits());
        writer.write_vec(&core_state);
        Ok(writer.into_bytes())
    }

    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let mut reader = StateReader::new(&bytes);
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        anyhow::ensure!(
            &magic == BACKEND_STATE_MAGIC,
            "not a valid PC Engine backend save-state"
        );
        let version = reader.read_u32()?;
        anyhow::ensure!(
            version == BACKEND_STATE_VERSION,
            "unsupported PC Engine backend save-state version {version}"
        );
        let frame_count = reader.read_u64()?;
        let mouse_host_buttons = PadButtons::from_bits_retain(reader.read_u8()?);
        let core_state = reader.read_vec(MAX_CORE_STATE_BYTES)?;
        anyhow::ensure!(
            reader.is_exhausted(),
            "PC Engine backend save-state has unexpected trailing data"
        );

        zeff_pce_core::hardware::save_state::decode_state(&mut self.machine, &core_state)
            .context("failed to decode PC Engine core state")?;
        self.frame_count = frame_count;
        self.mouse_host_buttons = mouse_host_buttons;
        self.pce_controller_mode = match self.machine.devices().controller().device() {
            zeff_pce_core::hardware::ControllerDevice::Disconnected => PceControllerMode::Automatic,
            zeff_pce_core::hardware::ControllerDevice::TwoButton(_) => PceControllerMode::TwoButton,
            zeff_pce_core::hardware::ControllerDevice::SixButton(_) => PceControllerMode::SixButton,
            zeff_pce_core::hardware::ControllerDevice::Multitap(_) => PceControllerMode::Multitap,
            zeff_pce_core::hardware::ControllerDevice::Mouse(_) => PceControllerMode::Mouse,
        };
        self.pce_memory_base_mode = if self
            .machine
            .devices()
            .controller()
            .memory_base128()
            .is_connected()
        {
            PceMemoryBaseMode::Enabled
        } else {
            PceMemoryBaseMode::Disabled
        };
        self.pce_arcade_card_mode = if self.machine.devices().arcade_card().is_some() {
            PceArcadeCardMode::Enabled
        } else {
            PceArcadeCardMode::Disabled
        };
        self.pending_runtime_fault = None;
        self.memory_base_force_flush = self
            .machine
            .devices()
            .controller()
            .memory_base128()
            .is_connected();
        self.project_presented_frame();
        Ok(())
    }

    fn rom_path(&self) -> &Path {
        self.paths.rom_path()
    }

    fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    fn memory_regions(&self) -> Vec<MemoryRegionDescriptor> {
        let supergrafx = self.machine.devices().supergrafx_video().is_some();
        let mut video_ram = MemoryRegionDescriptor::video_ram(self.video_ram_len());
        let mut oam = MemoryRegionDescriptor::oam(self.oam_len());
        if supergrafx {
            video_ram.view = MemoryRegionView::Aggregate;
            oam.view = MemoryRegionView::Aggregate;
        }
        let mut regions = vec![
            MemoryRegionDescriptor::cpu_address_space(16),
            MemoryRegionDescriptor::system_ram(self.machine.mapped_work_ram().len()),
            video_ram,
            MemoryRegionDescriptor::palette_ram(self.palette_ram_len()),
            oam,
            MemoryRegionDescriptor::framebuffer(self.framebuffer.len()),
        ];
        if self.cdrom2().is_some() {
            regions.insert(2, MemoryRegionDescriptor::save_ram(CDROM2_BRAM_LEN));
        } else if self.machine.hucard_ram().is_some() {
            regions.insert(2, MemoryRegionDescriptor::save_ram(POPULOUS_HUCARD_RAM_LEN));
        }
        if self
            .machine
            .devices()
            .controller()
            .memory_base128()
            .is_connected()
        {
            regions.insert(regions.len() - 1, MEMORY_BASE128_REGION);
        }
        if self.machine.devices().arcade_card().is_some() {
            regions.insert(regions.len() - 1, ARCADE_CARD_RAM_REGION);
        }
        regions
    }

    fn system_ram_len(&self) -> usize {
        self.machine.mapped_work_ram().len()
    }

    fn video_ram_len(&self) -> usize {
        let vdc_count = 1 + usize::from(self.machine.devices().supergrafx_video().is_some());
        VDC_VRAM_BYTES * vdc_count
    }

    fn palette_ram_len(&self) -> usize {
        VCE_PALETTE_COLORS * size_of::<u16>()
    }

    fn oam_len(&self) -> usize {
        let vdc_count = 1 + usize::from(self.machine.devices().supergrafx_video().is_some());
        VDC_SATB_WORDS * size_of::<u16>() * vdc_count
    }

    fn supports_audio(&self) -> bool {
        true
    }

    fn supports_cheats(&self) -> bool {
        true
    }

    fn apply_ram_cheats(&mut self, cheats: &[crate::cheats::CheatPatch]) {
        zeff_pce_core::hardware::apply_pce_cheats(&mut self.machine, cheats);
    }

    fn debug_suspend(&mut self) {
        self.machine.debug_suspend();
    }

    fn supports_save_states(&self) -> bool {
        true
    }

    fn supports_guest_calls(&self) -> bool {
        true
    }

    fn supports_debugger(&self) -> bool {
        true
    }

    fn supports_symbol_loading(&self) -> bool {
        true
    }

    fn supports_execution_controls(&self) -> bool {
        true
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    fn audio_semantic_frame(&self) -> Option<AudioSemanticFrame> {
        Some(pce_audio_semantic_frame(
            self.frame_count,
            self.machine.devices().psg().channels(),
        ))
    }

    fn audio_topology(&self) -> Option<AudioTopology> {
        Some(AudioTopology {
            generation: 1,
            channels: PCE_AUDIO_CHANNELS,
        })
    }

    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let region = resolve_memory_region(&self.memory_regions(), id_or_alias)
            .ok_or_else(|| anyhow::anyhow!("unknown memory region '{id_or_alias}'"))?;
        match region.kind {
            MemoryRegionKind::SystemRam => {
                out.clear();
                out.extend_from_slice(self.machine.mapped_work_ram());
                Ok(region)
            }
            MemoryRegionKind::VideoRam => {
                out.clear();
                append_words_le(out, self.machine.devices().vdc().vram());
                if let Some(video) = self.machine.devices().supergrafx_video() {
                    append_words_le(out, video.vdc2().vram());
                }
                Ok(region)
            }
            MemoryRegionKind::PaletteRam => {
                out.clear();
                for color in self.machine.devices().vce().palette() {
                    out.extend_from_slice(&color.raw().to_le_bytes());
                }
                Ok(region)
            }
            MemoryRegionKind::Oam => {
                out.clear();
                append_words_le(out, self.machine.devices().vdc().satb());
                if let Some(video) = self.machine.devices().supergrafx_video() {
                    append_words_le(out, video.vdc2().satb());
                }
                Ok(region)
            }
            MemoryRegionKind::ExternalWorkRam if region.id == ARCADE_CARD_RAM_REGION.id => {
                out.clear();
                out.extend_from_slice(
                    self.machine
                        .devices()
                        .arcade_card()
                        .expect("Arcade Card RAM region requires Arcade Card hardware")
                        .ram(),
                );
                Ok(region)
            }
            MemoryRegionKind::Framebuffer => {
                out.clear();
                out.extend_from_slice(&self.framebuffer);
                Ok(region)
            }
            MemoryRegionKind::SaveRam => {
                out.clear();
                if region.id == MEMORY_BASE128_REGION.id {
                    out.extend_from_slice(
                        self.machine.devices().controller().memory_base128().ram(),
                    );
                } else if let Some(cdrom) = self.cdrom2() {
                    out.extend_from_slice(cdrom.bram());
                } else {
                    out.extend_from_slice(
                        self.machine
                            .hucard_ram()
                            .expect("save RAM region requires HuCard or CD backup RAM"),
                    );
                }
                Ok(region)
            }
            MemoryRegionKind::CpuAddressSpace => {
                anyhow::bail!("CPU address space is debugger-addressable, not copyable")
            }
            _ => anyhow::bail!(
                "memory region '{}' is not available for PC Engine",
                region.id
            ),
        }
    }

    fn take_runtime_fault(&mut self) -> Option<String> {
        self.pending_runtime_fault.take()
    }
}

impl MachineTiming for PceBackend {
    fn timing_snapshot(&self) -> TimingSnapshot {
        TimingSnapshot::new(
            MasterTicks::new(self.machine.master_ticks()),
            ClockRate::from_ratio(
                PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR,
                PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR,
            ),
        )
    }
}

impl Reset for PceBackend {
    fn reset(&mut self) {
        self.machine.reset();
        self.frame_count = 0;
        self.pending_runtime_fault = None;
        self.project_presented_frame();
    }
}

impl FrameLifecycle for PceBackend {
    fn step_frame(&mut self) {
        if self.machine.faulted() {
            return;
        }
        match self.machine.run_until_frame() {
            Ok(run) => {
                self.frame_count = self.frame_count.saturating_add(run.frames_published());
                self.project_presented_frame();
            }
            Err(error) => {
                if self.pending_runtime_fault.is_none() {
                    self.pending_runtime_fault = Some(error.to_string());
                }
            }
        }
    }

    fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

fn pce_audio_semantic_frame(
    frame: u64,
    channels: &[zeff_pce_core::hardware::PsgChannel; zeff_pce_core::hardware::PSG_CHANNEL_COUNT],
) -> AudioSemanticFrame {
    let voices = channels
        .iter()
        .enumerate()
        .map(|(index, channel)| AudioVoiceState {
            channel: AudioChannelId(index as u16),
            name: PCE_AUDIO_CHANNELS[index].name,
            class: if index >= 4 {
                AudioVoiceClass::WavetableNoise
            } else {
                AudioVoiceClass::Wavetable
            },
            active: channel.key_on(),
            pitch_hz: (!channel.noise_enabled()).then(|| {
                let divider = if channel.frequency() == 0 {
                    PSG_ZERO_FREQUENCY_PERIOD as f64
                } else {
                    f64::from(channel.frequency())
                };
                (PSG_CLOCK_NUMERATOR as f64 / PSG_CLOCK_DENOMINATOR as f64) / (divider * 32.0)
            }),
            level: Some(f32::from(channel.amplitude()) / 31.0),
        })
        .collect();
    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
        voices,
    }
}

fn append_words_le(out: &mut Vec<u8>, words: &[u16]) {
    out.reserve(size_of_val(words));
    for &word in words {
        out.extend_from_slice(&word.to_le_bytes());
    }
}

fn map_pad_buttons(buttons: u8, dpad: u8) -> PadButtons {
    let mut mapped = PadButtons::empty();
    mapped.set(PadButtons::I, buttons & (1 << 0) != 0);
    mapped.set(PadButtons::II, buttons & (1 << 1) != 0);
    mapped.set(PadButtons::SELECT, buttons & (1 << 2) != 0);
    mapped.set(PadButtons::RUN, buttons & (1 << 3) != 0);
    mapped.set(PadButtons::RIGHT, dpad & (1 << 0) != 0);
    mapped.set(PadButtons::LEFT, dpad & (1 << 1) != 0);
    mapped.set(PadButtons::UP, dpad & (1 << 2) != 0);
    mapped.set(PadButtons::DOWN, dpad & (1 << 3) != 0);
    mapped
}

fn map_six_button_extra_buttons(buttons: u8) -> SixButtonExtraButtons {
    let mut mapped = SixButtonExtraButtons::empty();
    mapped.set(SixButtonExtraButtons::III, buttons & (1 << 4) != 0);
    mapped.set(SixButtonExtraButtons::IV, buttons & (1 << 5) != 0);
    mapped.set(SixButtonExtraButtons::V, buttons & (1 << 6) != 0);
    mapped.set(SixButtonExtraButtons::VI, buttons & (1 << 7) != 0);
    mapped
}

#[cfg(test)]
#[path = "pce_tests.rs"]
mod tests;
