use super::*;
use crate::hardware::VdcRegister;
use crate::hardware::cpu::InterruptSource;

#[test]
fn writes_retain_raw_aliases_values_and_entering_cpu_speed() {
    let mut machine = machine(&[
        0xA9, 0xA5, 0x8D, 0x10, 0x08, 0xD4, 0x8D, 0xF6, 0x0B, 0x54, 0x8D, 0x1F, 0x08,
    ]);
    machine.reset_and_begin_audio_trace(3).unwrap();
    run(&mut machine, 6);
    let trace = finish(&mut machine);
    assert_eq!(trace.end_cycle, 204);
    let expected = [
        (84, 0xE002, 0x1F_E810, 0),
        (135, 0xE006, 0x1F_EBF6, 6),
        (204, 0xE00A, 0x1F_E81F, 15),
    ];
    assert_eq!(trace.events.len(), expected.len());
    for (event, (cycle, pc, physical_address, register)) in trace.events.iter().zip(expected) {
        assert_eq!(event.cycle, cycle);
        assert_eq!(event.pc, pc);
        assert_eq!(
            event.instruction_source,
            AudioTraceSource::CartridgeRom {
                offset: u64::from(pc - 0xE000),
                bit_reversed: false
            }
        );
        assert_eq!(
            event.write,
            Huc6280TraceWrite {
                physical_address,
                register,
                value: 0xA5
            }
        );
    }
}

#[test]
fn block_transfers_capture_every_byte_at_its_bus_completion() {
    for (opcode, registers) in [(0xD3, [6, 6, 6, 6]), (0xE3, [6, 7, 6, 7])] {
        let mut image = rom(&[0xD4, opcode, 0x00, 0xE1, 0x06, 0x08, 4, 0]);
        image[0x100..0x104].copy_from_slice(&[0xFF, 0x40, 0x22, 0x19]);
        let mut machine = PceMachine::new(image).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
        machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xF8);
        machine.reset_and_begin_audio_trace(4).unwrap();
        run(&mut machine, 2);
        let trace = finish(&mut machine);
        assert_eq!(trace.end_cycle, 159);
        assert_eq!(trace.events.len(), 4);
        for (index, event) in trace.events.iter().enumerate() {
            assert_eq!(event.cycle, 87 + index as u64 * 18);
            assert_eq!(event.pc, 0xE001);
            assert_eq!(
                event.instruction_source,
                AudioTraceSource::CartridgeRom {
                    offset: 1,
                    bit_reversed: false
                }
            );
            assert_eq!(event.write.register, registers[index]);
            assert_eq!(event.write.value, [0xFF, 0x40, 0x22, 0x19][index]);
        }
    }
}

#[test]
fn block_transfers_include_waits_on_video_source_reads() {
    let mut machine = machine(&[0xD3, 0x00, 0x04, 0x06, 0x08, 2, 0]);
    machine.reset_and_begin_audio_trace(2).unwrap();
    let step = machine.step_boundary().unwrap();
    assert_eq!(step.wait_cycles(), 2);
    let trace = finish(&mut machine);
    assert_eq!(trace.end_cycle, 372);
    assert_eq!(
        trace
            .events
            .iter()
            .map(|event| event.cycle)
            .collect::<Vec<_>>(),
        [216, 300]
    );
}

#[test]
fn irq_entry_time_precedes_the_handler_writer_source() {
    let mut image = rom(&[
        0xA9, 0, 0x8D, 0, 0x0C, 0xA9, 1, 0x8D, 1, 0x0C, 0x58, 0xEA, 0x80, 0xFD,
    ]);
    image[0x100..0x105].copy_from_slice(&[0xA9, 31, 0x8D, 6, 8]);
    image[0x1FFA..0x1FFC].copy_from_slice(&0xE100_u16.to_le_bytes());
    let mut machine = PceMachine::new(image).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xF8);
    machine.reset_and_begin_audio_trace(1).unwrap();
    let mut handler_start = None;
    for _ in 0..1000 {
        let before = machine.master_ticks();
        let step = machine.step_boundary().unwrap();
        if let PceCpuAction::Interrupt(interrupt) = step.action() {
            assert_eq!(interrupt.source, InterruptSource::Timer);
            assert_eq!(step.master_ticks(), 8 * 12);
            handler_start = Some(before + 8 * 12);
            break;
        }
    }
    let handler_start = handler_start.expect("timer interrupt");
    run(&mut machine, 2);
    let trace = finish(&mut machine);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].cycle, handler_start + 7 * 12);
    assert_eq!(trace.events[0].pc, 0xE102);
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::CartridgeRom {
            offset: 0x102,
            bit_reversed: false
        }
    );
}

#[test]
fn dummy_writes_capture_only_successfully_applied_bus_writes() {
    let mut machine = machine(&[0xEA]);
    machine.reset_and_begin_audio_trace(1).unwrap();
    machine
        .step_boundary_faulting_with(|cpu, bus| {
            let mut step = cpu.step_instruction(bus)?;
            bus.dummy_write(0x1F_EAFE, 0xD0);
            step.cycles += 1;
            Ok(PceCpuAction::Instruction(step))
        })
        .unwrap();
    let trace = finish(&mut machine);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].cycle, 36);
    assert_eq!(
        trace.events[0].write,
        Huc6280TraceWrite {
            physical_address: 0x1F_EAFE,
            register: 14,
            value: 0xD0
        }
    );
}

#[test]
fn dma_contention_time_is_included_before_the_following_psg_write() {
    let mut machine = machine(&[0xD3, 3, 0, 6, 8, 1, 0]);
    machine.reset_and_begin_audio_trace(1).unwrap();
    let vdc = machine.bus.devices_mut().vdc_mut();
    for (register, value) in [
        (VdcRegister::DmaSource, 0x0100_u16),
        (VdcRegister::DmaDestination, 0x0200),
        (VdcRegister::DmaLength, 255),
    ] {
        vdc.write_port(VdcPort::SelectOrStatus, register as u8);
        vdc.write_port(VdcPort::DataLow, value as u8);
        vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
    }
    vdc.write_port(VdcPort::SelectOrStatus, VdcRegister::VramData as u8);
    let step = machine.step_boundary().unwrap();
    assert!(step.vram_contention_wait_cycles() > 0);
    assert_eq!(step.wait_cycles(), 1 + step.vram_contention_wait_cycles());
    let trace = finish(&mut machine);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(
        trace.events[0].cycle,
        u64::from(17 + step.wait_cycles()) * 12
    );
    assert_eq!(trace.end_cycle - trace.events[0].cycle, 6 * 12);
}

#[test]
fn zero_length_block_keeps_bounded_evidence_across_frame_boundaries() {
    let mut machine = machine(&[0xD3, 0, 0xE1, 1, 8, 0, 0]);
    machine.reset_and_begin_audio_trace(2).unwrap();
    let step = machine.step_boundary().unwrap();
    assert!(step.frames_published() > 1);
    assert_eq!(step.wait_cycles(), 0x800);
    let trace = machine.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 2);
    assert_eq!(trace.dropped_events, 65_534);
    assert_eq!(trace.events[0].cycle, 17 * 12);
    assert_eq!(trace.events[1].cycle, 23 * 12);
    assert_eq!(trace.end_cycle, (17 + 6 * 65_536 + 0x800) * 12);
    assert!(trace.validate_complete().is_err());
}
