use zeff_emu_common::cpu::{CpuCore, CpuStep};
use zeff_gb_core::hardware::cpu::{Cpu, GbCpuBus, GbCpuRead, GbCpuTiming};
use zeff_gb_core::hardware::types::ImeState;

struct FlatRam {
    memory: Box<[u8]>,
    interrupt_enable: u8,
    interrupt_flags: u8,
    halt_interrupts: Option<u8>,
    speed_switch_timing: Option<GbCpuTiming>,
    writes: Vec<(u16, u8, u64)>,
    internal_advances: Vec<u64>,
    oam_write_corruptions: Vec<u16>,
}

impl FlatRam {
    fn new() -> Self {
        Self {
            memory: vec![0; 0x1_0000].into_boxed_slice(),
            interrupt_enable: 0,
            interrupt_flags: 0,
            halt_interrupts: None,
            speed_switch_timing: None,
            writes: Vec::new(),
            internal_advances: Vec::new(),
            oam_write_corruptions: Vec::new(),
        }
    }

    fn load(&mut self, address: u16, bytes: &[u8]) {
        let start = usize::from(address);
        self.memory[start..start + bytes.len()].copy_from_slice(bytes);
    }

    fn timing(cpu_t_cycles: u64) -> GbCpuTiming {
        GbCpuTiming {
            cpu_t_cycles,
            master_ticks: cpu_t_cycles,
        }
    }
}

impl GbCpuBus for FlatRam {
    fn cpu_read_byte_timed(&mut self, addr: u16, _access_start_master_ticks: u64) -> GbCpuRead {
        GbCpuRead {
            value: self.memory[usize::from(addr)],
            timing: Self::timing(4),
        }
    }

    fn cpu_write_byte_timed(
        &mut self,
        addr: u16,
        value: u8,
        access_start_master_ticks: u64,
    ) -> GbCpuTiming {
        self.memory[usize::from(addr)] = value;
        self.writes.push((addr, value, access_start_master_ticks));
        Self::timing(4)
    }

    fn advance_cpu_t_cycles(&mut self, cpu_t_cycles: u64) -> GbCpuTiming {
        self.internal_advances.push(cpu_t_cycles);
        Self::timing(cpu_t_cycles)
    }

    fn pending_interrupts_for_cpu(&self) -> u8 {
        self.interrupt_enable & self.interrupt_flags & 0x1F
    }

    fn pending_interrupts_for_halt(&self) -> u8 {
        self.halt_interrupts
            .unwrap_or(self.interrupt_enable & self.interrupt_flags & 0x1F)
    }

    fn clear_interrupt_bit(&mut self, bit: usize) {
        self.interrupt_flags &= !(1 << bit);
    }

    fn maybe_trigger_oam_write_corruption(&mut self, addr: u16) {
        self.oam_write_corruptions.push(addr);
    }

    fn try_cgb_speed_switch(&mut self) -> Option<GbCpuTiming> {
        self.speed_switch_timing.take()
    }
}

fn step(cpu: &mut Cpu, bus: &mut FlatRam) -> u64 {
    let step = <Cpu as CpuCore<FlatRam>>::step_cpu(cpu, bus);
    assert_eq!(step.cpu_cycles().map(|cycles| cycles.get()), Some(step));
    step
}

#[test]
fn cpu_core_runs_load_store_and_internal_cycles_on_flat_ram() {
    let mut cpu = Cpu::new();
    let mut bus = FlatRam::new();
    cpu.pc = 0x4000;
    cpu.sp = 0x1234;
    bus.load(0x4000, &[0x3E, 0x42, 0xEA, 0x00, 0x80, 0x33]);

    assert_eq!(step(&mut cpu, &mut bus), 8);
    assert_eq!(step(&mut cpu, &mut bus), 16);
    assert_eq!(step(&mut cpu, &mut bus), 8);

    assert_eq!(cpu.pc, 0x4006);
    assert_eq!(cpu.sp, 0x1235);
    assert_eq!(cpu.cycles, 32);
    assert_eq!(cpu.last_step_master_ticks, 8);
    assert_eq!(bus.memory[0x8000], 0x42);
    assert_eq!(bus.writes, vec![(0x8000, 0x42, 12)]);
    assert_eq!(bus.internal_advances, vec![4]);
}

#[test]
fn cpu_core_acknowledges_interrupts_on_flat_ram() {
    let mut cpu = Cpu::new();
    let mut bus = FlatRam::new();
    cpu.pc = 0xC123;
    cpu.sp = 0xFFFE;
    cpu.ime = ImeState::Enabled;
    bus.interrupt_enable = 0x04;
    bus.interrupt_flags = 0x04;

    assert_eq!(step(&mut cpu, &mut bus), 20);

    assert_eq!(cpu.pc, 0x0050);
    assert_eq!(cpu.sp, 0xFFFC);
    assert_eq!(bus.interrupt_flags, 0);
    assert_eq!(bus.writes, vec![(0xFFFD, 0xC1, 8), (0xFFFC, 0x23, 12)]);
    assert_eq!(bus.internal_advances, vec![8, 4]);
}

#[test]
fn cpu_core_reports_oam_write_corruption_hook_on_flat_ram() {
    let mut cpu = Cpu::new();
    let mut bus = FlatRam::new();
    cpu.pc = 0x6000;
    cpu.regs.h = 0xC1;
    cpu.regs.l = 0x23;
    bus.load(0x6000, &[0xF9]);

    assert_eq!(step(&mut cpu, &mut bus), 8);

    assert_eq!(cpu.sp, 0xC123);
    assert_eq!(bus.oam_write_corruptions, vec![0xC123]);
    assert_eq!(bus.internal_advances, vec![4]);
}

#[test]
fn cpu_core_uses_the_halt_interrupt_query_on_flat_ram() {
    let mut cpu = Cpu::new();
    let mut bus = FlatRam::new();
    cpu.pc = 0x7000;
    bus.halt_interrupts = Some(0x01);
    bus.load(0x7000, &[0x76]);

    assert_eq!(step(&mut cpu, &mut bus), 4);

    assert!(cpu.halt_bug_active);
    assert_eq!(cpu.pc, 0x7001);
}

#[test]
fn cpu_core_accounts_for_an_alternate_bus_speed_switch() {
    let mut cpu = Cpu::new();
    let mut bus = FlatRam::new();
    cpu.pc = 0x7100;
    bus.speed_switch_timing = Some(GbCpuTiming {
        cpu_t_cycles: 16_400,
        master_ticks: 4_100,
    });
    bus.load(0x7100, &[0x10, 0x00]);

    assert_eq!(step(&mut cpu, &mut bus), 16_408);

    assert_eq!(cpu.last_step_master_ticks, 4_108);
    assert!(bus.speed_switch_timing.is_none());
}
