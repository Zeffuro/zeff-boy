use crate::cases;
use crate::device::Device;
use crate::literal::{self, Value};
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

fn run_one(row: &Value) -> bool {
    let initial = row.get("initial");
    let initial_cpu = initial.get("CPU");
    let expected = row.get("expected");
    let cpu_tree = expected.get("CPU");
    let registers = cases::registers(initial_cpu.get("registers"));
    let queue = cases::queue(initial_cpu.get("queue"));
    let mut device = cases::initial_device(initial);
    let mut cpu = PrefetchCpu::from_prefetched(
        registers,
        queue,
        CpuClocks(initial_cpu.get("clock_u64").number()),
    )
    .unwrap();
    let mut copied_cpu = cpu.clone();
    let mut copied_device = device.clone();
    let initial_debug = cases::private_debug(initial_cpu);
    let expected_debug = cases::private_debug(cpu_tree);
    let expected_device = cases::expected_device(&device, expected.get("device"));
    let expected_result = cases::outcome(expected.get("typed_result"));
    let mut valid =
        format!("{cpu:?}") == initial_debug && format!("{copied_cpu:?}") == initial_debug;
    valid &= cpu.registers == registers
        && cpu.queued_words() == Some(queue)
        && cpu == copied_cpu
        && device == copied_device;
    let actual_result = cpu.step_instruction(&mut device);
    let copied_result = copied_cpu.step_instruction(&mut copied_device);
    valid &= actual_result == expected_result && copied_result == expected_result;
    valid &= cpu == copied_cpu && device == copied_device;
    valid &= cpu.registers == cases::registers(cpu_tree.get("registers"));
    valid &= cpu.clock().0 == cpu_tree.get("clock_u64").number();
    valid &= cpu.queued_words() == Some(cases::queue(cpu_tree.get("queue")));
    valid &= format!("{cpu:?}") == expected_debug && format!("{copied_cpu:?}") == expected_debug;
    valid &= device == expected_device && copied_device == expected_device;
    if cpu_tree.get("private_cpu").get("frozen").boolean() {
        valid &= frozen_apis(&mut cpu, &mut device);
        valid &= frozen_apis(&mut copied_cpu, &mut copied_device);
        valid &= cpu == copied_cpu && device == copied_device;
    }
    valid
}

pub fn run(log_failures: bool) -> (usize, usize) {
    let rows = literal::records();
    let mut bad = 0;
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.get("axis").get("id").text(), format!("OCN{index:04}"));
        if !run_one(row) {
            bad += 1;
            if log_failures {
                eprintln!(
                    "ordinary FIRST mismatch {}",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    (rows.len(), bad)
}

pub fn anchors() -> (usize, usize) {
    let rows = literal::records();
    let anchors = literal::anchors();
    let mut bad = 0;
    for anchor in &anchors {
        let id = anchor.get("axis").get("id").text();
        let row = rows
            .iter()
            .find(|row| row.get("axis").get("id").text() == id)
            .unwrap();
        let mut valid =
            anchor.get("axis") == row.get("axis") && anchor.get("expected") == row.get("expected");
        valid &= anchor.get("initial").get("CPU") == row.get("initial").get("CPU");
        valid &= anchor.get("initial").get("word_RAM") == row.get("initial").get("word_RAM");
        valid &= anchor
            .get("copy_contract")
            .get("owned_initial_CPU_device_copy")
            .boolean();
        valid &= anchor
            .get("copy_contract")
            .get("original_and_copy_exact")
            .boolean();
        let frozen = anchor
            .get("expected")
            .get("CPU")
            .get("private_cpu")
            .get("frozen")
            .boolean();
        assert_eq!(
            anchor.get("terminal_guards").list().len(),
            if frozen { 5 } else { 0 }
        );
        for guard in anchor.get("terminal_guards").list() {
            valid &= guard
                .get("typed_result")
                .get("Err")
                .get("TimingFault::Frozen")
                .null();
            valid &= guard.get("CPU_device_unchanged").boolean();
        }
        bad += usize::from(!valid);
    }
    (anchors.len(), bad)
}
