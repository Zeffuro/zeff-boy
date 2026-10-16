use super::*;
#[path = "boundary_device.rs"]
mod device;
#[path = "boundary_model.rs"]
mod model;
#[path = "boundary_schedule.rs"]
mod schedule;
use model::Action;
use zeff_m68k::timing::TimedOutcome;

fn report(count: &mut u32, bad: &mut u32, good: bool, opcode: u16, route: u32) {
    *count += 1;
    *bad += u32::from(!good);
    if !good && *bad < 4 {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("ordinary NEG/NEGX boundary opcode={opcode:04x} route={route} failed");
    }
    let _ = (opcode, route);
}

pub(super) fn section21() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for family in [0x4000, 0x4400] {
        for low in 0..256 {
            let opcode = family | low;
            let mode = (opcode >> 3) & 7;
            let reg = opcode & 7;
            let size = (opcode >> 6) & 3;
            let unary = size < 3 && (mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg < 2);
            let status = family == 0x4400
                && size == 3
                && (mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg < 5);
            let from_sr = family == 0x4000
                && size == 3
                && (mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg < 2);
            if from_sr {
                for supervisor in [false, true] {
                    let s = schedule::from_sr(mode, reg as usize, supervisor);
                    let mut bus = s.bus.clone();
                    bus.waits = [3; 16];
                    let e = model::predict(s.r, s.queue, &bus, &s.actions, None, true);
                    let mut cpu =
                        PrefetchCpu::from_prefetched(s.r, s.queue, CpuClocks(bus.clock)).unwrap();
                    let result = cpu.step_instruction(&mut bus);
                    let good = result == e.result && model::matches(&mut cpu, &bus, &e, 0, false);
                    report(&mut count, &mut bad, good, opcode, u32::from(supervisor));
                }
                continue;
            }
            if unary || status {
                continue;
            }
            for supervisor in [false, true] {
                let r = model::registers(supervisor, false);
                let mut cpu =
                    PrefetchCpu::from_prefetched(r, [opcode, 0x8000], CpuClocks(11)).unwrap();
                let mut bus = device::Device::new(11);
                bus.put(r.pc, opcode);
                bus.put(r.pc + 2, 0x8000);
                let c = cpu.clone();
                let b = bus.clone();
                let result = cpu.step_instruction(&mut bus);
                let good = result == Err(TimingFault::UnsupportedInstruction { pc: r.pc, opcode })
                    && cpu == c
                    && bus == b;
                report(&mut count, &mut bad, good, opcode, u32::from(supervisor));
            }
        }
    }
    (count, bad)
}

pub(super) fn section24() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mode in [0, 2, 3, 4, 5, 6, 7] {
        for reg in 0..if mode == 7 { 5 } else { 8 } {
            for supervisor in [false, true] {
                let s = schedule::status(mode, reg, supervisor);
                for route in 0..3 {
                    let mut bus = s.bus.clone();
                    bus.waits = [3; 16];
                    bus.fail = match route {
                        1 => Some(0),
                        2 => Some(model::transfers(&s.actions) - 1),
                        _ => None,
                    };
                    let e = model::predict(s.r, s.queue, &bus, &s.actions, None, true);
                    let mut cpu =
                        PrefetchCpu::from_prefetched(s.r, s.queue, CpuClocks(bus.clock)).unwrap();
                    let result = cpu.step_instruction(&mut bus);
                    let mut good =
                        result == e.result && model::matches(&mut cpu, &bus, &e, 0, false);
                    if route > 0 {
                        good &= model::frozen(&mut cpu, &mut bus);
                    }
                    report(&mut count, &mut bad, good, s.queue[0], route);
                }
            }
        }
    }
    (count, bad)
}

fn block_f(s: &schedule::Schedule, bus: &mut device::Device) {
    let mut transfers = 0;
    let mut nominal = 0;
    for a in &s.actions {
        match a {
            Action::Idle(clocks) => nominal += u64::from(*clocks),
            Action::Read(_) | Action::Write(_, _) | Action::Ack(_) => {
                if transfers == s.f {
                    break;
                }
                nominal += 4;
                transfers += 1;
            }
            _ => {}
        }
    }
    bus.waits[s.f - 1] = u32::MAX;
    bus.clock = u64::MAX - (model::B - 1) - nominal - u64::from(u32::MAX);
}

fn next_boundary(
    cpu: &mut PrefetchCpu,
    bus: &mut device::Device,
    e: &model::Expected,
    trace: bool,
) -> bool {
    let mut good = cpu.interrupt_input().set_level(7);
    if trace {
        let c = cpu.clone();
        let b = bus.clone();
        good &=
            cpu.step_instruction(bus) == Err(TimingFault::PendingTrace) && *cpu == c && *bus == b;
    }
    let actions = model::entry(e.registers, if trace { 9 } else { 31 }, !trace);
    let mut expected = model::predict(e.registers, e.queue, &e.device, &actions, None, false);
    expected.device.waits = bus.waits;
    let result = cpu.step_boundary(bus);
    good &= result
        == if trace {
            Ok(TimedOutcome::Trace { clocks: 34 })
        } else {
            Ok(TimedOutcome::Interrupt {
                level: 7,
                vector: 31,
                clocks: 44,
            })
        };
    good &= model::matches(cpu, bus, &expected, 7, trace);
    if !trace {
        let r = expected.registers;
        let actions = [
            Action::Read(model::access(
                r.pc + 4,
                FunctionCode::SupervisorProgram,
                AccessKind::Prefetch,
            )),
            Action::Commit(r.pc + 2, [0x7201, 0x7402]),
        ];
        let again = model::predict(r, expected.queue, &expected.device, &actions, None, true);
        good &= cpu.step_boundary(bus) == again.result.clone().map(TimedOutcome::Instruction);
        good &= model::matches(cpu, bus, &again, 7, false);
    }
    good
}

pub(super) fn section22() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for family in [0x4000, 0x4400] {
        for size in [2, 4] {
            for mode in 2..=7 {
                for reg in 0..if mode == 7 { 2 } else { 8 } {
                    for supervisor in [false, true] {
                        for route in 0..4 {
                            let s =
                                schedule::ordinary(family, size, mode, reg, supervisor, route != 1);
                            let mut bus = s.bus.clone();
                            if route == 2 {
                                bus.fail = Some(s.f);
                            }
                            if route == 3 {
                                block_f(&s, &mut bus);
                            }
                            let e = model::predict(s.r, s.queue, &bus, &s.actions, None, true);
                            let mut cpu =
                                PrefetchCpu::from_prefetched(s.r, s.queue, CpuClocks(bus.clock))
                                    .unwrap();
                            let result = cpu.step_instruction(&mut bus);
                            let mut good =
                                result == e.result && model::matches(&mut cpu, &bus, &e, 0, false);
                            if route < 2 {
                                good &= next_boundary(&mut cpu, &mut bus, &e, route == 0);
                            } else {
                                good &= !cpu.trace_pending() && model::frozen(&mut cpu, &mut bus);
                            }
                            report(&mut count, &mut bad, good, s.queue[0], route);
                        }
                    }
                }
            }
        }
    }
    (count, bad)
}
