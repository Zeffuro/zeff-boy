use zeff_emu_common::audio_trace::{AudioTraceEvent, AudioTraceSource, NesTraceWrite};
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, RegisterDelta, TraceExecMode, TraceWrite, TraceWriteKind,
    TraceWriteWidth,
};

use super::*;

fn source() -> Vec<u8> {
    let mut source = vec![0; 16 + 0x4000];
    source[..6].copy_from_slice(&[b'N', b'E', b'S', 0x1a, 1, 0]);
    source
}

fn offset(pc: u16) -> usize {
    16 + (usize::from(pc) - 0x8000) % 0x4000
}

fn write_bytes(source: &mut [u8], pc: u16, bytes: &[u8]) {
    for (delta, byte) in bytes.iter().copied().enumerate() {
        source[offset(pc + delta as u16)] = byte;
    }
}

fn state(pc: u16, sp: u8, cycle: u64) -> State {
    State {
        pc,
        a: 0x3c,
        x: 2,
        y: 3,
        sp,
        p: 0x24,
        cycle,
    }
}

fn record(_source: &[u8], before: State, after: State, bytes: &[u8]) -> InstructionTraceRecord {
    let mut record = InstructionTraceRecord::new(
        TraceExecMode::Mos6502,
        u32::from(before.pc),
        Some((offset(before.pc) - 16) as u64),
        0,
        before.cycle,
        bytes,
    );
    for (register, before, after) in [
        (0, u32::from(before.a), u32::from(after.a)),
        (1, u32::from(before.x), u32::from(after.x)),
        (2, u32::from(before.y), u32::from(after.y)),
        (3, u32::from(before.sp), u32::from(after.sp)),
        (4, u32::from(before.p), u32::from(after.p)),
        (5, u32::from(before.pc), u32::from(after.pc)),
    ] {
        if before != after {
            assert!(record.push_register_delta(RegisterDelta {
                register,
                value: after
            }));
        }
    }
    record
}

fn stack_write(address: u16, value: u8) -> TraceWrite {
    TraceWrite {
        address: u32::from(address),
        old_value: 0,
        new_value: u32::from(value),
        width: TraceWriteWidth::Byte,
        kind: TraceWriteKind::Memory,
    }
}

fn push_writes(sp: u8, values: &[u8]) -> Vec<TraceWrite> {
    values
        .iter()
        .copied()
        .enumerate()
        .map(|(index, value)| stack_write(0x0100 | u16::from(sp.wrapping_sub(index as u8)), value))
        .collect()
}

fn writer(
    _source: &[u8],
    pc: u16,
    address: u16,
    value: u8,
    cycle: u64,
) -> AudioTraceEvent<NesTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: u32::from(pc),
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: offset(pc) as u64,
            bit_reversed: false,
        },
        write: NesTraceWrite::Register {
            address,
            value,
            odd_cycle: false,
        },
    }
}

fn add_writes(record: &mut InstructionTraceRecord, writes: &[TraceWrite]) {
    for write in writes {
        assert!(record.push_write(*write));
    }
}

