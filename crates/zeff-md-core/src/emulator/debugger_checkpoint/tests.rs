use super::*;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_m68k::timing::InstructionByteOrigin;

const PROFILES: [(TimingModel, bool); 3] = [
    (TimingModel::Standard, false),
    (TimingModel::Standard, true),
    (TimingModel::Precise, true),
];

fn image() -> Vec<u8> {
    let mut image = vec![0; 0x400];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    image[0x200..0x208].copy_from_slice(&[0x4e, 0xf9, 0, 0, 3, 0, 0x4e, 0x71]);
    image[0x300..0x308].copy_from_slice(&[0x70, 1, 0x4e, 0x71, 0x60, 0xfa, 0x4e, 0x71]);
    image
}

fn patch(value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address: 0x301,
        value: CheatValue::Constant(value),
    }
}

fn source(timing: TimingModel, concurrent_sound: bool) -> Emulator {
    let mut machine = Emulator::new_with_timing(
        image(),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
        timing,
    )
    .unwrap();
    machine.install_rom_patches(&[patch(7)]).unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(machine.queued_words(), Some([0x7007, 0x4e71]));
    machine
}

fn metadata(
    machine: &Emulator,
) -> (
    Option<[zeff_m68k::timing::InstructionInput; 2]>,
    Option<Vec<u8>>,
) {
    (
        machine.cpu.queued_instruction_inputs(),
        machine
            .bus
            .capture_board_origins(machine.rom_bytes().len() as u64)
            .unwrap(),
    )
}

