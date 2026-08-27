use std::cell::RefCell;

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum FlatAccess {
    MemoryRead { addr: u16, value: u8 },
    MemoryWrite { addr: u16, value: u8 },
    IoRead { port: u8, value: u8 },
    IoWrite { port: u8, value: u8 },
    NmiAcknowledged,
}

struct FlatBus {
    ram: Box<[u8]>,
    ports: [u8; 0x100],
    irq_pending: bool,
    nmi_pending: bool,
    accesses: RefCell<Vec<FlatAccess>>,
}

impl FlatBus {
    fn new() -> Self {
        Self {
            ram: vec![0; 0x1_0000].into_boxed_slice(),
            ports: [0; 0x100],
            irq_pending: false,
            nmi_pending: false,
            accesses: RefCell::new(Vec::new()),
        }
    }

    fn load(&mut self, addr: u16, bytes: &[u8]) {
        let start = usize::from(addr);
        self.ram[start..start + bytes.len()].copy_from_slice(bytes);
    }

    fn take_accesses(&mut self) -> Vec<FlatAccess> {
        std::mem::take(self.accesses.get_mut())
    }
}

impl Z80Bus for FlatBus {
    fn cpu_read(&self, addr: u16) -> u8 {
        let value = self.ram[usize::from(addr)];
        self.accesses
            .borrow_mut()
            .push(FlatAccess::MemoryRead { addr, value });
        value
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.ram[usize::from(addr)] = value;
        self.accesses
            .get_mut()
            .push(FlatAccess::MemoryWrite { addr, value });
    }

    fn io_read(&mut self, port: u8) -> u8 {
        let value = self.ports[usize::from(port)];
        self.accesses
            .get_mut()
            .push(FlatAccess::IoRead { port, value });
        value
    }

    fn io_write(&mut self, port: u8, value: u8) {
        self.ports[usize::from(port)] = value;
        self.accesses
            .get_mut()
            .push(FlatAccess::IoWrite { port, value });
    }

    fn maskable_interrupt_pending(&self) -> bool {
        self.irq_pending
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        self.nmi_pending
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        let pending = std::mem::take(&mut self.nmi_pending);
        if pending {
            self.accesses.get_mut().push(FlatAccess::NmiAcknowledged);
        }
        pending
    }
}

#[test]
fn flat_bus_executes_store_at_an_arbitrary_address() {
    let mut cpu = Cpu::new();
    cpu.regs.pc = 0x4123;
    cpu.regs.a = 0x5A;
    let mut bus = FlatBus::new();
    bus.load(0x4123, &[0x32, 0x34, 0xA2]);

    let fetched = cpu.step_with_bus(&mut bus).expect("store should execute");

    assert_eq!(fetched.cycles, 13);
    assert_eq!(cpu.regs.pc, 0x4126);
    assert_eq!(bus.ram[0xA234], 0x5A);
    assert_eq!(
        bus.take_accesses(),
        vec![
            FlatAccess::MemoryRead {
                addr: 0x4123,
                value: 0x32,
            },
            FlatAccess::MemoryRead {
                addr: 0x4124,
                value: 0x34,
            },
            FlatAccess::MemoryRead {
                addr: 0x4125,
                value: 0xA2,
            },
            FlatAccess::MemoryWrite {
                addr: 0xA234,
                value: 0x5A,
            },
        ]
    );
}

#[test]
fn step_and_step_with_bus_match_for_safe_ram_store() {
    let mut flat_cpu = Cpu::new();
    flat_cpu.regs.pc = 0xC000;
    flat_cpu.regs.a = 0x5A;
    let mut flat_bus = FlatBus::new();
    flat_bus.load(0xC000, &[0x32, 0x00, 0xC1]);

    let mut step_cpu = flat_cpu.clone();
    let mut step_bus = FlatBus::new();
    step_bus.load(0xC000, &[0x32, 0x00, 0xC1]);

    let flat_fetched = flat_cpu
        .step_with_bus(&mut flat_bus)
        .expect("flat store should execute");
    let step_fetched = step_cpu.step(&mut step_bus).expect("store should execute");

    assert_eq!(flat_fetched, step_fetched);
    assert_eq!(flat_cpu.regs, step_cpu.regs);
    assert_eq!(flat_cpu.state, step_cpu.state);
    assert_eq!(flat_cpu.cycles, step_cpu.cycles);
    assert_eq!(flat_bus.ram[0xC100], step_bus.ram[0xC100]);
    assert_eq!(flat_bus.take_accesses(), step_bus.take_accesses());
}