#[test]
fn records_nested_calls_and_strict_returns_for_a_sound_write() {
    let mut source = source();
    write_bytes(&mut source, 0x8000, &[0x20, 0x10, 0x80]);
    write_bytes(&mut source, 0x8010, &[0x20, 0x20, 0x80]);
    write_bytes(&mut source, 0x8020, &[0x8d, 0x00, 0x40]);
    write_bytes(&mut source, 0x8023, &[0x60]);
    write_bytes(&mut source, 0x8013, &[0x60]);
    let mut recorder = Recorder::new(&source).unwrap();

    let first = state(0x8000, 0xfd, 7);
    let second = state(0x8010, 0xfb, 13);
    let mut call = record(&source, first, second, &[0x20, 0x10, 0x80]);
    add_writes(&mut call, &push_writes(0xfd, &[0x80, 0x02]));
    recorder.step(first, second, Some(&call), &[]).unwrap();

    let third = state(0x8020, 0xf9, 19);
    let mut nested = record(&source, second, third, &[0x20, 0x20, 0x80]);
    add_writes(&mut nested, &push_writes(0xfb, &[0x80, 0x12]));
    recorder.step(second, third, Some(&nested), &[]).unwrap();

    let fourth = state(0x8023, 0xf9, 23);
    let mut store = record(&source, third, fourth, &[0x8d, 0x00, 0x40]);
    add_writes(&mut store, &[stack_write(0x4000, third.a)]);
    recorder
        .step(
            third,
            fourth,
            Some(&store),
            &[(9, writer(&source, 0x8020, 0x4000, third.a, 12))],
        )
        .unwrap();

    let fifth = state(0x8013, 0xfb, 29);
    let rts = record(&source, fourth, fifth, &[0x60]);
    recorder.step(fourth, fifth, Some(&rts), &[]).unwrap();
    let sixth = state(0x8003, 0xfd, 35);
    let outer_rts = record(&source, fifth, sixth, &[0x60]);
    recorder.step(fifth, sixth, Some(&outer_rts), &[]).unwrap();

    let result = recorder.finish();
    let path = &result["observations"][0]["call_path"];
    assert_eq!(result["observed_writes"], 1);
    assert_eq!(path.as_array().unwrap().len(), 2);
    assert_eq!(path[0]["kind"], "call");
    assert_eq!(path[0]["return_pc"], 0x8003);
    assert_eq!(path[0]["entry"]["sp"], 0xfb);
    assert_eq!(path[1]["return_pc"], 0x8013);
}

#[test]
fn interrupt_at_a_peeked_jsr_is_not_recorded_as_a_call() {
    let mut source = source();
    write_bytes(&mut source, 0x8000, &[0x20, 0x34, 0x12]);
    write_bytes(&mut source, 0x9000, &[0x8d, 0x00, 0x40]);
    let mut recorder = Recorder::new(&source).unwrap();
    let before = state(0x8000, 0xfd, 7);
    let after = state(0x9000, 0xfa, 14);
    let mut interrupt = record(&source, before, after, &[]);
    interrupt.event = Some(DebugEvent::Interrupt);
    add_writes(&mut interrupt, &push_writes(0xfd, &[0x80, 0x00, 0x24]));
    recorder.step(before, after, Some(&interrupt), &[]).unwrap();

    let store_after = state(0x9003, 0xfa, 18);
    let mut store = record(&source, after, store_after, &[0x8d, 0x00, 0x40]);
    add_writes(&mut store, &[stack_write(0x4000, after.a)]);
    recorder
        .step(
            after,
            store_after,
            Some(&store),
            &[(3, writer(&source, 0x9000, 0x4000, after.a, 7))],
        )
        .unwrap();
    let result = recorder.finish();
    let frame = &result["observations"][0]["call_path"][0];
    assert_eq!(frame["kind"], "interrupt");
    assert_eq!(frame["trigger"], "external");
    assert!(frame["call_pc"].is_null());
    assert_eq!(frame["return_pc"], 0x8000);
}

#[test]
fn brk_and_rti_keep_a_separate_interrupt_frame() {
    let mut source = source();
    write_bytes(&mut source, 0x8000, &[0x00, 0xea]);
    write_bytes(&mut source, 0x9000, &[0x8d, 0x00, 0x40]);
    write_bytes(&mut source, 0x9003, &[0x40]);
    let mut recorder = Recorder::new(&source).unwrap();
    let before = state(0x8000, 0xfd, 7);
    let handler = state(0x9000, 0xfa, 14);
    let mut brk = record(&source, before, handler, &[0x00, 0xea]);
    add_writes(&mut brk, &push_writes(0xfd, &[0x80, 0x02, 0x34]));
    recorder.step(before, handler, Some(&brk), &[]).unwrap();
    let store_after = state(0x9003, 0xfa, 18);
    let mut store = record(&source, handler, store_after, &[0x8d, 0x00, 0x40]);
    add_writes(&mut store, &[stack_write(0x4000, handler.a)]);
    recorder
        .step(
            handler,
            store_after,
            Some(&store),
            &[(2, writer(&source, 0x9000, 0x4000, handler.a, 7))],
        )
        .unwrap();
    let done = state(0x8002, 0xfd, 24);
    let rti = record(&source, store_after, done, &[0x40]);
    recorder.step(store_after, done, Some(&rti), &[]).unwrap();
    let result = recorder.finish();
    assert_eq!(result["observations"][0]["call_path"][0]["trigger"], "brk");
    assert_eq!(
        result["observations"][0]["call_path"][0]["return_pc"],
        0x8002
    );
}

