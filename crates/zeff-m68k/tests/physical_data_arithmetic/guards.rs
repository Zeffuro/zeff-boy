use super::*;

struct Legacy<'a>(&'a mut BusDevice);
impl ClockedBus for Legacy<'_> {
    type Error = &'static str;
    fn read_cycle(&mut self, a: BusAccess, c: CpuClocks) -> CycleResult<u16, Self::Error> {
        assert_eq!(c.0, self.0.clock);
        self.0.events.push(Event::Read(a, c.0));
        self.0.clock += 4;
        CycleResult {
            result: Ok(self.0.sample(a.bus_address)),
            wait_clocks: 0,
        }
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), Self::Error> {
        panic!("unexpected legacy write")
    }
    fn idle(&mut self, c: CpuClocks, n: u32) {
        self.0.idle_entry(c, n);
    }
}

fn rejected(mut case: Case, opcode: u16, gate: u8) -> bool {
    let (_, mut bus) = case.seed();
    let mut cpu =
        PrefetchCpu::from_prefetched(case.r, [opcode, case.retained()], CpuClocks(case.start))
            .unwrap();
    let expected = match gate {
        0 => InstructionFault::Execution(TimingFault::UnsupportedInstruction {
            pc: case.r.pc,
            opcode,
        }),
        1 => {
            cpu.registers.pc = cpu.registers.pc.wrapping_add(2);
            InstructionFault::Execution(TimingFault::Unprimed)
        }
        2 => {
            cpu = PrefetchCpu::default();
            cpu.registers = case.r;
            InstructionFault::Execution(TimingFault::Unprimed)
        }
        3 => {
            cpu.registers
                .set_status_register(case.r.status_register() | TRACE);
            InstructionFault::TraceUnqualified
        }
        4 => {
            case.r.set_status_register(case.r.status_register() | TRACE);
            cpu = PrefetchCpu::from_prefetched(
                case.r,
                [0x4e71, case.opcode()],
                CpuClocks(case.start),
            )
            .unwrap();
            bus.word(case.r.pc.wrapping_add(4), case.retained());
            if cpu.step_instruction(&mut Legacy(&mut bus)).is_err() {
                return false;
            }
            cpu.registers
                .set_status_register(cpu.registers.status_register() & !TRACE);
            InstructionFault::Execution(TimingFault::PendingTrace)
        }
        _ => {
            cpu.interrupt_input().set_level(7);
            cpu.interrupt_input().set_level(0);
            InstructionFault::InterruptUnqualified
        }
    };
    let before = cpu.clone();
    let memory = bus.words.clone();
    let events = bus.events.clone();
    let clock = bus.clock;
    let counts = (bus.count, bus.entry_count);
    cpu.step_physical_instruction(&mut bus) == Err(expected)
        && cpu == before
        && bus.words == memory
        && bus.events == events
        && bus.clock == clock
        && (bus.count, bus.entry_count) == counts
}

pub(super) fn pure_read() -> (u32, u32) {
    let (mut count, mut bad, mut legal) = (0, 0, 0);
    for op in OPS {
        for bits in [0, 0x40, 0x80] {
            for destination in 0..8 {
                for ea in 0..64 {
                    let word = op.top() | bits | destination << 9 | ea;
                    if !oracle::data_known(word) {
                        continue;
                    }
                    legal += 1;
                    let mode = ea >> 3;
                    let register = ea & 7;
                    let selected = bits != 0 && (2..=6).contains(&mode)
                        || bits != 0 && mode == 7 && register <= 3;
                    if selected {
                        continue;
                    }
                    for supervisor in [false, true] {
                        let mut case = Case::new(Form {
                            op,
                            size: if bits == 0 {
                                Size::Byte
                            } else if bits == 0x40 {
                                Size::Word
                            } else {
                                Size::Long
                            },
                            ea: Ea::Indirect,
                        });
                        case.r = registers(supervisor, 31);
                        count += 1;
                        bad += u32::from(!rejected(case, word, 0));
                    }
                }
            }
        }
    }
    bad += u32::from(legal != 5344 || count != 5056);
    (count, bad)
}

pub(super) fn rmw() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for op in OPS {
        for bits in [0x100, 0x140, 0x180] {
            for destination in 0..8 {
                for mode in 2..=7 {
                    for register in 0..if mode == 7 { 2 } else { 8 } {
                        for supervisor in [false, true] {
                            let word = op.top() | bits | destination << 9 | mode << 3 | register;
                            let mut case = Case::new(Form {
                                op,
                                size: Size::Word,
                                ea: Ea::Indirect,
                            });
                            case.r = registers(supervisor, 31);
                            count += 1;
                            bad += u32::from(!rejected(case, word, 0));
                        }
                    }
                }
            }
        }
    }
    bad += u32::from(count != 8064);
    (count, bad)
}

pub(super) fn preflight(gate: u8) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for kind in if gate == 3 { 3..6 } else { gate..gate + 1 } {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                count += 1;
                bad += u32::from(!rejected(case, case.opcode(), kind));
            }
        }
    }
    bad += u32::from(count != if gate == 3 { 432 } else { 144 });
    (count, bad)
}

pub(super) fn unknown() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for kind in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.register = 7;
                case.next = 0x4afc;
                count += 1;
                bad += u32::from(!run::check(case, Some((form.count() - 1, kind)), 3));
            }
        }
    }
    bad += u32::from(count != 432);
    (count, bad)
}

pub(super) fn classifier() -> (u32, u32) {
    let (mut count, mut bad, mut legal) = (0, 0, 0);
    for op in OPS {
        for bits in [0, 0x40, 0x80] {
            for destination in 0..8 {
                for ea in 0..64 {
                    let word = op.top() | bits | destination << 9 | ea;
                    legal += u32::from(oracle::data_known(word));
                    for supervisor in [false, true] {
                        let mut case = Case::new(Form {
                            op: Op::Or,
                            size: Size::Word,
                            ea: Ea::Indirect,
                        });
                        case.r = registers(supervisor, 31);
                        case.register = 0;
                        case.next = word;
                        case.value = 1;
                        case.r.d[5] = 0;
                        count += 1;
                        bad += u32::from(!run::check(case, Some((1, Failure::Boundary)), 3));
                    }
                }
            }
        }
    }
    bad += u32::from(legal != 5344 || count != 12288);
    (count, bad)
}
