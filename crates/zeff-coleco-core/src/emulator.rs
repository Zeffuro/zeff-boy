use anyhow::bail;
use sha2::{Digest, Sha256};
use zeff_emu_common::debug::{AddressDebugController, InstructionTraceStore, OpcodeLog};
use zeff_emu_common::time::{
    ClockRate, FrameLifecycle, MachineTiming, MasterTicks, Reset, TimingSnapshot,
};
use zeff_z80::{Cpu, CpuTrap, ResetState, Z80_RESET_PC};

use crate::bus::Bus;
use crate::constants::{
    BIOS_SIZE, CPU_CLOCK_HZ, DEFAULT_SAMPLE_RATE, MAX_CARTRIDGE_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH,
};
use crate::input::{ControllerPorts, StandardController};
use crate::psg::PSG_CHANNEL_COUNT;

const COLECO_RESET_STATE: ResetState = ResetState::new(Z80_RESET_PC, 0x0000);

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) bios_hash: [u8; 32],
    pub(crate) cartridge_hash: [u8; 32],
    pub(crate) effective_cycles: u64,
    pub(crate) debug: AddressDebugController,
    pub(crate) opcode_log: OpcodeLog<(u16, u8, u32)>,
    pub(crate) instruction_trace: InstructionTraceStore,
    pub(crate) debug_hooks_active: bool,
}

mod debug;
mod runtime;

impl Emulator {
    pub fn new(cartridge: &[u8], bios: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        if bios.len() != BIOS_SIZE {
            bail!(
                "ColecoVision BIOS must be exactly {BIOS_SIZE} bytes, got {}",
                bios.len()
            );
        }
        if cartridge.is_empty() || cartridge.len() > MAX_CARTRIDGE_SIZE {
            bail!(
                "bounded ColecoVision core supports non-empty standard cartridges up to 32 KiB; got {} bytes",
                cartridge.len()
            );
        }
        if !cartridge.starts_with(&[0xAA, 0x55]) && !cartridge.starts_with(&[0x55, 0xAA]) {
            bail!("ColecoVision standard cartridge header must begin with AA55 or 55AA");
        }
        let sample_rate = if sample_rate == 0 {
            DEFAULT_SAMPLE_RATE
        } else {
            sample_rate
        };
        Ok(Self {
            cpu: Cpu::new_with_reset(COLECO_RESET_STATE),
            bus: Bus::new(bios, cartridge, sample_rate)?,
            bios_hash: Sha256::digest(bios).into(),
            cartridge_hash: Sha256::digest(cartridge).into(),
            effective_cycles: 0,
            debug: AddressDebugController::new(),
            opcode_log: OpcodeLog::new(),
            instruction_trace: InstructionTraceStore::default(),
            debug_hooks_active: false,
        })
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.reset();
        self.effective_cycles = 0;
        self.debug.clear_hits();
        self.opcode_log.clear();
        self.instruction_trace.clear();
    }

    pub fn framebuffer(&self) -> &[u8] {
        self.bus.vdp().framebuffer()
    }

    pub const fn framebuffer_dimensions(&self) -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    pub fn frame_count(&self) -> u64 {
        self.bus.vdp().frame_count()
    }

    pub fn effective_cycles(&self) -> u64 {
        self.effective_cycles
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles()
    }

    pub fn timing_snapshot(&self) -> TimingSnapshot {
        <Self as MachineTiming>::timing_snapshot(self)
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn cpu_trap(&self) -> Option<CpuTrap> {
        self.cpu.trap()
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn controller_ports(&self) -> &ControllerPorts {
        self.bus.input()
    }

    pub fn set_controller(&mut self, player: usize, state: StandardController) -> bool {
        let Some(controller) = self.bus.input_mut().player_mut(player) else {
            return false;
        };
        *controller = state;
        true
    }

    pub fn sample_rate(&self) -> u32 {
        self.bus.psg().sample_rate()
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.bus.psg_mut().set_sample_rate(sample_rate);
    }

    pub fn set_audio_generation_enabled(&mut self, enabled: bool) {
        self.bus.psg_mut().set_sample_generation_enabled(enabled);
    }

    pub fn set_audio_muted(&mut self, muted: bool) {
        self.bus.psg_mut().set_muted(muted);
    }

    pub fn set_audio_channel_mutes(&mut self, mutes: [bool; PSG_CHANNEL_COUNT]) {
        self.bus.psg_mut().set_channel_mutes(mutes);
    }

    pub fn drain_audio_samples_into(&mut self, out: &mut Vec<f32>) {
        self.bus.psg_mut().drain_audio_samples_into(out);
    }

    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        let mut samples = Vec::new();
        self.drain_audio_samples_into(&mut samples);
        samples
    }

    pub fn save_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        crate::save_state::decode_state(self, data)
    }
}