#[test]
fn rejects_mismatched_returns_txs_and_consumed_return_slots() {
    let mut source = source();
    write_bytes(&mut source, 0x8000, &[0x20, 0x10, 0x80]);
    write_bytes(&mut source, 0x8010, &[0x60]);
    let mut recorder = Recorder::new(&source).unwrap();
    let before = state(0x8000, 0xfd, 7);
    let callee = state(0x8010, 0xfb, 13);
    let mut call = record(&source, before, callee, &[0x20, 0x10, 0x80]);
    add_writes(&mut call, &push_writes(0xfd, &[0x80, 0x02]));
    recorder.step(before, callee, Some(&call), &[]).unwrap();
    let wrong = state(0x8004, 0xfd, 19);
    let rts = record(&source, callee, wrong, &[0x60]);
    assert!(recorder.step(callee, wrong, Some(&rts), &[]).is_err());

    write_bytes(&mut source, 0x8010, &[0x9a]);
    let mut recorder = Recorder::new(&source).unwrap();
    let mut call = record(&source, before, callee, &[0x20, 0x10, 0x80]);
    add_writes(&mut call, &push_writes(0xfd, &[0x80, 0x02]));
    recorder.step(before, callee, Some(&call), &[]).unwrap();
    let txs_after = state(0x8011, 0x10, 15);
    let txs = record(&source, callee, txs_after, &[0x9a]);
    assert!(recorder.step(callee, txs_after, Some(&txs), &[]).is_err());

    write_bytes(&mut source, 0x8010, &[0x68]);
    let mut recorder = Recorder::new(&source).unwrap();
    let mut call = record(&source, before, callee, &[0x20, 0x10, 0x80]);
    add_writes(&mut call, &push_writes(0xfd, &[0x80, 0x02]));
    recorder.step(before, callee, Some(&call), &[]).unwrap();
    let pla_after = state(0x8011, 0xfc, 17);
    let pla = record(&source, callee, pla_after, &[0x68]);
    assert!(recorder.step(callee, pla_after, Some(&pla), &[]).is_err());

    write_bytes(&mut source, 0x8010, &[0x8d, 0xfd, 0x09]);
    let mut recorder = Recorder::new(&source).unwrap();
    let mut call = record(&source, before, callee, &[0x20, 0x10, 0x80]);
    add_writes(&mut call, &push_writes(0xfd, &[0x80, 0x02]));
    recorder.step(before, callee, Some(&call), &[]).unwrap();
    let mirrored_after = state(0x8013, 0xfb, 17);
    let mut overwrite = record(&source, callee, mirrored_after, &[0x8d, 0xfd, 0x09]);
    add_writes(&mut overwrite, &[stack_write(0x09fd, callee.a)]);
    assert!(
        recorder
            .step(callee, mirrored_after, Some(&overwrite), &[])
            .is_err()
    );

    write_bytes(&mut source, 0x8010, &[0xbb, 0x00, 0x20]);
    let mut recorder = Recorder::new(&source).unwrap();
    let mut call = record(&source, before, callee, &[0x20, 0x10, 0x80]);
    add_writes(&mut call, &push_writes(0xfd, &[0x80, 0x02]));
    recorder.step(before, callee, Some(&call), &[]).unwrap();
    let las_after = state(0x8013, 0xf0, 17);
    let las = record(&source, callee, las_after, &[0xbb, 0x00, 0x20]);
    assert!(recorder.step(callee, las_after, Some(&las), &[]).is_err());
}

