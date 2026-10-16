use super::*;
use bulk_device::{Event, Kind, Stage};
use bulk_matrix::Count;
use bulk_model::{access, encoded, sample, status};

pub(super) fn boundaries() -> (u32, u32) {
    let mut total = Count::default();
    for base in encoded().into_iter().filter(|s| s.family == 0x46c0) {
        for mask in 0..8 {
            for irq in 0..9 {
                for upper in [0, 0x2000, 0x8000, 0xa000] {
                    for ccr in 0..32 {
                        let mut s = base;
                        s.r = status(s.r, s.r.status_register() & 0x201f | 0x0700);
                        s.level = if irq <= 6 { irq } else { 0 };
                        s.edge = false;
                        s.value(upper | mask << 8 | ccr | 0x58e0);
                        let (mut cpu, mut b) = s.seed();
                        b.word(s.anchor(), 0x4e71);
                        b.word(s.anchor().wrapping_add(2), 0x4e71);
                        b.word(s.anchor().wrapping_add(4), 0x7002);
                        let p = bulk_plan::plan(s, &b);
                        let mut e = bulk_oracle::predict(s, &b, &p);
                        let actual = cpu.step_physical_instruction(&mut b);
                        let mut good = bulk_oracle::verify(&mut cpu, &mut b, e.clone(), actual);
                        if irq >= 7 {
                            cpu.interrupt_input().set_level(7);
                            e.level = 7;
                            e.edge = true;
                            if irq == 8 {
                                cpu.interrupt_input().set_level(0);
                                e.level = 0;
                            }
                        }
                        good &= bulk_private::verify(&cpu, &e)
                            && bulk_private::interrupts(&mut cpu, e.level, e.edge);
                        if upper & 0x8000 != 0 {
                            good &= bulk_guards::rejected(
                                &mut cpu,
                                &mut b,
                                InstructionFault::TraceUnqualified,
                            );
                        } else if e.edge || e.level > mask as u8 {
                            good &= bulk_guards::rejected(
                                &mut cpu,
                                &mut b,
                                InstructionFault::InterruptUnqualified,
                            );
                        } else {
                            let start = e.clock;
                            let a = access(
                                e.r.pc.wrapping_add(4),
                                if e.r.supervisor() { 6 } else { 2 },
                                AccessKind::Prefetch,
                            );
                            let value = sample(&e.words, a.logical_address);
                            e.events.push(Event::Transfer(
                                Stage::Instruction,
                                a,
                                Direction::Read,
                                value,
                                start,
                                4,
                                Kind::Complete,
                                false,
                            ));
                            e.count += 1;
                            e.clock += 4;
                            e.r.pc = e.r.pc.wrapping_add(2);
                            e.queue = Some([e.queue.unwrap()[1], value]);
                            e.result = Ok(PhysicalInstructionStep::Instruction(TimedStep {
                                opcode: 0x4e71,
                                clocks: 4,
                                exception: None,
                            }));
                            let mut copy = cpu.clone();
                            let mut copied_device = b.clone();
                            let copied = copy.step_physical_instruction(&mut copied_device);
                            good &= bulk_oracle::verify(
                                &mut copy,
                                &mut copied_device,
                                e.clone(),
                                copied,
                            );
                            let actual = cpu.step_physical_instruction(&mut b);
                            good &= bulk_oracle::verify(&mut cpu, &mut b, e, actual);
                        }
                        total.record(good);
                    }
                }
            }
        }
    }
    total.result()
}
