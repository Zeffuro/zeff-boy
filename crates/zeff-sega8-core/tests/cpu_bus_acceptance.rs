use std::num::NonZeroU64;

use zeff_emu_common::cpu::{CpuCore, CpuStep};
use zeff_sega8_core::hardware::cpu::{Cpu, FetchedInstruction, SegaCpuBus};

struct CpuBusHarness<C, B> {
    cpu: C,
    bus: B,
    cycles: u64,
}

impl<C, B> CpuBusHarness<C, B>
where
    C: CpuCore<B>,
{
    fn new(cpu: C, bus: B) -> Self {
        Self {
            cpu,
            bus,
            cycles: 0,
        }
    }

    fn step(&mut self) -> C::Step {
        let step = self.cpu.step_cpu(&mut self.bus);
        self.cycles = self
            .cycles
            .wrapping_add(step.cpu_cycles().map(NonZeroU64::get).unwrap_or(0));
        step
    }
}

struct FlatRam {
    bytes: Box<[u8]>,
}

impl FlatRam {
    fn new() -> Self {
        Self {
            bytes: vec![0; 0x1_0000].into_boxed_slice(),
        }
    }
}

impl SegaCpuBus for FlatRam {
    fn cpu_read(&self, addr: u16) -> u8 {
        self.bytes[usize::from(addr)]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.bytes[usize::from(addr)] = value;
    }

    fn io_read(&mut self, _port: u8) -> u8 {
        0xFF
    }

    fn io_write(&mut self, _port: u8, _value: u8) {}

    fn maskable_interrupt_pending(&self) -> bool {
        false
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        false
    }
}

#[test]
fn cpu_core_runs_with_flat_ram() {
    let mut machine = CpuBusHarness::new(Cpu::new(), FlatRam::new());
    machine.bus.bytes[..5].copy_from_slice(&[0x3E, 0x42, 0x32, 0x00, 0x80]);

    let first: Option<FetchedInstruction> = machine.step();
    assert_eq!(first.expect("LD A,n should execute").cycles, 7);

    let second = machine.step();
    assert_eq!(second.expect("LD (nn),A should execute").cycles, 13);
    assert_eq!(machine.cycles, 20);
    assert_eq!(machine.cpu.regs().pc, 5);
    assert_eq!(machine.bus.bytes[0x8000], 0x42);
}
