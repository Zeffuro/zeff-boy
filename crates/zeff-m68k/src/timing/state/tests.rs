use super::*;
use crate::SUPERVISOR;
use crate::timing::TimedOutcome;
use crate::timing::test_bus::{Bus, cpu};

fn bytes(cpu: &PrefetchCpu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer).unwrap();
    writer.into_bytes()
}

fn restored(source: &PrefetchCpu) -> PrefetchCpu {
    let encoded = bytes(source);
    let mut reader = StateReader::new(&encoded);
    let mut result = PrefetchCpu::default();
    result.read_boundary_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(result, *source);
    result
}

fn dense() -> PrefetchCpu {
    let mut registers = Registers::new();
    registers.d = std::array::from_fn(|i| (i as u32 + 1) * 0x0101_0101);
    registers.a = std::array::from_fn(|i| (i as u32 + 1) * 0x1111_1111);
    registers.usp = 0x7856_3412;
    registers.ssp = 0xfedc_ba99;
    registers.pc = 0x1234_5678;
    registers.set_status_register(0xa715);
    let mut result = PrefetchCpu::from_prefetched(
        registers,
        [0x7080, 0x4e71],
        CpuClocks(0x0123_4567_89ab_cdef),
    )
    .unwrap();
    result.pending_trace = true;
    result.interrupt_input().set_level(7);
    result.interrupt_input().set_level(6);
    result
}

#[test]
fn ordinary_boundary_codec_has_explicit_stable_bytes() {
    let source = dense();
    let golden = "01 5f 00 00 00
        01 01 01 01 02 02 02 02 03 03 03 03 04 04 04 04
        05 05 05 05 06 06 06 06 07 07 07 07 08 08 08 08
        11 11 11 11 22 22 22 22 33 33 33 33 44 44 44 44
        55 55 55 55 66 66 66 66 77 77 77 77
        12 34 56 78 99 ba dc fe 78 56 34 12 15 a7
        ef cd ab 89 67 45 23 01 01 78 56 34 12 80 70 71 4e 00 01 06 01"
        .split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(bytes(&source), golden);
    assert_eq!(bytes(&restored(&source)), golden);
    restored(&PrefetchCpu::default());
}

#[test]
fn restored_prefetch_executes_cached_opcode_after_memory_changes() {
    let mut source = cpu(SUPERVISOR);
    source.pipeline.as_mut().unwrap().words = [0x4e71, 0x7080];
    let mut bus = Bus::default();
    bus.words.insert(0x340102, 0x707f);
    source.step_boundary(&mut bus).unwrap();
    let mut target = restored(&source);
    let mut other = bus.clone();
    for _ in 0..4 {
        assert_eq!(
            source.step_boundary(&mut bus),
            target.step_boundary(&mut other)
        );
        assert_eq!(source.registers.d[0], 0xffff_ff80);
        assert_eq!(source, target);
        assert_eq!(bus, other);
    }
}

#[test]
fn restored_stop_preserves_masked_poll_then_interrupt_wakeup() {
    let mut source = cpu(0x2700);
    source.pipeline.as_mut().unwrap().words = [0x4e72, 0x2300];
    let mut bus = Bus::default();
    bus.handler(28);
    source.step_boundary(&mut bus).unwrap();
    assert!(source.stopped());
    source.interrupt_input().set_level(3);
    let mut target = restored(&source);
    let mut other = bus.clone();
    assert_eq!(
        source.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Stopped
    );
    assert_eq!(
        target.step_boundary(&mut other).unwrap(),
        TimedOutcome::Stopped
    );
    source.interrupt_input().set_level(4);
    target.interrupt_input().set_level(4);
    let outcome = source.step_boundary(&mut bus).unwrap();
    assert!(matches!(outcome, TimedOutcome::Interrupt { level: 4, .. }));
    assert_eq!(target.step_boundary(&mut other).unwrap(), outcome);
    assert_eq!(source, target);
    assert_eq!(bus, other);
}

#[test]
fn restored_traced_stop_services_trace_before_stopped_poll() {
    let mut source = cpu(0xa700);
    source.pipeline.as_mut().unwrap().words = [0x4e72, 0x2700];
    let mut bus = Bus::default();
    bus.handler(9);
    source.step_instruction(&mut bus).unwrap();
    assert!(source.stopped() && source.trace_pending());
    let mut target = restored(&source);
    let mut other = bus.clone();
    let outcome = source.step_boundary(&mut bus).unwrap();
    assert!(matches!(outcome, TimedOutcome::Trace { .. }));
    assert_eq!(target.step_boundary(&mut other).unwrap(), outcome);
    assert!(!target.stopped());
    assert_eq!(source, target);
    assert_eq!(bus, other);
}

