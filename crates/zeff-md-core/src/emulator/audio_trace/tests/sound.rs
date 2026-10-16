use super::*;

#[test]
fn actual_main_upload_and_sound_execution_emit_only_committed_chip_writes() {
    let sound: [u8; 16] = [
        0x3e, 0x85, 0x32, 0x11, 0x7f, 0x3e, 0x2a, 0x32, 0x00, 0x40, 0x3e, 0xc0, 0x32, 0x01, 0x40,
        0x76,
    ];
    let mut program = Vec::new();
    move_immediate(&mut program, true, 0x100, 0xa11100);
    move_immediate(&mut program, true, 0x100, 0xa11200);
    for (index, value) in sound.into_iter().enumerate() {
        move_immediate(
            &mut program,
            false,
            u16::from(value),
            0xa00000 + index as u32,
        );
    }
    move_immediate(&mut program, true, 0, 0xa11100);
    program.push(0x60fe);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut off = fixture(region, true, &program);
        let mut on = off.clone();
        on.reset_and_begin_audio_trace(8).unwrap();
        for _ in 0..300 {
            assert_eq!(off.step_instruction(), on.step_instruction());
        }
        same_hardware(&mut off, &mut on);
        let capture = on.finish_audio_trace().unwrap();
        capture.trace.validate_complete().unwrap();
        assert_eq!(capture.trace.events.len(), 3);
        let values: Vec<_> = capture
            .trace
            .events
            .iter()
            .map(|event| {
                assert_eq!(event.cycle % 15, 0);
                assert_eq!(event.instruction_source, AudioTraceSource::Unknown);
                match event.write {
                    MdTraceWrite::Psg { origin, value } => (origin, 4, value),
                    MdTraceWrite::Opn2Port {
                        origin,
                        port,
                        value,
                    } => (origin, port, value),
                    _ => panic!("unexpected FM reset"),
                }
            })
            .collect();
        assert_eq!(
            values
                .iter()
                .map(|(_, port, value)| (*port, *value))
                .collect::<Vec<_>>(),
            [(4, 0x85), (0, 0x2a), (1, 0xc0)]
        );
        for ((origin, _, _), pc) in values.into_iter().zip([5, 10, 15]) {
            assert_eq!(origin.writer, MdTraceWriter::Z80);
            assert_eq!(origin.boundary_pc, pc);
            assert_eq!(origin.source, MdTraceSource::SoundRam { offset: pc as u16 });
        }
        step(&mut off, 300);
        step(&mut on, 300);
        same_hardware(&mut off, &mut on);
    }
}

#[test]
fn banked_rom_execution_retains_effective_input_mapping_at_segment_entry() {
    let sound = [
        0x3e, 0x85, 0x32, 0x11, 0x7f, 0x3e, 0x2a, 0x32, 0, 0x40, 0x3e, 0xc0, 0x32, 1, 0x40, 0x76,
    ];
    let mut program = Vec::new();
    move_immediate(&mut program, true, 0x100, 0xa11100);
    move_immediate(&mut program, true, 0x100, 0xa11200);
    for (index, value) in [0xc3_u8, 0x00, 0x90].into_iter().enumerate() {
        move_immediate(
            &mut program,
            false,
            u16::from(value),
            0xa00000 + index as u32,
        );
    }
    move_immediate(&mut program, true, 0, 0xa11100);
    program.push(0x60fe);
    let mut rom = fixture(Region::NtscUs, true, &program).rom_bytes().to_vec();
    rom[0x1000..0x1010].copy_from_slice(&sound);
    let mut on = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    let mut off = on.clone();
    on.reset_and_begin_audio_trace(8).unwrap();
    for _ in 0..300 {
        assert_eq!(off.step_instruction(), on.step_instruction());
    }
    same_hardware(&mut off, &mut on);
    let trace = on.finish_audio_trace().unwrap().trace;
    trace.validate_complete().unwrap();
    assert_eq!(trace.events.len(), 3);
    for (event, pc) in trace.events.iter().zip([0x9005, 0x900a, 0x900f]) {
        assert_eq!(event.pc, pc);
        let origin = match event.write {
            MdTraceWrite::Psg { origin, .. } | MdTraceWrite::Opn2Port { origin, .. } => origin,
            _ => unreachable!(),
        };
        assert_eq!(
            origin,
            MdTraceOrigin {
                writer: MdTraceWriter::Z80,
                boundary_pc: pc,
                source: MdTraceSource::EffectiveRom {
                    offset: pc - 0x8000
                }
            }
        );
    }
    step(&mut off, 300);
    step(&mut on, 300);
    same_hardware(&mut off, &mut on);
}
