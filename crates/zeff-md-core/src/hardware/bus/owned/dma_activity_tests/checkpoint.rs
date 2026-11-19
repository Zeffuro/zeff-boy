use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::bus::FunctionCode;

fn hardware(board: &Board) -> Vec<u8> {
    let mut writer = StateWriter::new();
    board.write_state(&mut writer).unwrap();
    writer.into_bytes()
}

fn slots(bytes: &[u8]) -> [Option<std::ops::Range<usize>>; 2] {
    let mut reader = StateReader::new(bytes);
    reader.set_position(33);
    std::array::from_fn(|_| {
        if !reader.read_bool().unwrap() {
            return None;
        }
        let start = reader.position();
        reader.set_position(start + 35);
        Some(start..start + 35)
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_owned_capsule_restores_real_frozen_request_and_rejects_late_slot_mismatches() {
    let mut bus = bus_at_prefix(&[0x4e71, 0x60fc], 0);
    let request = main_bus::Request {
        access: BusAccess {
            logical_address: 0xab000200,
            bus_address: 0x200,
            lanes: ByteLanes::Both,
            kind: AccessKind::Prefetch,
            function_code: FunctionCode::SupervisorProgram,
        },
        operation: main_bus::Operation::Read,
    };
    bus.owned.as_mut().unwrap().board.offer(request).unwrap();
    let mut reached = false;
    for _ in 0..4096 {
        bus.owned_context().unwrap().half().unwrap();
        if bus.owned.as_ref().unwrap().board.snapshot().unwrap().phase == main_bus::Phase::S7 {
            reached = true;
            break;
        }
    }
    assert!(reached, "actual MAIN request never reached S7");
    let board = &bus.owned.as_ref().unwrap().board;
    let origin = board.sampled_read_debug();
    assert_eq!(
        origin
            .1
            .sampled_input()
            .unwrap()
            .origin
            .map(|tag| tag.storage_offset()),
        [Some(0x200), Some(0x201)]
    );
    let bytes = hardware(board);
    let capsule = board
        .capture_instruction_origins(bus.cartridge.bytes.len() as u64)
        .unwrap();
    let mut restored = Board::read_state(&mut StateReader::new(&bytes), board.half()).unwrap();
    assert_ne!(restored.sampled_read_debug().1, origin.1);
    restored
        .attach_instruction_origins(&capsule, bus.cartridge.bytes.len() as u64)
        .unwrap();
    assert_eq!(restored.sampled_read_debug(), origin);
    assert_eq!(hardware(&restored), bytes);
    let frozen = slots(&capsule)[1].clone().unwrap();
    let mut variants = Vec::new();
    for index in [0, 1, frozen.start, frozen.start + 17] {
        let mut invalid = capsule.clone();
        invalid[index] ^= 1;
        variants.push(invalid);
    }
    let mut invalid = capsule.clone();
    invalid[frozen.start + 9..frozen.start + 17].copy_from_slice(&u64::MAX.to_le_bytes());
    variants.push(invalid);
    let mut invalid = capsule.clone();
    invalid[frozen.start + 27..frozen.end].copy_from_slice(&u64::MAX.to_le_bytes());
    variants.push(invalid);
    let mut invalid = capsule.clone();
    invalid.push(0);
    variants.push(invalid);
    for invalid in variants {
        let before = restored.sampled_read_debug();
        assert!(
            restored
                .attach_instruction_origins(&invalid, bus.cartridge.bytes.len() as u64)
                .is_err()
        );
        assert_eq!(restored.sampled_read_debug(), before);
        assert_eq!(hardware(&restored), bytes);
    }
    assert!(
        restored
            .attach_instruction_origins(&capsule, 0x201)
            .is_err()
    );
    assert_eq!(restored.sampled_read_debug(), origin);
}