#[test]
fn restored_pending_trace_precedes_latched_level7_after_input_falls() {
    let mut source = cpu(0xa700);
    let mut bus = Bus::default();
    bus.handler(9);
    bus.handler(31);
    source.step_instruction(&mut bus).unwrap();
    assert!(source.trace_pending());
    source.interrupt_input().set_level(7);
    source.interrupt_input().set_level(0);
    let mut target = restored(&source);
    let mut other = bus.clone();
    for phase in 0..3 {
        let outcome = source.step_boundary(&mut bus).unwrap();
        assert!(matches!(
            (phase, outcome),
            (0, TimedOutcome::Trace { .. })
                | (1, TimedOutcome::Interrupt { level: 7, .. })
                | (2, TimedOutcome::Instruction(_))
        ));
        assert_eq!(target.step_boundary(&mut other).unwrap(), outcome);
        assert_eq!(source, target);
        assert_eq!(bus, other);
    }
}

#[test]
fn restored_held_level7_does_not_recreate_consumed_edge() {
    let mut source = cpu(0x2700);
    source.interrupt_input().set_level(7);
    let mut bus = Bus::default();
    bus.handler(31);
    source.step_boundary(&mut bus).unwrap();
    let mut target = restored(&source);
    let mut other = bus.clone();
    let outcome = source.step_boundary(&mut bus).unwrap();
    assert!(matches!(outcome, TimedOutcome::Instruction(_)));
    assert_eq!(target.step_boundary(&mut other).unwrap(), outcome);
    assert_eq!(source, target);
    assert_eq!(bus, other);
}

#[test]
fn malformed_boundary_sections_leave_target_unchanged() {
    const PC: usize = 73;
    const SR: usize = 77;
    const PIPE_PRESENT: usize = 87;
    const PIPE_ADDRESS: usize = 88;
    const STOPPED: usize = 96;
    const TRACE_PENDING: usize = 97;
    const IRQ_LEVEL: usize = 98;
    const IRQ_EDGE: usize = 99;
    let encoded = bytes(&dense());
    let mut malformed = (0..encoded.len())
        .map(|length| encoded[..length].to_vec())
        .collect::<Vec<_>>();
    for (offset, value) in [
        (0, 2),
        (PC, encoded[PC] | 1),
        (SR, encoded[SR] | 0x20),
        (PIPE_PRESENT, 2),
        (PIPE_ADDRESS, encoded[PIPE_ADDRESS] ^ 2),
        (STOPPED, 1),
        (STOPPED, 2),
        (TRACE_PENDING, 2),
        (IRQ_LEVEL, 8),
        (IRQ_EDGE, 2),
    ] {
        let mut changed = encoded.clone();
        changed[offset] = value;
        malformed.push(changed);
    }
    let mut too_large = encoded;
    too_large[1..5].copy_from_slice(&u32::MAX.to_le_bytes());
    malformed.push(too_large);
    let mut trailing = bytes(&PrefetchCpu::default());
    let length = u32::from_le_bytes(trailing[1..5].try_into().unwrap()) + 1;
    trailing[1..5].copy_from_slice(&length.to_le_bytes());
    trailing.push(0);
    malformed.push(trailing);
    for changed in malformed {
        let mut target = cpu(0x2000);
        let original = target.clone();
        assert!(
            target
                .read_boundary_state(&mut StateReader::new(&changed))
                .is_err()
        );
        assert_eq!(target, original);
    }
}

#[test]
fn unsupported_boundary_encode_does_not_append_partial_state() {
    let valid = dense();
    for kind in 0..7 {
        let mut source = valid.clone();
        match kind {
            0 => source.frozen = true,
            1 => {
                source.physical_state =
                    physical::PhysicalEntryState::Critical(physical::EntryKind::ImportedGroup0)
            }
            2 => source.physical_inspection = physical::PhysicalInspection::Unavailable,
            3 => source.registers.pc |= 1,
            4 => source.pipeline.as_mut().unwrap().address ^= 2,
            5 => source.stopped = true,
            6 => {
                source.imported_boundary = Some(physical::ImportedGroup0Boundary {
                    registers: source.registers,
                    clock: source.clock,
                    capture: physical::Group0Capture {
                        vector: 2,
                        internal_pc: source.registers.pc,
                        phase_sr: source.registers.status_register(),
                        ird: 0x4e71,
                        logical_address: source.registers.pc,
                        function_code: crate::bus::FunctionCode::SupervisorProgram,
                        direction: crate::bus::Direction::Read,
                        processing_class: physical::ProcessingClass::InstructionOrGroup2,
                        ssw_profile: physical::SswProfile::SelectedIrdUpperBits,
                    },
                });
            }
            _ => unreachable!(),
        }
        let mut writer = StateWriter::new();
        writer.write_u8(0xaa);
        assert!(source.write_boundary_state(&mut writer).is_err());
        assert_eq!(writer.into_bytes(), [0xaa]);
    }
}
