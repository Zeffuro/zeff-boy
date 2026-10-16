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
    let device = bus.clone();
    let memory = bus.words.clone();
    let events = bus.events.clone();
    let clock = bus.clock;
    let counts = (bus.count, bus.entry_count);
    cpu.step_physical_instruction(&mut bus) == Err(expected)
        && cpu == before
        && bus == device
        && bus.words == memory
        && bus.events == events
        && bus.clock == clock
        && (bus.count, bus.entry_count) == counts
}

pub(super) fn processing_only() -> (u32, u32) {
    let (mut count, mut bad, mut legal) = (0, 0, 0);
    for word in 0x0c00..=0x0cff {
        if !oracle::cmpi_known(word) {
            continue;
        }
        legal += 1;
        let mode = word >> 3 & 7;
        if word >> 6 & 3 != 0 && mode != 0 {
            continue;
        }
        for supervisor in [false, true] {
            let mut c = Case::new(Form {
                size: if word >> 6 & 3 == 0 {
                    Size::Byte
                } else {
                    Size::Word
                },
                ea: Ea::Indirect,
            });
            c.r = registers(supervisor, 31);
            count += 1;
            bad += u32::from(!remainder::migrated(word, c));
        }
    }
    (count, bad + u32::from(legal != 150 || count != 132))
}
pub(super) fn illegal() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for word in 0x0c00..=0x0cff {
        if oracle::cmpi_known(word) {
            continue;
        }
        for supervisor in [false, true] {
            let mut c = Case::new(Form {
                size: Size::Word,
                ea: Ea::Indirect,
            });
            c.r = registers(supervisor, 31);
            count += 1;
            bad += u32::from(!rejected(c, word, 0));
        }
    }
    (count, bad + u32::from(count != 212))
}
pub(super) fn preflight(gate: u8) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for kind in if gate == 3 { 3..6 } else { gate..gate + 1 } {
                let mut c = Case::new(form);
                c.r = registers(supervisor, 31);
                count += 1;
                bad += u32::from(!rejected(c, c.opcode(), kind));
            }
        }
    }
    (
        count,
        bad + u32::from(count != if gate == 3 { 84 } else { 28 }),
    )
}
pub(super) fn retained() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for next in [0x0c24, 0x4afc] {
                for kind in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    let mut c = Case::new(form);
                    c.r = registers(supervisor, 31);
                    c.register = 7;
                    c.next = next;
                    count += 1;
                    bad += u32::from(!run::check(c, Some((form.count() - 1, kind)), 3));
                }
            }
        }
    }
    (count, bad + u32::from(count != 168))
}
fn classification(word: u16, supervisor: bool) -> bool {
    let mut c = Case::new(Form {
        size: Size::Word,
        ea: Ea::Indirect,
    });
    c.r = registers(supervisor, 0x071a);
    let (_, mut bus) = c.seed();
    bus.elapsed[0] = 7;
    bus.failure = Some((0, Failure::Boundary));
    let mut cpu = PrefetchCpu::from_prefetched(c.r, [0x4e71, word], CpuClocks(c.start)).unwrap();
    let address = c.r.pc.wrapping_add(4);
    let t = oracle::Transfer {
        access: oracle::access(address, supervisor, true, AccessKind::Prefetch),
        phase: InstructionPhase::SequentialRefill,
        registers: c.r,
        internal_pc: address,
        ird: word,
    };
    let result = cpu.step_physical_instruction(&mut bus);
    if oracle::cmpi_known(word) {
        let p = entry::predict(c, t, 2, c.start + 7, c.seed().1.words, (None, None));
        let mut events = vec![Event::Read(t.access, c.start)];
        events.extend(p.events);
        result
            == Ok(PhysicalInstructionStep::Group0 {
                opcode: 0x4e71,
                capture: oracle::capture(t, 2),
                clocks: 57,
            })
            && cpu.registers == p.registers
            && cpu.queued_words() == p.queue
            && bus.events == events
            && bus.words == p.words
            && bus.count == 1
            && bus.entry_count == 11
            && cpu.physical_entry_state() == p.state
            && cpu.physical_inspection() == p.inspection
            && bus.clock == c.start + 57
            && cpu.clock().0 == bus.clock
    } else {
        let cause = CriticalFault::BusAbort {
            access: t.access,
            direction: Direction::Read,
        };
        result
            == Err(InstructionFault::CaptureUnavailable {
                phase: t.phase,
                cause,
            })
            && cpu.registers == c.r
            && cpu.queued_words().is_none()
            && bus.words == c.seed().1.words
            && bus.events == vec![Event::Read(t.access, c.start)]
            && bus.count == 1
            && bus.entry_count == 0
            && bus.clock == c.start + 7
            && cpu.clock().0 == bus.clock
            && cpu.physical_entry_state() == PhysicalEntryState::FaultUnqualified { cause }
            && cpu.physical_inspection() == PhysicalInspection::Unavailable
            && run::frozen(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified)
    }
}
pub(super) fn classifier() -> (u32, u32) {
    let (mut count, mut bad, mut legal) = (0, 0, 0);
    for word in 0x0c00..=0x0cff {
        legal += u32::from(oracle::cmpi_known(word));
        for supervisor in [false, true] {
            count += 1;
            bad += u32::from(!classification(word, supervisor));
        }
    }
    (count, bad + u32::from(legal != 150 || count != 512))
}
