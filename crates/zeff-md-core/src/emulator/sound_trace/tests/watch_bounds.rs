use std::cell::Cell;
use zeff_z80::{BusState, Cpu, PrefixState, Z80Bus};

struct Counter {
    memory: Box<[u8]>,
    reads: Cell<u8>,
    writes: u8,
    irq: bool,
    acknowledges: u8,
}
impl Counter {
    fn new(program: &[u8]) -> Self {
        let mut memory = vec![0; 65536].into_boxed_slice();
        memory[..program.len()].copy_from_slice(program);
        Self {
            memory,
            reads: Cell::new(0),
            writes: 0,
            irq: false,
            acknowledges: 0,
        }
    }
    fn reset_counts(&mut self) {
        self.reads.set(0);
        self.writes = 0;
    }
    fn bound(&self) {
        assert!(self.reads.get() <= 6);
        assert!(self.writes <= 2);
    }
}
impl Z80Bus for Counter {
    fn cpu_read(&self, address: u16) -> u8 {
        self.reads.set(self.reads.get() + 1);
        self.memory[usize::from(address)]
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.writes += 1;
        self.memory[usize::from(address)] = value;
    }
    fn io_read(&mut self, _: u8) -> u8 {
        self.reads.set(self.reads.get() + 1);
        0xff
    }
    fn io_write(&mut self, _: u8, _: u8) {
        self.writes += 1;
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.irq
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        false
    }
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.acknowledges += 1;
        self.irq = false;
        0xff
    }
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_decoder_counts_all_opcode_families_six_reads_two_writes_per_attempt() {
    assert_eq!(std::mem::size_of::<crate::sound_trace::NoSoundCapture>(), 0);
    let mut largest = 0;
    for split in [false, true] {
        for prefix in [
            &[][..],
            &[0xcb],
            &[0xed],
            &[0xdd],
            &[0xfd],
            &[0xdd, 0xcb, 0],
            &[0xfd, 0xcb, 0],
            &[0xdd, 0xed],
            &[0xfd, 0xed],
            &[0xdd, 0xfd, 0xdd],
        ] {
            for opcode in 0..=255 {
                let mut program = prefix.to_vec();
                program.extend([opcode, 0x00, 0x20, 0x00, 0x00]);
                let mut bus = Counter::new(&program);
                let mut cpu = Cpu::new();
                let mut bounded = PrefixState::default();
                let mut state = BusState::default();
                let mut complete = false;
                for _ in 0..24 {
                    bus.reset_counts();
                    let step = if split {
                        cpu.step_bus_segment(&mut bus, &mut state)
                    } else {
                        cpu.step_bounded(&mut bus, &mut bounded)
                    }
                    .unwrap();
                    bus.bound();
                    largest = largest.max(bus.reads.get());
                    if step.complete {
                        complete = true;
                        break;
                    }
                }
                assert!(complete, "{split} {prefix:?} {opcode:02X}");
            }
        }
        for program in [
            &[0xe3][..],
            &[0xed, 0xb0],
            &[0xed, 0xb2],
            &[0x76],
            &[0xed, 0x5e, 0xfb, 0x00, 0x76],
        ] {
            let mut bus = Counter::new(program);
            let mut cpu = Cpu::new();
            let mut bounded = PrefixState::default();
            let mut state = BusState::default();
            for _ in 0..24 {
                bus.reset_counts();
                if cpu.regs().pc >= 4 && bus.acknowledges == 0 {
                    bus.irq = true;
                }
                if split {
                    cpu.step_bus_segment(&mut bus, &mut state);
                } else {
                    cpu.step_bounded(&mut bus, &mut bounded);
                }
                bus.bound();
            }
            if program.starts_with(&[0xed, 0x5e]) {
                assert_eq!(bus.acknowledges, 1);
            }
        }
    }
    assert_eq!(largest, 6);
    let mut bus = Counter::new(&[0xed, 0x4b, 0x00, 0x20]);
    let mut cpu = Cpu::new();
    let step = cpu
        .step_bounded(&mut bus, &mut PrefixState::default())
        .unwrap();
    assert!(step.complete);
    assert_eq!(bus.reads.get(), 6);
}
