use super::*;

#[test]
fn wait_report_and_total_overflow_are_atomic() {
    for (total, cycles, waits) in [(u64::MAX, 4, 1), (u64::MAX - 1, 4, 2), (8, u32::MAX, 1)] {
        let mut cpu = Cpu::new();
        cpu.cycles = total;
        let mut report = FetchedInstruction {
            pc: 0x1234,
            opcode: 0xe5,
            cycles,
        };
        let saved = cpu.clone();
        let original = report;
        assert!(!cpu.extend_bus_segment(&mut report, waits));
        assert_eq!(cpu, saved);
        assert_eq!(report, original);
    }
}

#[test]
fn maximum_wait_accounting_preserves_nonclock_state() {
    let mut cpu = Cpu::new();
    cpu.cycles = u64::MAX - u64::from(u32::MAX);
    let mut report = FetchedInstruction {
        pc: 0x1234,
        opcode: 0xe5,
        cycles: 0,
    };
    let mut expected = cpu.clone();
    assert!(cpu.extend_bus_segment(&mut report, u32::MAX));
    expected.cycles = u64::MAX;
    assert_eq!(cpu, expected);
    assert_eq!(report.cycles, u32::MAX);
    let saved = cpu.clone();
    let original = report;
    assert!(cpu.extend_bus_segment(&mut report, 0));
    assert_eq!(cpu, saved);
    assert_eq!(report, original);
}
