use super::*;

pub(super) fn run(spec: Spec, path: Path) {
    let expected = model::predict(spec, path);
    let (registers, queue, ram, start) = spec.input(path);
    let mut cpu = PrefetchCpu::from_prefetched(registers, queue, CpuClocks(start)).unwrap();
    let mut device = Device::new(ram, start, path, spec.form == 8);
    let result = black_box(cpu.step_physical_instruction(&mut device));
    assert_eq!(result, expected.result);
    assert_eq!(cpu.registers, expected.registers);
    assert_eq!(cpu.queued_words(), expected.queue);
    assert_eq!(cpu.clock().0, expected.device.clock);
    assert_eq!(device, expected.device);
    assert_eq!(cpu.physical_entry_state(), expected.state);
    assert_eq!(cpu.physical_inspection(), expected.inspection);
    assert!(!cpu.trace_pending());
    assert!(!cpu.stopped());
    assert_eq!(*cpu.interrupt_input(), Default::default());
    if matches!(result, Ok(PhysicalInstructionStep::Instruction(_))) {
        let image = PrefetchCpu::from_prefetched(
            expected.registers,
            expected.queue.unwrap(),
            CpuClocks(expected.device.clock),
        )
        .unwrap();
        assert_eq!(cpu, image);
    }
    if result.is_err() {
        let saved = cpu.clone();
        let snapshot = device.clone();
        let guard = if expected.inspection == PhysicalInspection::Unavailable {
            TimingFault::PhysicalFaultUnqualified
        } else {
            TimingFault::Frozen
        };
        assert_eq!(
            cpu.step_physical_instruction(&mut device),
            Err(InstructionFault::Execution(guard.clone()))
        );
        assert_eq!(cpu, saved);
        assert_eq!(device, snapshot);
        assert_eq!(cpu.step_instruction(&mut device), Err(guard.clone()));
        assert_eq!(cpu, saved);
        assert_eq!(device, snapshot);
        assert_eq!(cpu.step_boundary(&mut device), Err(guard.clone()));
        assert_eq!(cpu, saved);
        assert_eq!(device, snapshot);
        assert_eq!(cpu.prime(&mut device), Err(guard.clone()));
        assert_eq!(cpu, saved);
        assert_eq!(device, snapshot);
        assert_eq!(cpu.service_pending_trace(&mut device), Err(guard.clone()));
        assert_eq!(cpu, saved);
        assert_eq!(device, snapshot);
        let mut copied = saved.clone();
        let mut copied_device = snapshot.clone();
        assert_eq!(
            copied.step_physical_instruction(&mut copied_device),
            Err(InstructionFault::Execution(guard))
        );
        assert_eq!(copied, saved);
        assert_eq!(copied_device, snapshot);
        assert_eq!(cpu, saved);
        assert_eq!(device, snapshot);
    }
}