impl MachineTiming for Emulator {
    fn timing_snapshot(&self) -> TimingSnapshot {
        TimingSnapshot::new(
            MasterTicks::new(self.effective_cycles),
            ClockRate::from_hz(CPU_CLOCK_HZ),
        )
    }
}

impl Reset for Emulator {
    fn reset(&mut self) {
        Self::reset(self);
    }
}

impl FrameLifecycle for Emulator {
    fn step_frame(&mut self) {
        Self::step_frame(self);
    }

    fn frame_count(&self) -> u64 {
        Self::frame_count(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{CPU_CYCLES_PER_FRAME, MEMORY_OPEN_BUS_VALUE, WORK_RAM_SIZE};

    fn bios_with_program(program: &[u8]) -> Vec<u8> {
        let mut bios = vec![0; BIOS_SIZE];
        bios[..program.len()].copy_from_slice(program);
        bios
    }

    fn cartridge(fill: u8) -> Vec<u8> {
        let mut cartridge = vec![fill; 8 * 1024];
        cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
        cartridge
    }

    #[test]
    fn validates_the_bounded_bios_and_cartridge_contract() {
        let bios = vec![0; BIOS_SIZE];
        assert!(Emulator::new(&cartridge(0), &bios, 48_000).is_ok());
        let mut maximum = vec![0; MAX_CARTRIDGE_SIZE];
        maximum[..2].copy_from_slice(&[0x55, 0xAA]);
        assert!(Emulator::new(&maximum, &bios, 48_000).is_ok());
        assert!(Emulator::new(&cartridge(0), &bios[..BIOS_SIZE - 1], 48_000).is_err());
        assert!(Emulator::new(&[], &bios, 48_000).is_err());
        assert!(Emulator::new(&vec![0; MAX_CARTRIDGE_SIZE + 1], &bios, 48_000).is_err());
    }

    #[test]
    fn rejects_invalid_standard_cartridge_headers() {
        let bios = vec![0; BIOS_SIZE];
        assert!(Emulator::new(&[0x00, 0x00], &bios, 48_000).is_err());
        assert!(Emulator::new(&[0xAA], &bios, 48_000).is_err());
    }

    #[test]
    fn rejects_observed_bios_and_monitor_test_headers() {
        let bios = vec![0; BIOS_SIZE];
        for header in [[0x31, 0xB9], [0xF3, 0xED]] {
            assert!(Emulator::new(&header, &bios, 48_000).is_err());
        }
    }

    #[test]
    fn accepts_partial_standard_cartridges_without_padding_or_mirroring() {
        let bios = vec![0; BIOS_SIZE];
        for size in [12 * 1024, 17 * 1024, 20 * 1024] {
            let mut cartridge = vec![0; size];
            cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
            cartridge[size - 1] = 0xA5;
            let emulator = Emulator::new(&cartridge, &bios, 48_000).unwrap();
            let expected_hash: [u8; 32] = Sha256::digest(&cartridge).into();

            assert_eq!(emulator.bus().cartridge(), cartridge.as_slice());
            assert_eq!(emulator.bus().cpu_peek(0x8000 + size as u16 - 1), 0xA5);
            assert_eq!(
                emulator.bus().cpu_peek(0x8000 + size as u16),
                MEMORY_OPEN_BUS_VALUE
            );
            assert_eq!(emulator.cartridge_hash, expected_hash);
        }
    }

    #[test]
    fn executes_bios_code_against_the_coleco_bus() {
        let bios = bios_with_program(&[0x3E, 0x5A, 0x32, 0x00, 0x60, 0x76]);
        let mut emulator = Emulator::new(&cartridge(0), &bios, 48_000).unwrap();
        assert_eq!(emulator.cpu().regs().sp, 0x0000);
        emulator.step_instruction();
        emulator.step_instruction();
        emulator.step_instruction();

        assert_eq!(emulator.bus().work_ram()[0], 0x5A);
        assert_eq!(emulator.cpu_cycles(), 24);
        assert_eq!(emulator.effective_cycles(), 27);
    }

    #[test]
    fn accounts_for_each_prefixed_m1_wait_using_the_refresh_counter() {
        let bios = bios_with_program(&[0xCB, 0x00, 0x76]);
        let mut emulator = Emulator::new(&cartridge(0), &bios, 48_000).unwrap();
        let instruction = emulator.step_instruction().unwrap();

        assert_eq!(instruction.cycles, 8);
        assert_eq!(emulator.cpu().regs().r & 0x7F, 2);
        assert_eq!(emulator.effective_cycles(), 10);
    }

    #[test]
    fn ld_r_a_cannot_corrupt_the_m1_wait_count() {
        let bios = bios_with_program(&[0x3E, 0x7E, 0xED, 0x4F, 0x76]);
        let mut emulator = Emulator::new(&cartridge(0), &bios, 48_000).unwrap();
        emulator.step_instruction();
        let before = emulator.effective_cycles();
        let instruction = emulator.step_instruction().unwrap();

        assert_eq!(instruction.cycles, 9);
        assert_eq!(emulator.cpu().regs().r & 0x7F, 0x7E);
        assert_eq!(emulator.effective_cycles() - before, 11);
    }

    #[test]
    fn immediate_psg_write_overlaps_ready_with_the_io_cycle() {
        let bios = bios_with_program(&[0x3E, 0x9F, 0xD3, 0xE0, 0x76]);
        let mut emulator = Emulator::new(&cartridge(0), &bios, 48_000).unwrap();
        emulator.step_instruction();
        let before = emulator.effective_cycles();
        let instruction = emulator.step_instruction().unwrap();

        assert_eq!(instruction.cycles, 11);
        assert_eq!(emulator.effective_cycles() - before, 40);
        assert!(emulator.bus().psg().ready());
        assert_eq!(emulator.bus().psg().last_write(), Some(0x9F));
    }

    #[test]
    fn ed_psg_writes_preserve_their_distinct_instruction_tails() {
        let bios = bios_with_program(&[0x01, 0xE0, 0x9F, 0xED, 0x41, 0x76]);
        let mut emulator = Emulator::new(&cartridge(0), &bios, 48_000).unwrap();
        emulator.step_instruction();
        let before = emulator.effective_cycles();

        assert_eq!(emulator.step_instruction().unwrap().cycles, 12);
        assert_eq!(emulator.effective_cycles() - before, 42);
        assert_eq!(emulator.bus().psg().last_write(), Some(0x9F));

        let bios = bios_with_program(&[0x21, 0x09, 0x00, 0x01, 0xE0, 0x01, 0xED, 0xA3, 0x76, 0x8F]);
        let mut emulator = Emulator::new(&cartridge(0), &bios, 48_000).unwrap();
        emulator.step_instruction();
        emulator.step_instruction();
        let before = emulator.effective_cycles();

        assert_eq!(emulator.step_instruction().unwrap().cycles, 16);
        assert_eq!(emulator.effective_cycles() - before, 46);
        assert_eq!(emulator.bus().psg().last_write(), Some(0x8F));

        let bios = bios_with_program(&[
            0x21, 0x09, 0x00, 0x01, 0xE0, 0x02, 0xED, 0xB3, 0x76, 0x9F, 0x8F,
        ]);
        let mut emulator = Emulator::new(&cartridge(0), &bios, 48_000).unwrap();
        emulator.step_instruction();
        emulator.step_instruction();
        let before = emulator.effective_cycles();

        assert_eq!(emulator.step_instruction().unwrap().cycles, 21);
        assert_eq!(emulator.effective_cycles() - before, 51);
        let before = emulator.effective_cycles();
        assert_eq!(emulator.step_instruction().unwrap().cycles, 16);
        assert_eq!(emulator.effective_cycles() - before, 46);
        assert_eq!(emulator.bus().psg().last_write(), Some(0x8F));
    }

    #[test]
    fn steps_one_ntsc_frame_with_only_instruction_overshoot() {
        let bios = bios_with_program(&[0x76]);
        let mut emulator = Emulator::new(&cartridge(0), &bios, 48_000).unwrap();
        emulator.step_frame();

        assert_eq!(emulator.frame_count(), 1);
        assert!(emulator.effective_cycles() >= CPU_CYCLES_PER_FRAME);
        assert!(emulator.effective_cycles() < CPU_CYCLES_PER_FRAME + 5);
        assert_eq!(emulator.bus().work_ram().len(), WORK_RAM_SIZE);
    }
}
