use super::*;
use device::Kind;
use model::Spec;
use zeff_m68k::timing::physical::ImportedGroup0Boundary;
pub(super) fn run(index: u32, mark: fn(bool)) -> (u32, u32) {
    let mut tally = Tally::default();
    match index {
        1 => {
            let (mut processing, mut user, mut supervisor) = (0, 0, 0);
            for word in 0..=u16::MAX {
                let mut s = Spec::new(0x4a00, false);
                s.nword = word;
                let mut good = check::run(s, |b| b.report = Some((0, Kind::Boundary, 8)), mark);
                processing += u32::from(classifier::known(word));
                for bank in [false, true] {
                    let s = Spec::new(0x4a00, bank);
                    let (r, _, mut b, _, _) = s.seed();
                    b.clock = u64::MAX;
                    let selected = classifier::selected(word, bank);
                    if selected {
                        if bank { supervisor += 1 } else { user += 1 }
                    }
                    let mut e = oracle::Expected::new(r, [word, 0x4e71], &b);
                    if selected {
                        e.guard = Some(TimingFault::Frozen);
                    }
                    mark(true);
                    let mut cpu =
                        PrefetchCpu::from_prefetched(r, [word, 0x4e71], CpuClocks(u64::MAX))
                            .unwrap();
                    let actual = check::step(&mut cpu, &mut b);
                    let error = if selected {
                        InstructionFault::Entry(EntryFault::ClockOverflow)
                    } else {
                        InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                            pc: s.pc,
                            opcode: word,
                        })
                    };
                    let copy = if selected {
                        check::freeze(&mut cpu, &mut b, TimingFault::Frozen)
                    } else {
                        check::rejected(&mut cpu, &mut b, error.clone())
                    };
                    mark(false);
                    good &= actual == Err(error)
                        && copy
                        && check::private(&cpu, &e)
                        && b.events.is_empty();
                }
                tally.add(good);
            }
            tally.bad += u32::from((processing, user, supervisor) != (26599, 19634, 19687));
        }
        2 => {
            for bank in [false, true] {
                for word in 0x4a00..=0x4aff {
                    if legal(word) {
                        continue;
                    }
                    let s = Spec::new(0x4a00, bank);
                    let (r, _, mut b, _, _) = s.seed();
                    mark(true);
                    let mut cpu =
                        PrefetchCpu::from_prefetched(r, [word, 0x7001], CpuClocks(0)).unwrap();
                    let good = check::rejected(
                        &mut cpu,
                        &mut b,
                        InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                            pc: s.pc,
                            opcode: word,
                        }),
                    );
                    mark(false);
                    tally.add(good);
                }
            }
        }
        3 => {
            for s in model::encoded() {
                for route in 0..5 {
                    tally.add(preflight(s, route, mark));
                }
            }
        }
        _ => unreachable!(),
    }
    tally.result()
}
fn legal(w: u16) -> bool {
    w & 0xff00 == 0x4a00
        && w >> 6 & 3 < 3
        && (w >> 3 & 7 == 0 || (2..=6).contains(&(w >> 3 & 7)) || w >> 3 & 7 == 7 && w & 7 <= 1)
}
fn preflight(s: Spec, route: u8, mark: fn(bool)) -> bool {
    let (r, q, mut b, _, _) = s.seed();
    mark(true);
    let mut cpu = PrefetchCpu::from_prefetched(r, q, CpuClocks(0)).unwrap();
    let error = match route {
        0 => {
            cpu.registers = status(r, r.status_register() | 0x8000);
            InstructionFault::TraceUnqualified
        }
        1 => {
            check::interrupts(&mut cpu, 6, false);
            InstructionFault::InterruptUnqualified
        }
        2 => {
            check::interrupts(&mut cpu, 0, true);
            InstructionFault::InterruptUnqualified
        }
        3 => {
            let fc = if s.bank {
                FunctionCode::SupervisorProgram
            } else {
                FunctionCode::UserProgram
            };
            cpu = PrefetchCpu::from_group0_boundary(ImportedGroup0Boundary {
                registers: r,
                clock: CpuClocks(0),
                capture: Group0Capture {
                    vector: 2,
                    internal_pc: s.pc.wrapping_add(4),
                    phase_sr: r.status_register(),
                    ird: s.opcode,
                    logical_address: s.pc.wrapping_add(4),
                    function_code: fc,
                    direction: Direction::Read,
                    processing_class: ProcessingClass::InstructionOrGroup2,
                    ssw_profile: SswProfile::SelectedIrdUpperBits,
                },
            })
            .unwrap();
            InstructionFault::Execution(TimingFault::ImportedBoundaryPending)
        }
        _ => {
            b.report = Some((s.count() - 1, Kind::Boundary, 8));
            b.entry_failure = Some((0, Kind::Boundary, 8));
            let _ = check::step(&mut cpu, &mut b);
            InstructionFault::Execution(TimingFault::DoubleFaultHalted)
        }
    };
    let good = check::rejected(&mut cpu, &mut b, error.clone())
        && if let InstructionFault::Execution(f) = error {
            check::freeze(&mut cpu, &mut b, f)
        } else {
            true
        };
    mark(false);
    good
}
fn status(r: Registers, sr: u16) -> Registers {
    model::status(r, sr)
}