fn changed(source: &Emulator) -> Emulator {
    let mut receiver = source.clone();
    receiver.step_instruction().unwrap();
    receiver.bus.ram[0x300] = 0x5a;
    receiver.install_rom_patches(&[patch(9)]).unwrap();
    assert_ne!(
        receiver.encode_frontend_state().unwrap(),
        source.encode_frontend_state().unwrap()
    );
    receiver
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn owned_debugger_checkpoint_preserves_real_sampled_queue_board_and_current_external_set() {
    for (timing, concurrent) in PROFILES {
        let donor = source(timing, concurrent);
        let frontend = donor.encode_frontend_state().unwrap();
        let checkpoint = donor.encode_debugger_checkpoint().unwrap();
        let origins = metadata(&donor);
        assert_eq!(
            origins.0.unwrap()[0].origin.map(|tag| tag.storage_offset()),
            [Some(0x300), Some(0x301)]
        );
        let mut receiver = changed(&donor);
        receiver.decode_debugger_checkpoint(&checkpoint).unwrap();
        assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
        assert_eq!(metadata(&receiver), origins);
        assert_eq!(receiver.peek(0x301), Some(9));
        assert_eq!(receiver.queued_words(), Some([0x7007, 0x4e71]));
        receiver.step_instruction().unwrap();
        assert_eq!(receiver.cpu.registers.d[0], 7);
        for _ in 0..3 {
            receiver.step_instruction().unwrap();
        }
        assert_eq!(receiver.cpu.registers.d[0], 9);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn owned_checkpoint_keeps_historical_bank_source_when_payload_mapper_already_changed() {
    use crate::cartridge::ssf2::tests::{cartridge, page_program};
    let mut cart = cartridge(&[0x13fc, 8, 0x00a1, 0x30f3, 0x4ef9, 0x0008, 0]);
    let old = [0x13fc_u16, 9, 0x00a1, 0x30f3, 0x7008, 0x4ef9, 0x0008, 8];
    let mut new = old;
    new[4] = 0x7009;
    let bytes = |words: &[u16]| {
        words
            .iter()
            .flat_map(|word| word.to_be_bytes())
            .collect::<Vec<_>>()
    };
    page_program(&mut cart, 8, &bytes(&old));
    page_program(&mut cart, 9, &bytes(&new));
    let mut donor = Emulator::new_with_cartridge(
        cart,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    for _ in 0..3 {
        donor.step_instruction().unwrap();
    }
    let tags = donor.cpu.queued_instruction_inputs().unwrap();
    assert_eq!(tags[0].value, 0x7008);
    assert_eq!(tags[0].origin[0].storage_offset(), Some(0x400008));
    assert_eq!(
        donor.bus.cartridge.rom_source_offset(0x80008),
        Some(0x480008)
    );
    let checkpoint = donor.encode_debugger_checkpoint().unwrap();
    let mut receiver = changed(&donor);
    receiver.decode_debugger_checkpoint(&checkpoint).unwrap();
    assert_eq!(receiver.cpu.queued_instruction_inputs(), Some(tags));
    assert_eq!(
        receiver.bus.cartridge.rom_source_offset(0x80008),
        Some(0x480008)
    );
    receiver.step_instruction().unwrap();
    assert_eq!(receiver.cpu.registers.d[0], 8);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn owned_legacy_frontend_is_strict_unknown_and_public_load_rejects_tagged_envelope() {
    for (timing, concurrent) in PROFILES {
        let donor = source(timing, concurrent);
        let frontend = donor.encode_frontend_state().unwrap();
        let envelope = donor.encode_debugger_checkpoint().unwrap();
        let mut receiver = changed(&donor);
        let before = receiver.clone();
        let before_metadata = metadata(&receiver);
        assert!(receiver.decode_frontend_state(&envelope).is_err());
        assert!(receiver.decode_state(&envelope).is_err());
        assert_eq!(receiver, before);
        assert_eq!(metadata(&receiver), before_metadata);
        receiver.decode_debugger_checkpoint(&frontend).unwrap();
        assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
        assert!(
            receiver
                .cpu
                .queued_instruction_inputs()
                .unwrap()
                .iter()
                .all(|word| word.origin == [InstructionByteOrigin::UNKNOWN; 2])
        );
        let unknown = receiver.encode_debugger_checkpoint().unwrap();
        let mut known = donor.clone();
        known.decode_debugger_checkpoint(&unknown).unwrap();
        assert_eq!(metadata(&known), metadata(&receiver));
        let before = metadata(&known);
        assert!(
            known
                .decode_debugger_checkpoint(&donor.encode_state().unwrap())
                .is_err()
        );
        assert_eq!(metadata(&known), before);
    }
}

fn ranges(
    bytes: &[u8],
) -> (
    std::ops::Range<usize>,
    std::ops::Range<usize>,
    Option<std::ops::Range<usize>>,
) {
    let mut reader = StateReader::new(bytes);
    reader.set_position(76);
    let next = |reader: &mut StateReader<'_>| {
        let length = reader.read_u32().unwrap() as usize;
        let start = reader.position();
        reader.set_position(start + length);
        start..start + length
    };
    let frontend = next(&mut reader);
    let cpu = next(&mut reader);
    let board = if reader.read_bool().unwrap() {
        Some(next(&mut reader))
    } else {
        None
    };
    (frontend, cpu, board)
}

fn reseal(bytes: &mut [u8]) {
    let end = bytes.len() - 32;
    let checksum = digest(&bytes[..end]);
    bytes[end..].copy_from_slice(&checksum);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn owned_checkpoint_corruption_binding_bounds_and_late_board_failure_leave_everything_unchanged() {
    for (timing, concurrent) in PROFILES {
        let donor = source(timing, concurrent);
        let good = donor.encode_debugger_checkpoint().unwrap();
        let (_, cpu, board) = ranges(&good);
        let mut variants = vec![good[..5].to_vec(), good[..good.len() - 1].to_vec()];
        let mut bytes = good.clone();
        bytes[100] ^= 1;
        variants.push(bytes);
        for index in [8, 12, 44, cpu.start, cpu.start + 10] {
            let mut bytes = good.clone();
            bytes[index] ^= 1;
            reseal(&mut bytes);
            variants.push(bytes);
        }
        let mut bytes = good.clone();
        bytes[76..80].copy_from_slice(&u32::MAX.to_le_bytes());
        reseal(&mut bytes);
        variants.push(bytes);
        let mut bytes = good.clone();
        bytes[cpu.end - 8..cpu.end].copy_from_slice(&u64::MAX.to_le_bytes());
        reseal(&mut bytes);
        variants.push(bytes);
        if let Some(board) = board {
            for index in [board.start, board.start + 1] {
                let mut bytes = good.clone();
                bytes[index] ^= 1;
                reseal(&mut bytes);
                variants.push(bytes);
            }
        }
        for bytes in variants {
            let mut receiver = changed(&donor);
            let before = receiver.clone();
            let tags = metadata(&receiver);
            let frontend = receiver.encode_frontend_state().unwrap();
            assert!(receiver.decode_debugger_checkpoint(&bytes).is_err());
            assert_eq!(receiver, before);
            assert_eq!(metadata(&receiver), tags);
            assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
        }
        let mut foreign = image();
        foreign[0x3fe] ^= 1;
        let mut receiver = Emulator::new_with_timing(
            foreign,
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: concurrent,
            },
            timing,
        )
        .unwrap();
        let before = receiver.encode_frontend_state().unwrap();
        let tags = metadata(&receiver);
        assert!(receiver.decode_debugger_checkpoint(&good).is_err());
        assert_eq!(receiver.encode_frontend_state().unwrap(), before);
        assert_eq!(metadata(&receiver), tags);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn owned_checkpoint_refuses_active_audio_batch_before_state_encoder_assertions() {
    let mut machine = source(TimingModel::Standard, true);
    let checkpoint = machine.encode_debugger_checkpoint().unwrap();
    machine.bus.audio.begin_batch();
    let before = machine.clone();
    let tags = metadata(&machine);
    assert!(machine.encode_debugger_checkpoint().is_err());
    assert!(machine.decode_debugger_checkpoint(&checkpoint).is_err());
    assert_eq!(machine, before);
    assert_eq!(metadata(&machine), tags);
}
