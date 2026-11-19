use super::device::{Device, access};
use super::fixed::Event;
use super::model::Failure;
use super::{
    AccessKind, CpuClocks, Direction, InstructionFault, InstructionPhase, PhysicalInstructionStep,
    PrefetchCpu, TimedStep, check, matrix, oracle, seed,
};

pub(super) fn run(mark: fn(bool)) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    seed::each(|base| {
        for mask in 0..8 {
            for route in 0..4 {
                let mut c = matrix::status(base, 31, mask);
                c.next = 0x4e71;
                if c.p.extensions() == 0 {
                    c.retained = c.next;
                }
                let config = Device::seed(c);
                let origin = oracle::predict(c, &config);
                let good = check::actual(c, config, &origin, mark).is_some_and(|(mut cpu, bus)| {
                    let mut e = origin.clone();
                    e.level = [0, 3, 6, 0][route];
                    e.edge = route == 3;
                    if e.edge {
                        cpu.interrupt_input().set_level(7);
                    }
                    cpu.interrupt_input().set_level(e.level);
                    if !check::verify(&cpu, &bus, &e) {
                        return false;
                    }
                    let rejected = e.edge || e.level > mask as u8;
                    let result = if rejected {
                        Err(InstructionFault::InterruptUnqualified)
                    } else {
                        let a = access(
                            e.r.pc.wrapping_add(4),
                            e.r.supervisor(),
                            true,
                            AccessKind::Prefetch,
                            false,
                        );
                        let value = e.words.sample(a.bus_address);
                        e.events.push(Event::Transfer {
                            stage: 0,
                            phase: Some(InstructionPhase::SequentialRefill),
                            access: a,
                            direction: Direction::Read,
                            value: Some(value),
                            start: e.clock,
                            elapsed: 4,
                            kind: Failure::Complete,
                            deposited: false,
                        });
                        e.scripts.push(0, Failure::Complete, 4);
                        e.clock += 4;
                        e.count += 1;
                        e.counts[0] += 1;
                        e.r.pc = e.r.pc.wrapping_add(2);
                        e.pipeline_address = Some(e.r.pc);
                        e.queue = Some([e.queue.unwrap()[1], value]);
                        Ok(PhysicalInstructionStep::Instruction(TimedStep {
                            opcode: 0x4e71,
                            clocks: 4,
                            exception: None,
                        }))
                    };
                    let before = cpu.clone();
                    let device = bus.clone();
                    for (mut copy, mut copied_bus) in [(cpu.clone(), bus.clone()), (cpu, bus)] {
                        mark(true);
                        let actual = check::step(&mut copy, &mut copied_bus);
                        mark(false);
                        if actual != result
                            || !check::verify(&copy, &copied_bus, &e)
                            || rejected && (copy != before || copied_bus != device)
                        {
                            return false;
                        }
                    }
                    true
                });
                count += 1;
                bad += u32::from(!good);
            }
        }
    });
    (count, bad)
}

pub(super) fn unsupported(word: u16, bank: bool, mark: fn(bool)) -> bool {
    let mut c = seed::make(0x4200, bank, 0);
    c.word = word;
    let config = Device::seed(c);
    let e = oracle::initial(c, &config);
    mark(true);
    let cpu = PrefetchCpu::from_prefetched(c.r, [word, c.retained], CpuClocks(0)).unwrap();
    mark(false);
    check::verify(&cpu, &config, &e)
        && super::preflight::attempt(
            cpu,
            config,
            InstructionFault::Execution(super::TimingFault::UnsupportedInstruction {
                pc: c.r.pc,
                opcode: word,
            }),
            mark,
        )
}
