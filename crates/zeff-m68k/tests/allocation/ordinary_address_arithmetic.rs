use super::*;
use zeff_m68k::{Registers, SUPERVISOR};

struct Device {
    memory: [u16; 0x4000],
    clock: u64,
    transfers: usize,
    fail: Option<usize>,
}

impl ClockedBus for Device {
    type Error = ();

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, ()> {
        assert_eq!(start.0, self.clock);
        let value = self.memory[(access.bus_address as usize & 0x7fff) / 2];
        let index = self.transfers;
        self.transfers += 1;
        self.clock += 4;
        CycleResult {
            result: if self.fail == Some(index) {
                Err(())
            } else {
                Ok(value)
            },
            wait_clocks: 0,
        }
    }

    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), ()> {
        panic!()
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(start.0, self.clock);
        self.clock += u64::from(clocks);
    }
}

pub(super) fn address_steps() {
    let mut device = Device {
        memory: [0; 0x4000],
        clock: 0,
        transfers: 0,
        fail: None,
    };
    let mut cases = 0;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for _ in 0..100 {
        for family in [9u16, 13] {
            for long in [false, true] {
                for destination in 0..8 {
                    for mode in 2..=7 {
                        for source in 0..if mode == 7 { 4 } else { 8 } {
                            for status in [0x071f, SUPERVISOR | 0x071f] {
                                let opcode = family << 12
                                    | destination << 9
                                    | if long { 7 << 6 } else { 3 << 6 }
                                    | mode << 3
                                    | source;
                                let mut registers = Registers::new();
                                registers.set_status_register(status);
                                registers.pc = 0x2000;
                                registers.d = [0x1234_5678; 8];
                                registers.d[6] = 4;
                                registers.usp = 0x4000;
                                registers.ssp = 0x5000;
                                for index in 0..8 {
                                    registers
                                        .set_address_register(index, 0x3008 + index as u32 * 8);
                                }
                                let base = registers.address_register(source as usize).unwrap();
                                let stride = if long { 4 } else { 2 };
                                let (address, extensions, words) = match (mode, source) {
                                    (2 | 3, _) => (base, 0, [0, 0]),
                                    (4, _) => (base - stride, 0, [0, 0]),
                                    (5, _) => (base + 16, 1, [16, 0]),
                                    (6, _) => (base + 4, 1, [0x6000, 0]),
                                    (7, 0) => (0x3010, 1, [0x3010, 0]),
                                    (7, 1) => (0x1234_3010, 2, [0x1234, 0x3010]),
                                    (7, 2) => (0x2012, 1, [16, 0]),
                                    (7, 3) => (0x2006, 1, [0x6000, 0]),
                                    _ => unreachable!(),
                                };
                                device.memory.fill(0x4e71);
                                device.memory[0x2000 / 2] = opcode;
                                for (index, word) in words.into_iter().take(extensions).enumerate()
                                {
                                    device.memory[0x2002 / 2 + index] = word;
                                }
                                device.memory[0x2002 / 2 + extensions] = 0x7201;
                                device.memory[0x2004 / 2 + extensions] = 0x7402;
                                let address_index = (address as usize & 0x7fff) / 2;
                                if long {
                                    device.memory[address_index] = 0;
                                    device.memory[(address_index + 1) & 0x3fff] = 2;
                                } else {
                                    device.memory[address_index] = 2;
                                }
                                let queue = [opcode, device.memory[0x2002 / 2]];
                                let retained = device.memory[0x2002 / 2 + extensions];
                                let fetched = device.memory[0x2004 / 2 + extensions];
                                let count = extensions + if long { 2 } else { 1 } + 1;
                                for failure in 0..=count {
                                    let mut expected = registers;
                                    if mode == 4 || mode == 3 && (!long || failure > 0) {
                                        expected.set_address_register(
                                            source as usize,
                                            if mode == 4 {
                                                base - stride
                                            } else {
                                                base + stride
                                            },
                                        );
                                    }
                                    let sampled =
                                        expected.address_register(destination as usize).unwrap();
                                    let result = if family == 9 {
                                        sampled.wrapping_sub(2)
                                    } else {
                                        sampled.wrapping_add(2)
                                    };
                                    if failure >= count - 1 {
                                        expected.set_address_register(
                                            destination as usize,
                                            if failure == count {
                                                result
                                            } else {
                                                sampled & 0xffff_0000 | result & 0xffff
                                            },
                                        );
                                    }
                                    let mut cpu = PrefetchCpu::from_prefetched(
                                        registers,
                                        queue,
                                        CpuClocks(0),
                                    )
                                    .unwrap();
                                    device.clock = 0;
                                    device.transfers = 0;
                                    device.fail = (failure != count).then_some(failure);
                                    let outcome = black_box(cpu.step_instruction(&mut device));
                                    if failure == count {
                                        expected.pc += 2 + extensions as u32 * 2;
                                        assert!(outcome.is_ok());
                                        assert_eq!(cpu.queued_words(), Some([retained, fetched]));
                                    } else {
                                        assert!(matches!(
                                            outcome,
                                            Err(zeff_m68k::timing::TimingFault::Bus { .. })
                                        ));
                                        assert_eq!(cpu.queued_words(), Some(queue));
                                        let transfers = device.transfers;
                                        assert!(matches!(
                                            cpu.step_instruction(&mut device),
                                            Err(zeff_m68k::timing::TimingFault::Frozen)
                                        ));
                                        assert_eq!(device.transfers, transfers);
                                    }
                                    assert_eq!(cpu.registers, expected);
                                    assert_eq!(cpu.clock().0, device.clock);
                                    cases += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 1_120_000);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
