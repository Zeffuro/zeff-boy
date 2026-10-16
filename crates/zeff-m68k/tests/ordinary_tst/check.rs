use super::*;
use model::{Action, RESERVE, Spec};

fn run(spec: Spec) -> bool {
    let (registers, queue, expected_device, result) = model::expected(&spec);
    let mut cpu =
        PrefetchCpu::from_prefetched(spec.initial, spec.queue, CpuClocks(spec.device.clock))
            .unwrap();
    let mut bus = spec.device.clone();
    let actual = cpu.step_instruction(&mut bus);
    let mut image = PrefetchCpu::from_prefetched(
        if result.is_ok() {
            registers
        } else {
            spec.initial
        },
        queue,
        CpuClocks(expected_device.clock),
    )
    .unwrap();
    image.registers = registers;
    let expected_image = if result.is_ok() {
        format!("{image:?}")
    } else {
        format!("{image:?}").replace("frozen: false", "frozen: true")
    };
    let mut good = actual == result
        && cpu.registers == registers
        && cpu.queued_words() == Some(queue)
        && cpu.clock().0 == expected_device.clock
        && bus == expected_device
        && format!("{cpu:?}") == expected_image;
    if result.is_err() {
        let saved = cpu.clone();
        let device = bus.clone();
        let mut copied = saved.clone();
        let mut copied_bus = device.clone();
        good &= copied.step_instruction(&mut copied_bus) == Err(TimingFault::Frozen);
        good &= copied == saved && copied_bus == device;
        good &= cpu.step_instruction(&mut bus) == Err(TimingFault::Frozen);
        good &= cpu == saved && bus == device;
        good &= cpu.step(&mut bus) == Err(TimingFault::Frozen);
        good &= cpu == saved && bus == device;
        good &= cpu.prime(&mut bus) == Err(TimingFault::Frozen);
        good &= cpu == saved && bus == device;
        good &= cpu.service_pending_trace(&mut bus) == Err(TimingFault::Frozen);
        good &= cpu == saved && bus == device;
        good &= cpu.step_boundary(&mut bus) == Err(TimingFault::Frozen);
        good &= cpu == saved && bus == device;
    }
    good
}

pub(super) fn section(section: u32) -> (u32, u32) {
    if section == 6 {
        return sized_registers();
    }
    let (mut count, mut bad) = (0, 0);
    for (opcode, odd) in model::forms() {
        for bank in [0, 0x2000] {
            let flags: Vec<u16> = if section <= 1 {
                (0..32).collect()
            } else {
                vec![0, 31]
            };
            for flags in flags {
                for value in [0, 1, 0x8000_8080, u32::MAX] {
                    let base = model::seed(opcode, odd, 0x0500 | bank | flags, value);
                    let reads = base.device.final_index + 1;
                    let choices: Vec<(u64, u32, Option<usize>, Option<u32>)> = match section {
                        0 | 5 => [0, 3]
                            .into_iter()
                            .filter(|&wait| section == 0 || wait == 0)
                            .map(|wait| (0, wait, None, None))
                            .collect(),
                        1 => (0..reads)
                            .flat_map(|i| [0, 3].map(|wait| (0, wait, Some(i), None)))
                            .collect(),
                        2 => {
                            let mut elapsed = 0;
                            let mut starts = Vec::new();
                            for action in &base.actions {
                                match action {
                                    Action::Idle(n) => elapsed += u64::from(*n),
                                    Action::Read(..) => {
                                        starts.push((
                                            u64::MAX - RESERVE - elapsed + 1,
                                            0,
                                            None,
                                            None,
                                        ));
                                        elapsed += 4;
                                    }
                                    _ => {}
                                }
                            }
                            starts
                        }
                        3 => {
                            let prefix = base
                                .actions
                                .iter()
                                .map(|a| match a {
                                    Action::Idle(n) => u64::from(*n),
                                    Action::Read(_, false) => 4,
                                    _ => 0,
                                })
                                .sum::<u64>();
                            vec![(u64::MAX - RESERVE - prefix, 0, None, Some(u32::MAX))]
                        }
                        4 if matches!(opcode >> 3 & 7, 4 | 6) => {
                            vec![(u64::MAX - 1, 0, None, None)]
                        }
                        4 => vec![],
                        _ => return (0, 1),
                    };
                    for (start, wait, fail, final_wait) in choices {
                        let mut spec = model::seed(opcode, odd, 0x0500 | bank | flags, value);
                        spec.device.clock = start;
                        spec.device.wait = wait;
                        spec.device.fail = fail;
                        spec.device.final_wait = final_wait;
                        if section == 5 {
                            spec.device.mutation =
                                Some((spec.device.final_index, spec.source, 0xa55a));
                        }
                        count += 1;
                        bad += u32::from(!run(spec));
                    }
                }
            }
        }
    }
    (count, bad)
}

fn sized_registers() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for size in 0..3 {
        let values = match size {
            0 => [0xdead_0000, 0xbeef_0080],
            1 => [0xdead_0000, 0xbeef_8000],
            _ => [0x0000_8000, 0x8000_0000],
        };
        for register in 0..8 {
            for bank in [0, 0x2000] {
                for flags in [0, 31] {
                    for value in values {
                        for control in 0..5 {
                            let mut spec = model::seed(
                                0x4a00 | size << 6 | register,
                                false,
                                0x0500 | bank | flags,
                                value,
                            );
                            match control {
                                1 | 2 => {
                                    spec.device.fail = Some(0);
                                    spec.device.wait = if control == 1 { 0 } else { 3 };
                                }
                                3 => spec.device.clock = u64::MAX - RESERVE + 1,
                                4 => {
                                    spec.device.clock = u64::MAX - RESERVE;
                                    spec.device.final_wait = Some(u32::MAX);
                                }
                                _ => {}
                            }
                            count += 1;
                            bad += u32::from(!run(spec));
                        }
                    }
                }
            }
        }
    }
    (count, bad)
}
