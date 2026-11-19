use super::*;
use zeff_m68k::bus::FunctionCode;
use zeff_m68k::timing::{InstructionByteOrigin, InstructionInput};

fn access(address: u32, kind: AccessKind, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0xfffffe,
        lanes,
        kind,
        function_code: if kind == AccessKind::Prefetch {
            FunctionCode::SupervisorProgram
        } else {
            FunctionCode::SupervisorData
        },
    }
}

fn fetch(bus: &mut Bus, access: BusAccess) -> InstructionInput {
    let start = bus.cpu_clock;
    let result = bus
        .owned_context()
        .unwrap()
        .read_instruction_cycle(access, start);
    let input = result.result.unwrap();
    assert_eq!(bus.cpu_clock.0 - start.0, 4 + u64::from(result.wait_clocks));
    input
}

fn known(input: InstructionInput, source: u64, value: u16) {
    assert_eq!(input.value, value);
    assert_eq!(
        input.origin.map(|origin| origin.storage_offset()),
        [Some(source), Some(source + 1)]
    );
}

fn unknown(input: InstructionInput, value: u16) {
    assert_eq!(input.value, value);
    assert_eq!(input.origin, [InstructionByteOrigin::UNKNOWN; 2]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_instruction_samples_use_actual_rom_owner_and_reject_equal_nonrom_drivers() {
    let mut bus = bus_at_prefix(&[0x4e71, 0x60fc], 0);
    for address in [0x200, 0xab000200] {
        known(
            fetch(
                &mut bus,
                access(address, AccessKind::Prefetch, ByteLanes::Both),
            ),
            0x200,
            0x4e71,
        );
    }
    unknown(
        fetch(
            &mut bus,
            access(0xa00000, AccessKind::Prefetch, ByteLanes::Both),
        ),
        0x4e71,
    );
    known(
        fetch(
            &mut bus,
            access(0x200, AccessKind::Prefetch, ByteLanes::Both),
        ),
        0x200,
        0x4e71,
    );
    unknown(
        fetch(
            &mut bus,
            access(0x200, AccessKind::Prefetch, ByteLanes::Upper),
        ),
        0x4e71,
    );
    unknown(
        fetch(&mut bus, access(0x200, AccessKind::Data, ByteLanes::Both)),
        0x4e71,
    );
    let start = bus.cpu_clock;
    bus.owned_context()
        .unwrap()
        .write_cycle(
            access(0xff0200, AccessKind::Data, ByteLanes::Both),
            0x4e71,
            start,
        )
        .result
        .unwrap();
    unknown(
        fetch(
            &mut bus,
            access(0xff0200, AccessKind::Prefetch, ByteLanes::Both),
        ),
        0x4e71,
    );
    let start = bus.cpu_clock;
    let observed = bus
        .owned_context()
        .unwrap()
        .read_instruction_cycle_observed(
            access(0x200, AccessKind::Prefetch, ByteLanes::Both),
            start,
            zeff_m68k::timing::InterruptAperture::FollowingPrefetch,
        );
    assert!(observed.samples.is_none());
    known(observed.cycle.result.unwrap(), 0x200, 0x4e71);
    bus.validate_owned_state().unwrap();
    unknown(
        fetch(
            &mut bus,
            access(0x800, AccessKind::Prefetch, ByteLanes::Both),
        ),
        0xffff,
    );
    let start = bus.cpu_clock;
    let failed = bus.owned_context().unwrap().read_instruction_cycle(
        access(0xa10020, AccessKind::Prefetch, ByteLanes::Both),
        start,
    );
    assert!(failed.result.is_err());
    assert!(
        bus.owned
            .as_ref()
            .unwrap()
            .board
            .sampled_read_debug()
            .1
            .sampled_input()
            .is_none()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sampled_origin_survives_postsample_overwrite_drop_and_public_board_decode() {
    let mut bus = bus_at_prefix(&[0x4e71, 0x60fc], 0);
    let request = main_bus::Request {
        access: access(0x200, AccessKind::Prefetch, ByteLanes::Both),
        operation: main_bus::Operation::Read,
    };
    bus.owned.as_mut().unwrap().board.offer(request).unwrap();
    let overlay = bus
        .cartridge
        .prepare_rom_overlay(&[
            zeff_emu_common::cheats::CheatPatch::WideRomWrite {
                address: 0x200,
                value: zeff_emu_common::cheats::CheatValue::Constant(0xaa),
            },
            zeff_emu_common::cheats::CheatPatch::WideRomWrite {
                address: 0x201,
                value: zeff_emu_common::cheats::CheatValue::Constant(0xbb),
            },
        ])
        .unwrap();
    let mut sampled = false;
    for _ in 0..4096 {
        let before = bus.clone();
        let metadata = bus.owned.as_ref().unwrap().board.sampled_read_debug();
        discard_half(&mut bus);
        assert_eq!(bus, before);
        assert_eq!(
            bus.owned.as_ref().unwrap().board.sampled_read_debug(),
            metadata
        );
        let snapshot = bus.owned.as_ref().unwrap().board.snapshot().unwrap();
        if snapshot.phase == main_bus::Phase::S6 {
            let mut trial = bus.clone();
            trial.cartridge.rom_overlay = overlay.clone();
            trial.owned_context().unwrap().half().unwrap();
            let sampled_edge = trial.owned.as_ref().unwrap().board.snapshot().unwrap();
            if sampled_edge.phase == main_bus::Phase::S7 {
                assert!(snapshot.clock_high && !sampled_edge.clock_high);
                assert_eq!(sampled_edge.now_half, snapshot.now_half + 1);
                assert_eq!(sampled_edge.last_edge_half, sampled_edge.now_half);
                assert_eq!(snapshot.pending.unwrap().read_word, None);
                assert_eq!(metadata.0, 0x4e71);
                known(metadata.1.latched_input().unwrap(), 0x200, 0x4e71);
                assert_eq!(sampled_edge.pending.unwrap().read_word, Some(0x4e71));
                let (held, metadata) = trial.owned.as_ref().unwrap().board.sampled_read_debug();
                known(metadata.sampled_input().unwrap(), 0x200, 0x4e71);
                assert_eq!(
                    held, 0xaabb,
                    "post-sample cart data was not physically driven"
                );
                let saved = trial.clone();
                discard_half(&mut trial);
                assert_eq!(trial, saved);
                assert_eq!(
                    trial.owned.as_ref().unwrap().board.sampled_read_debug(),
                    (held, metadata)
                );
                bus = trial;
                sampled = true;
                break;
            }
        }
        bus.owned_context().unwrap().half().unwrap();
    }
    assert!(
        sampled,
        "offered MAIN read never reached its S6-to-S7 sample half"
    );
    let board = &bus.owned.as_ref().unwrap().board;
    let mut writer = zeff_emu_common::save_state::StateWriter::new();
    board.write_state(&mut writer).unwrap();
    let bytes = writer.into_bytes();
    let decoded = Board::read_state(
        &mut zeff_emu_common::save_state::StateReader::new(&bytes),
        board.half(),
    )
    .unwrap();
    assert_eq!(decoded, *board);
    assert_ne!(decoded.sampled_read_debug().1, board.sampled_read_debug().1);
    assert!(decoded.sampled_read_debug().1.latched_input().is_none());
    assert!(decoded.sampled_read_debug().1.sampled_input().is_none());
    let mut writer = zeff_emu_common::save_state::StateWriter::new();
    decoded.write_state(&mut writer).unwrap();
    assert_eq!(writer.into_bytes(), bytes);
    let mut imported = bus.clone();
    imported.owned.as_mut().unwrap().board = decoded;
    for _ in 0..256 {
        if bus
            .owned
            .as_ref()
            .unwrap()
            .board
            .snapshot()
            .unwrap()
            .completed
            .is_some()
        {
            break;
        }
        bus.owned_context().unwrap().half().unwrap();
        imported.owned_context().unwrap().half().unwrap();
        assert_eq!(bus, imported);
    }
    let complete = bus
        .owned
        .as_mut()
        .unwrap()
        .board
        .take_completed()
        .unwrap()
        .unwrap();
    let restored = imported
        .owned
        .as_mut()
        .unwrap()
        .board
        .take_completed()
        .unwrap()
        .unwrap();
    assert_eq!(complete.main, restored.main);
    assert_eq!(complete.main.read_word, Some(0x4e71));
    assert_eq!(
        complete.origin.map(|origin| origin.storage_offset()),
        [Some(0x200), Some(0x201)]
    );
    assert_eq!(restored.origin, [InstructionByteOrigin::UNKNOWN; 2]);
}