#[test]
fn flat_bus_preserves_immediate_port_io() {
    let mut cpu = Cpu::new();
    cpu.regs.pc = 0x7100;
    cpu.regs.a = 0x8E;
    let mut bus = FlatBus::new();
    bus.load(0x7100, &[0xD3, 0x42, 0xDB, 0x42]);

    assert_eq!(
        cpu.step_with_bus(&mut bus)
            .expect("OUT should execute")
            .cycles,
        11
    );
    assert_eq!(bus.ports[0x42], 0x8E);
    bus.ports[0x42] = 0x37;
    assert_eq!(
        cpu.step_with_bus(&mut bus)
            .expect("IN should execute")
            .cycles,
        11
    );
    assert_eq!(cpu.regs.a, 0x37);
    assert_eq!(
        bus.take_accesses(),
        vec![
            FlatAccess::MemoryRead {
                addr: 0x7100,
                value: 0xD3,
            },
            FlatAccess::MemoryRead {
                addr: 0x7101,
                value: 0x42,
            },
            FlatAccess::IoWrite {
                port: 0x42,
                value: 0x8E,
            },
            FlatAccess::MemoryRead {
                addr: 0x7102,
                value: 0xDB,
            },
            FlatAccess::MemoryRead {
                addr: 0x7103,
                value: 0x42,
            },
            FlatAccess::IoRead {
                port: 0x42,
                value: 0x37,
            },
        ]
    );
}

#[test]
fn flat_bus_nmi_acknowledges_and_pushes_the_return_pc() {
    let mut cpu = Cpu::new();
    cpu.regs.pc = 0x9ABC;
    cpu.regs.sp = 0xD000;
    let mut bus = FlatBus::new();
    bus.nmi_pending = true;

    let interrupt = cpu.step_with_bus(&mut bus).expect("NMI should execute");

    assert_eq!(interrupt.cycles, 11);
    assert_eq!(cpu.regs.pc, Z80_INTERRUPT_VECTOR_NMI);
    assert_eq!(cpu.regs.sp, 0xCFFE);
    assert!(!bus.nmi_pending);
    assert_eq!(bus.ram[0xCFFF], 0x9A);
    assert_eq!(bus.ram[0xCFFE], 0xBC);
    assert_eq!(
        bus.take_accesses(),
        vec![
            FlatAccess::NmiAcknowledged,
            FlatAccess::MemoryWrite {
                addr: 0xCFFF,
                value: 0x9A,
            },
            FlatAccess::MemoryWrite {
                addr: 0xCFFE,
                value: 0xBC,
            },
        ]
    );
}

#[test]
fn flat_bus_im1_irq_remains_level_triggered_after_stack_push() {
    let mut cpu = Cpu::new();
    cpu.regs.pc = 0x1357;
    cpu.regs.sp = 0xE000;
    cpu.interrupt_mode = InterruptMode::Im1;
    cpu.interrupt_flip_flop_1 = true;
    cpu.interrupt_flip_flop_2 = true;
    let mut bus = FlatBus::new();
    bus.irq_pending = true;

    let interrupt = cpu.step_with_bus(&mut bus).expect("IRQ should execute");

    assert_eq!(interrupt.cycles, 13);
    assert_eq!(cpu.regs.pc, Z80_INTERRUPT_VECTOR_IM1);
    assert_eq!(cpu.regs.sp, 0xDFFE);
    assert!(!cpu.interrupt_flip_flop_1);
    assert!(!cpu.interrupt_flip_flop_2);
    assert!(bus.irq_pending);
    assert_eq!(bus.ram[0xDFFF], 0x13);
    assert_eq!(bus.ram[0xDFFE], 0x57);
    assert_eq!(
        bus.take_accesses(),
        vec![
            FlatAccess::MemoryWrite {
                addr: 0xDFFF,
                value: 0x13,
            },
            FlatAccess::MemoryWrite {
                addr: 0xDFFE,
                value: 0x57,
            },
        ]
    );
}

fn run_mixed_program() -> (u16, u64, u8, u8, u8, Vec<FlatAccess>) {
    let mut cpu = Cpu::new();
    cpu.regs.pc = 0x6000;
    let mut bus = FlatBus::new();
    bus.ports[0x34] = 0xA5;
    bus.load(
        0x6000,
        &[
            0x3E, 0x11, 0x32, 0x00, 0x90, 0xD3, 0x33, 0xDB, 0x34, 0x32, 0x01, 0x90,
        ],
    );

    for _ in 0..5 {
        cpu.step_with_bus(&mut bus)
            .expect("mixed program instruction should execute");
    }

    (
        cpu.regs.pc,
        cpu.cycles,
        cpu.regs.a,
        bus.ram[0x9000],
        bus.ram[0x9001],
        bus.take_accesses(),
    )
}

#[test]
fn flat_bus_mixed_program_is_deterministic_from_fresh_state() {
    let first = run_mixed_program();
    let second = run_mixed_program();

    assert_eq!(first, second);
    assert_eq!(first.0, 0x600C);
    assert_eq!(first.1, 55);
    assert_eq!(first.2, 0xA5);
    assert_eq!(first.3, 0x11);
    assert_eq!(first.4, 0xA5);
}
