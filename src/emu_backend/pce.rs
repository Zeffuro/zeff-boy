use std::path::{Path, PathBuf};

use zeff_emu_common::address::Address;
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{
    ClockRate, FrameLifecycle, MachineTiming, MasterTicks, Reset, TimingSnapshot,
};
#[cfg(test)]
use zeff_pce_core::hardware::PCE_ACTIVE_FRAME_WIDTH;
use zeff_pce_core::hardware::{
    CDROM2_BRAM_LEN, CdDisc, ControllerPort, FivePortMultitap,
    PCE_MASTER_CLOCK_NTSC_REFERENCE_MULTIPLIER, PCE_NTSC_REFERENCE_MHZ_DENOMINATOR,
    PCE_NTSC_REFERENCE_MHZ_NUMERATOR, POPULOUS_HUCARD_RAM_LEN, PSG_CLOCK_DENOMINATOR,
    PSG_CLOCK_NUMERATOR, PSG_ZERO_FREQUENCY_PERIOD, PadButtons, PceCartridgeDescriptor,
    PceConsoleWiring, PceControllerMode, PceCpuDebugSnapshot, PceHardwareTopology, PceHuCardBoard,
    PceMachine,
};

use super::pce_display::project_presented_frame;
#[cfg(test)]
use super::pce_display::{
    OPAQUE_BLACK, ProjectionRow, project_base_rgba_rows, project_sgx_rgba_rows,
};
use crate::audio_tooling::{
    AudioChannelDescriptor, AudioChannelId, AudioSemanticCaps, AudioSemanticFrame, AudioTopology,
    AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT,
};
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::EmulatorCore;
use crate::settings::{PceOverscanMode, PcePaletteMode};

pub(crate) const PCE_PRESENTED_WIDTH: usize = 640;
pub(crate) const PCE_PRESENTED_HEIGHT: usize = 480;
pub(crate) const PCE_PRESENTED_RGBA_BYTES: usize = PCE_PRESENTED_WIDTH * PCE_PRESENTED_HEIGHT * 4;
const HUCARD_BANK_LEN: usize = 0x2000;
const PCEAS_HEADER_LEN: usize = 0x200;
pub(crate) const LEMMINGS_JAPAN_CANONICAL_DISC_SHA256: [u8; 32] = [
    0x0f, 0x29, 0x95, 0xc0, 0x20, 0xab, 0x89, 0x33, 0x6c, 0x1e, 0x6d, 0xba, 0x49, 0xc6, 0x3a, 0xd8,
    0x80, 0x5a, 0xad, 0xd2, 0x01, 0xb9, 0x21, 0x87, 0xf8, 0x1d, 0x53, 0xa1, 0x77, 0x04, 0x9a, 0x52,
];
pub(crate) const TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256: [u8; 32] = [
    0x18, 0x14, 0xb8, 0xb2, 0x56, 0x34, 0x70, 0x8b, 0x4b, 0x00, 0xef, 0x3f, 0xa0, 0x49, 0xef, 0xb3,
    0x3d, 0xfd, 0xb5, 0x44, 0x20, 0xf0, 0x46, 0x05, 0xed, 0x11, 0xd5, 0xb0, 0x24, 0x1b, 0xe7, 0xc0,
];

pub(crate) fn automatic_controller_mode(content_hash: [u8; 32]) -> PceControllerMode {
    if content_hash == LEMMINGS_JAPAN_CANONICAL_DISC_SHA256 {
        PceControllerMode::Mouse
    } else if content_hash == TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256 {
        PceControllerMode::Multitap
    } else {
        PceControllerMode::TwoButton
    }
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
    framebuffer: Box<[u8]>,
    frame_count: u64,
    pending_runtime_fault: Option<String>,
    overscan_mode: PceOverscanMode,
    palette_mode: PcePaletteMode,
    pce_controller_mode: PceControllerMode,
    mouse_host_buttons: PadButtons,
}

pub(crate) struct PceCdBackendConfig {
    pub(crate) system_card_board: PceHuCardBoard,
    pub(crate) cue_path: PathBuf,
    pub(crate) source_path: PathBuf,
    pub(crate) content_hash: [u8; 32],
    pub(crate) console_wiring: PceConsoleWiring,
}

impl PceBackend {
    pub(crate) fn new_cdrom2(
        system_card_rom: Vec<u8>,
        disc: CdDisc,
        config: PceCdBackendConfig,
    ) -> anyhow::Result<Self> {
        let machine = PceMachine::with_cdrom2_system_card_and_controller(
            system_card_rom,
            config.system_card_board,
            disc,
            config.console_wiring,
            ControllerPort::two_button(),
        )?;
        let mut backend = Self {
            machine,
            paths: BackendPaths::with_source_path(config.cue_path, config.source_path),
            rom_hash: config.content_hash,
            framebuffer: vec![0; PCE_PRESENTED_RGBA_BYTES].into_boxed_slice(),
            frame_count: 0,
            pending_runtime_fault: None,
            overscan_mode: PceOverscanMode::default(),
            palette_mode: PcePaletteMode::default(),
            pce_controller_mode: PceControllerMode::Automatic,
            mouse_host_buttons: PadButtons::empty(),
        };
        backend.project_presented_frame();
        backend.update_controller_mode(PceControllerMode::Automatic);
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
        let mut backend = Self {
            machine,
            paths,
            rom_hash,
            framebuffer: vec![0; PCE_PRESENTED_RGBA_BYTES].into_boxed_slice(),
            frame_count: 0,
            pending_runtime_fault: None,
            overscan_mode: PceOverscanMode::default(),
            palette_mode: PcePaletteMode::default(),
            pce_controller_mode: PceControllerMode::Automatic,
            mouse_host_buttons: PadButtons::empty(),
        };
        backend.project_presented_frame();
        backend.update_controller_mode(PceControllerMode::Automatic);
        Ok(backend)
    }

