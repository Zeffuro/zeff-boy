use crate::axis;
use crate::binding;
use crate::device::Device;
use crate::literal;
use crate::oracle::{self, Expected};
use crate::seed::{self, Axis};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimingFault};

fn frozen_apis(cpu: &mut PrefetchCpu, device: &mut Device) -> bool {
    let saved_cpu = cpu.clone();
    let saved_device = device.clone();
    let mut valid = true;
    valid &= matches!(cpu.step_instruction(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= matches!(cpu.step_boundary(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= matches!(cpu.prime(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= matches!(cpu.service_pending_trace(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= matches!(cpu.step(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid
}

fn run_one(expected: &Expected) -> bool {
    let initial = &expected.initial;
    assert_eq!((initial.level, initial.edge), (0, false));
    let input = (initial.r, initial.queue, CpuClocks(initial.device.clock));
    let copied_input = input;
    let saved_device = initial.device.clone();
    let mut device = saved_device.clone();
    let mut copied_device = device.clone();
    let Some(mut cpu) = PrefetchCpu::from_prefetched(input.0, input.1, input.2) else {
        let copied_cpu =
            PrefetchCpu::from_prefetched(copied_input.0, copied_input.1, copied_input.2);
        return !expected.importable
            && copied_cpu.is_none()
            && input == copied_input
            && device == saved_device
            && copied_device == saved_device
            && device == expected.device
            && expected.device.events.len == 0
            && expected.device.scripts.len == 0;
    };
    if !expected.importable {
        return false;
    }
    let mut copied_cpu = cpu.clone();
    let mut valid = input == copied_input && cpu == copied_cpu && device == copied_device;
    valid &= cpu.registers == initial.r
        && cpu.clock().0 == initial.device.clock
        && cpu.queued_words() == Some(initial.queue);
    valid &= format!("{cpu:?}") == expected.debug(true)
        && format!("{copied_cpu:?}") == expected.debug(true);
    let actual = cpu.step_instruction(&mut device);
    let copied = copied_cpu.step_instruction(&mut copied_device);
    valid &= actual == expected.result() && copied == expected.result();
    valid &= cpu == copied_cpu && device == copied_device;
    valid &= cpu.registers == expected.r
        && cpu.clock().0 == expected.device.clock
        && cpu.queued_words() == Some(expected.queue);
    valid &= format!("{cpu:?}") == expected.debug(false)
        && format!("{copied_cpu:?}") == expected.debug(false);
    valid &= device == expected.device && copied_device == expected.device;
    if expected.error.is_some() {
        let original_frozen = frozen_apis(&mut cpu, &mut device);
        let copied_frozen = frozen_apis(&mut copied_cpu, &mut copied_device);
        valid &= original_frozen && copied_frozen && cpu == copied_cpu && device == copied_device;
    }
    valid
}

pub fn section(section: u8, log: bool) -> (usize, usize) {
    let mut count = 0;
    let mut bad = 0;
    let mut rejected = 0;
    let mut maxima = [0; 3];
    axis::each(section, |value| {
        let input = Axis::from_literal(&value);
        let expected = oracle::predict(seed::build(&input));
        rejected += usize::from(!expected.importable);
        for (target, value) in maxima.iter_mut().zip([
            expected.device.words.len,
            expected.device.events.len,
            expected.device.scripts.len,
        ]) {
            *target = (*target).max(value);
        }
        if !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary operand OCO{section:02}_{count:07} word={:04x} phase={:?} mismatch",
                    input.word, input.phase
                );
            }
        }
        count += 1;
    });
    assert_eq!(count, axis::COUNTS[usize::from(section)]);
    assert_eq!(
        rejected,
        if section == 16 {
            30
        } else if section == 25 {
            1296
        } else {
            0
        }
    );
    assert!(maxima[0] <= 10 && maxima[1] <= 5 && maxima[2] <= 5);
    (count, bad)
}

pub fn run(log: bool) -> (usize, usize) {
    let mut total = (0, 0);
    for index in 0..26 {
        let result = section(index, log);
        total.0 += result.0;
        total.1 += result.1;
    }
    assert_eq!(total.0, 8_363_048);
    total
}

pub fn source_literals(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut bad = 0;
    let mut sections = [false; 26];
    for row in &rows {
        let value = row.get("record").get("axis");
        sections[value.get("section").number() as usize] = true;
        let expected = oracle::predict(seed::build(&Axis::from_literal(value)));
        if binding::wrapper(&expected, value.clone()) != *row.get("expected") {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary operand source literal {} mismatch",
                    value.get("id").text()
                );
            }
        }
    }
    assert!(sections.into_iter().all(|covered| covered));
    assert_eq!(rows.len(), 1918);
    (rows.len(), bad)
}

pub fn literal_probes(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut bad = 0;
    let mut rejected = 0;
    for row in &rows {
        let value = row.get("record").get("axis");
        let expected = oracle::predict(seed::build(&Axis::from_literal(value)));
        rejected += usize::from(!expected.importable);
        if !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary operand bounded probe {} mismatch",
                    value.get("id").text()
                );
            }
        }
    }
    assert_eq!(rejected, 109);
    assert_eq!(rows.len(), 1918);
    (rows.len(), bad)
}

pub fn source_axes(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut matched = vec![false; rows.len()];
    let mut bad = 0;
    for section in 0..26 {
        let subset = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.get("record").get("axis").get("section").number() == u64::from(section)
            })
            .collect::<Vec<_>>();
        let mut at = 0;
        axis::each(section, |value| {
            if at < subset.len()
                && value.get("id") == subset[at].1.get("record").get("axis").get("id")
            {
                let (index, row) = subset[at];
                matched[index] = true;
                if value != *row.get("record").get("axis") {
                    bad += 1;
                    if log {
                        eprintln!(
                            "ordinary operand section {section} literal axis {index} mismatch"
                        );
                    }
                }
                at += 1;
            }
        });
        assert_eq!(at, subset.len());
    }
    assert!(matched.into_iter().all(|value| value));
    (rows.len(), bad)
}
