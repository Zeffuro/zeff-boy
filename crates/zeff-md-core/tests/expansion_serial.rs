use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::{ControllerKind, ControllerPort, Emulator, LoadConfig, Region};

const TX: u32 = 0xa1_001b;
const CONTROL: u32 = 0xa1_001f;
const MARKER: u32 = 0xff_0100;
const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

fn store(program: &mut Vec<u16>, address: u32, value: u16, word: bool) {
    program.extend_from_slice(&[
        if word { 0x33fc } else { 0x13fc },
        value,
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn read(program: &mut Vec<u16>, register: u16) {
    program.extend_from_slice(&[0x1039 | register << 9, 0x00a1, 0x001f]);
}

fn poll(program: &mut Vec<u16>) {
    program.extend_from_slice(&[0x0839, 0, 0x00a1, 0x001f, 0x66f6]);
}

fn machine(program: &[u16], region: Region, concurrent_sound: bool) -> Emulator {
    let mut bytes = vec![0; 0x1000];
    bytes[..8].copy_from_slice(&[0, 255, 255, 252, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        bytes[0x100 + 2 * index..0x102 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        bytes,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn program(word: bool) -> Vec<u16> {
    let offset = u32::from(word);
    let mut program = Vec::new();
    store(&mut program, CONTROL - offset, 0x123f, word);
    read(&mut program, 0);
    store(&mut program, TX - offset, 0x125a, word);
    read(&mut program, 1);
    poll(&mut program);
    store(&mut program, TX - offset, 0x34a5, word);
    read(&mut program, 2);
    poll(&mut program);
    read(&mut program, 3);
    store(&mut program, MARKER, 0xcafe, true);
    program.extend_from_slice(&[0x60fe, 0x4e71]);
    program
}

fn finish(machine: &mut Emulator) {
    for _ in 0..20_000 {
        if machine.peek(MARKER) == Some(0xca) {
            assert_eq!(machine.peek(MARKER + 1), Some(0xfe));
            assert_eq!(&machine.registers().d[..4], &[0x38, 0x39, 0x39, 0x38]);
            assert_eq!(machine.interrupt_level(), 0);
            return;
        }
        machine.step_instruction().unwrap();
    }
    panic!("serial polling did not finish");
}

#[test]
fn actual_m68k_polling_and_lower_word_lanes_complete_in_every_region_and_scheduler() {
    for region in REGIONS {
        for concurrent in [false, true] {
            for word in [false, true] {
                let mut machine = machine(&program(word), region, concurrent);
                finish(&mut machine);
                assert_eq!(machine.peek(TX), Some(0xa5));
                assert_eq!(machine.peek(CONTROL - 2), Some(0));
                assert_eq!(machine.peek(0xa1_0013), Some(0));
                assert_eq!(machine.peek(0xa1_000d), Some(0));
            }
        }
    }
}

#[test]
fn actual_machine_queued_and_shifting_transfers_survive_clone_raw_and_frontend_restore() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let code = program(false);
            let mut source = machine(&code, region, concurrent);
            source.set_sample_rate(48_000);
            source
                .input_mut()
                .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
            for stage in 0..2 {
                if stage == 0 {
                    for _ in 0..4 {
                        source.step_instruction().unwrap();
                    }
                } else {
                    for _ in 0..20_000 {
                        source.step_instruction().unwrap();
                        if source.peek(TX) == Some(0xa5) {
                            break;
                        }
                    }
                    assert_eq!(source.peek(TX), Some(0xa5));
                }
                assert_eq!(source.peek(CONTROL), Some(0x39));
                let raw = source.encode_state().unwrap();
                let front = source.encode_frontend_state().unwrap();
                assert_eq!(&raw[8..12], &1_u32.to_le_bytes());
                for frontend in [false, true] {
                    let mut expected = source.clone();
                    let mut restored = source.clone();
                    restored.step_instruction().unwrap();
                    if frontend {
                        restored.decode_frontend_state(&front).unwrap();
                        assert_eq!(restored.encode_frontend_state().unwrap(), front);
                    } else {
                        restored.decode_state(&raw).unwrap();
                        expected.decode_state(&raw).unwrap();
                        assert_eq!(restored.encode_state().unwrap(), raw);
                    }
                    finish(&mut expected);
                    finish(&mut restored);
                    assert_eq!(
                        restored.encode_state().unwrap(),
                        expected.encode_state().unwrap()
                    );
                    assert_eq!(
                        restored.drain_audio_samples(),
                        expected.drain_audio_samples()
                    );
                    assert_eq!(restored.framebuffer(), expected.framebuffer());
                }
            }
            let before = source.input().clone();
            source.reset_sound_cpu();
            source.reset_cpu().unwrap();
            assert_eq!(source.input(), &before);
            Reset::reset(&mut source);
            assert_eq!(source.peek(CONTROL), Some(0));
            assert_eq!(source.peek(TX), Some(0xff));
        }
    }
}

fn bus_history(bytes: &[u8]) -> (usize, usize, StateReader<'_>) {
    let mut outer = StateReader::new(bytes);
    outer.set_position(52);
    outer.read_slice(2_000_000).unwrap();
    let bus_at = outer.position() + 4;
    let mut bus = StateReader::new(outer.read_slice(2_000_000).unwrap());
    bus.read_u64().unwrap();
    bus.read_u64().unwrap();
    bus.read_u8().unwrap();
    bus.read_u64().unwrap();
    let history_at = bus_at + bus.position();
    (bus_at, history_at, bus)
}

fn serial_offset(bytes: &[u8]) -> usize {
    let (bus_at, _, mut bus) = bus_history(bytes);
    match bus.read_u8().unwrap() {
        0 => {}
        1 => {
            bus.read_u8().unwrap();
        }
        2 => {
            bus.read_u16().unwrap();
        }
        tag => panic!("invalid current prefetch tag {tag}"),
    }
    bus.set_position(bus.position() + 65_536);
    let input_at = bus_at + bus.position() + 4;
    let mut input = StateReader::new(bus.read_slice(2_000_000).unwrap());
    for _ in 0..2 {
        assert!(input.read_u8().unwrap() <= 1);
        input.read_u8().unwrap();
        input.read_u8().unwrap();
        input.read_bool().unwrap();
        input.read_u8().unwrap();
        if input.read_bool().unwrap() {
            input.read_u64().unwrap();
        }
    }
    let mut registers = [0; 12];
    input.read_exact(&mut registers).unwrap();
    let at = input_at + input.position();
    input.read_u64().unwrap();
    input.read_u32().unwrap();
    assert!(input.read_u8().unwrap() < 4);
    assert!(input.read_u8().unwrap() <= 10);
    input.read_u8().unwrap();
    assert!(input.read_bool().unwrap());
    assert_eq!(input.read_u8().unwrap(), 0x5a);
    assert_eq!(input.read_slice(2_000_000).unwrap().len(), 2);
    input.read_slice(2_000_000).unwrap();
    assert!(input.is_exhausted());
    at
}

#[test]
fn malformed_serial_state_and_unsupported_pad_modes_leave_state_transactional() {
    let code = program(false);
    let mut source = machine(&code, Region::NtscUs, true);
    for _ in 0..4 {
        source.step_instruction().unwrap();
    }
    let saved = source.encode_frontend_state().unwrap();
    let raw = source.encode_state().unwrap();
    let uart_at = serial_offset(&raw);
    for (offset, value, expected) in [
        (7, 0xff, "invalid MD serial clock"),
        (11, 0xff, "invalid MD serial clock"),
        (12, 4, "invalid MD serial clock"),
        (13, 11, "invalid MD serial transmitter"),
        (15, 2, "invalid boolean value"),
    ] {
        let mut invalid = raw.clone();
        invalid[uart_at + offset] = value;
        let mut target = source.clone();
        assert!(
            target
                .decode_state(&invalid)
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
        assert_eq!(target.encode_frontend_state().unwrap(), saved);
        let mut front = StateWriter::new();
        front.write_bytes(&saved[..12]);
        front.write_vec(&invalid);
        let mut reader = StateReader::new(&saved);
        reader.set_position(12);
        reader.read_slice(2_000_000).unwrap();
        front.write_vec(reader.read_slice(2_000_000).unwrap());
        assert!(
            target
                .decode_frontend_state(&front.into_bytes())
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
        assert_eq!(target.encode_frontend_state().unwrap(), saved);
    }
    for address in [0xa1_0013, 0xa1_0019] {
        let mut code = Vec::new();
        store(&mut code, address, 0x38, false);
        code.extend_from_slice(&[0x60fe, 0x4e71]);
        let mut target = machine(&code, Region::NtscUs, true);
        let input = target.input().clone();
        let before = target.timing_snapshot().now();
        assert!(target.step_instruction().is_err());
        assert_eq!(target.input(), &input);
        assert!(target.timing_snapshot().now() > before);
    }
    let mut code = Vec::new();
    store(&mut code, CONTROL - 1, 0x38, false);
    code.extend_from_slice(&[0x60fe, 0x4e71]);
    let mut target = machine(&code, Region::NtscUs, true);
    let input = target.input().clone();
    let before = target.timing_snapshot().now();
    target.step_instruction().unwrap();
    assert_eq!(target.input(), &input);
    assert_eq!(target.peek(CONTROL), Some(0));
    assert!(target.timing_snapshot().now() > before);
}