    pub(crate) fn source_path(&self) -> &Path {
        self.paths.source_path()
    }

    pub(crate) fn debug_cpu_snapshot(&self) -> PceCpuDebugSnapshot {
        self.machine.debug_snapshot()
    }

    pub(crate) fn debug_peek8(&self, address: Address) -> u8 {
        u16::try_from(address)
            .ok()
            .map_or(0xFF, |address| self.machine.debug_peek_cpu8(address))
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
        let Some(pad) = self
            .machine
            .devices_mut()
            .controller_mut()
            .two_button_pad_mut()
        else {
            return;
        };
        pad.set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
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
        if let zeff_pce_core::hardware::MultitapDevice::TwoButton(pad) = multitap.port_mut(port) {
            pad.set_buttons(map_pad_buttons(buttons_pressed, dpad_pressed));
        }
        true
    }

    fn effective_controller_mode(&self, requested: PceControllerMode) -> PceControllerMode {
        match requested {
            PceControllerMode::Automatic => automatic_controller_mode(self.rom_hash),
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
            PceControllerMode::Multitap => {
                zeff_pce_core::hardware::ControllerDevice::Multitap(FivePortMultitap::new([
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::TwoButton(
                        zeff_pce_core::hardware::TwoButtonPad::new(),
                    ),
                    zeff_pce_core::hardware::MultitapDevice::Disconnected,
                    zeff_pce_core::hardware::MultitapDevice::Disconnected,
                    zeff_pce_core::hardware::MultitapDevice::Disconnected,
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
}

fn normalize_hucard_image(hucard_rom: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    if hucard_rom.len().is_multiple_of(HUCARD_BANK_LEN) {
        return Ok(hucard_rom);
    }
    let payload_len = hucard_rom.len().saturating_sub(PCEAS_HEADER_LEN);
    let has_pceas_header = hucard_rom.len() > PCEAS_HEADER_LEN
        && payload_len.is_multiple_of(HUCARD_BANK_LEN)
        && usize::from(hucard_rom[0]) == payload_len / HUCARD_BANK_LEN
        && hucard_rom[1..PCEAS_HEADER_LEN]
            .iter()
            .all(|&byte| byte == 0);
    anyhow::ensure!(
        has_pceas_header,
        "PC Engine HuCard image length must be a multiple of {HUCARD_BANK_LEN} bytes or carry a valid PCEAS header"
    );
    Ok(hucard_rom[PCEAS_HEADER_LEN..].to_vec())
}

impl EmulatorCore for PceBackend {
    fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
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
            mouse.accumulate_motion(delta_x, delta_y);
        }
    }

    fn is_suspended(&self) -> bool {
        self.machine.faulted()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        crate::save_paths::flush_battery_sram(
            self.paths.rom_path(),
            self.cdrom2().map(|cdrom| cdrom.bram().to_vec()),
        )
    }

    fn save_ram_kind(&self) -> SaveRamKind {
        match self.machine.hucard_board() {
            PceHuCardBoard::Populous => SaveRamKind::mapper_ram_unknown(POPULOUS_HUCARD_RAM_LEN),
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3
                if self.cdrom2().is_some() =>
            {
                SaveRamKind::known_battery_backed(CDROM2_BRAM_LEN)
            }
            PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce => SaveRamKind::none(),
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3 => SaveRamKind::none(),
        }
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("PC Engine save states are not supported")
    }

    fn load_state_from_bytes(&mut self, _bytes: Vec<u8>) -> anyhow::Result<()> {
        anyhow::bail!("PC Engine save states are not supported")
    }

    fn rom_path(&self) -> &Path {
        self.paths.rom_path()
    }

    fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    fn memory_regions(&self) -> Vec<MemoryRegionDescriptor> {
        let mut regions = vec![
            MemoryRegionDescriptor::read_only_cpu_address_space(16),
            MemoryRegionDescriptor::system_ram(self.machine.mapped_work_ram().len()),
            MemoryRegionDescriptor::framebuffer(self.framebuffer.len()),
        ];
        if self.cdrom2().is_some() {
            regions.insert(2, MemoryRegionDescriptor::save_ram(CDROM2_BRAM_LEN));
        } else if self.machine.hucard_ram().is_some() {
            regions.insert(2, MemoryRegionDescriptor::save_ram(POPULOUS_HUCARD_RAM_LEN));
        }
        regions
    }

    fn system_ram_len(&self) -> usize {
        self.machine.mapped_work_ram().len()
    }

    fn supports_audio(&self) -> bool {
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
            MemoryRegionKind::Framebuffer => {
                out.clear();
                out.extend_from_slice(&self.framebuffer);
                Ok(region)
            }
            MemoryRegionKind::SaveRam => {
                out.clear();
                if let Some(cdrom) = self.cdrom2() {
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
            MemoryRegionKind::CpuAddressSpace => anyhow::bail!(
                "CPU address space is read-only and debugger-addressable, not copyable"
            ),
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
                PCE_NTSC_REFERENCE_MHZ_NUMERATOR
                    * 1_000_000
                    * PCE_MASTER_CLOCK_NTSC_REFERENCE_MULTIPLIER,
                PCE_NTSC_REFERENCE_MHZ_DENOMINATOR,
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

#[cfg(test)]
#[path = "pce_tests.rs"]
mod tests;