#[test]
fn rejects_source_or_writer_provenance_mismatches() {
    let mut source = source();
    write_bytes(&mut source, 0x8000, &[0x8d, 0x00, 0x40]);
    let before = state(0x8000, 0xfd, 7);
    let after = state(0x8003, 0xfd, 11);
    let mut wrong_bytes = record(&source, before, after, &[0x8d, 0x01, 0x40]);
    add_writes(&mut wrong_bytes, &[stack_write(0x4001, before.a)]);
    assert!(
        Recorder::new(&source)
            .unwrap()
            .step(before, after, Some(&wrong_bytes), &[])
            .is_err()
    );

    let mut good = record(&source, before, after, &[0x8d, 0x00, 0x40]);
    add_writes(&mut good, &[stack_write(0x4000, before.a)]);
    let mut bad_writer = writer(&source, 0x8000, 0x4000, before.a, 0);
    bad_writer.instruction_source = AudioTraceSource::CartridgeRom {
        offset: 99,
        bit_reversed: false,
    };
    assert!(
        Recorder::new(&source)
            .unwrap()
            .step(before, after, Some(&good), &[(0, bad_writer)])
            .is_err()
    );
}

#[test]
fn authenticates_each_byte_through_the_sixteen_kib_mirror() {
    let mut source = source();
    write_bytes(&mut source, 0x8000, &[0x8d, 0x00, 0x40]);
    let before = state(0xc000, 0xfd, 7);
    let after = state(0xc003, 0xfd, 11);
    let mut record = record(&source, before, after, &[0x8d, 0x00, 0x40]);
    assert_eq!(record.physical_rom_offset, Some(0));
    add_writes(&mut record, &[stack_write(0x4000, before.a)]);
    let result = Recorder::new(&source).unwrap().step(
        before,
        after,
        Some(&record),
        &[(1, writer(&source, 0xc000, 0x4000, before.a, 0))],
    );
    assert!(result.is_ok());
}

#[test]
fn rejects_idle_writers_and_enforces_depth_and_observation_caps() {
    let rom = source();
    let before = state(0x8000, 0xfd, 7);
    let after = state(0x8000, 0xfd, 8);
    assert!(
        Recorder::new(&rom)
            .unwrap()
            .step(
                before,
                after,
                None,
                &[(0, writer(&rom, 0x8000, 0x4000, before.a, 0))]
            )
            .is_err()
    );

    let mut depth_source = source();
    for index in 0..=MAX_DEPTH {
        let pc = 0x8000 + (index as u16 * 3);
        let target = pc + 3;
        write_bytes(
            &mut depth_source,
            pc,
            &[0x20, target as u8, (target >> 8) as u8],
        );
    }
    let mut recorder = Recorder::new(&depth_source).unwrap();
    let mut before = state(0x8000, 0xfd, 7);
    for index in 0..=MAX_DEPTH {
        let target = before.pc + 3;
        let after = state(target, before.sp.wrapping_sub(2), before.cycle + 6);
        let mut call = record(
            &depth_source,
            before,
            after,
            &[0x20, target as u8, (target >> 8) as u8],
        );
        let pushed = target.wrapping_sub(1);
        add_writes(
            &mut call,
            &push_writes(before.sp, &[pushed.to_be_bytes()[0], pushed as u8]),
        );
        let result = recorder.step(before, after, Some(&call), &[]);
        if index == MAX_DEPTH {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
        before = after;
    }

    let mut observation_source = source();
    for index in 0..=MAX_OBSERVATIONS {
        write_bytes(
            &mut observation_source,
            0x8000 + (index as u16 * 3),
            &[0x8d, 0x00, 0x40],
        );
    }
    let mut recorder = Recorder::new(&observation_source).unwrap();
    let mut before = state(0x8000, 0xfd, 7);
    for index in 0..=MAX_OBSERVATIONS {
        let after = state(before.pc + 3, before.sp, before.cycle + 4);
        let mut store = record(&observation_source, before, after, &[0x8d, 0x00, 0x40]);
        add_writes(&mut store, &[stack_write(0x4000, before.a)]);
        let result = recorder.step(
            before,
            after,
            Some(&store),
            &[(
                index,
                writer(
                    &observation_source,
                    before.pc,
                    0x4000,
                    before.a,
                    before.cycle - 7,
                ),
            )],
        );
        if index == MAX_OBSERVATIONS {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
        before = after;
    }
}
