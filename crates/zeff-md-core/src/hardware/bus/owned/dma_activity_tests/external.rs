use super::*;
use zeff_m68k::bus::FunctionCode;

fn before_external() -> (Bus, Vec<u8>) {
    let mut program = Vec::new();
    for word in [
        0x8114, 0x8f02, 0x9340, 0x9400, 0x9580, 0x9601, 0x9700, 0x4400, 0x0080,
    ] {
        program.extend([0x33fc, word, 0x00c0, 4]);
    }
    program.extend([0x60fe, 0x4e71]);
    program.resize(128, 0x4e71);
    let mut expected = Vec::new();
    for index in 0..64_u16 {
        let word = 0xa500 | index;
        program.push(word);
        expected.extend(word.to_be_bytes());
    }
    let bus = bus_at_prefix(&program, 8);
    assert_eq!(bus.vdp.dma_activity_epoch(), 0);
    assert_eq!(&bus.cartridge.bytes[0x300..0x380], expected);
    assert!(bus.vdp.memory().vram().iter().all(|byte| *byte == 0));
    (bus, expected)
}

fn restored_vdp(bus: &Bus) -> Bus {
    let mut writer = zeff_emu_common::save_state::StateWriter::new();
    bus.vdp
        .write_state_in_region(&mut writer, bus.region)
        .unwrap();
    let bytes = writer.into_bytes();
    let mut replacement = Vdp::bootstrap_mode5();
    replacement
        .read_state(
            &mut zeff_emu_common::save_state::StateReader::new(&bytes),
            bus.region,
        )
        .unwrap();
    let mut restored = bus.clone();
    restored.vdp = replacement;
    assert_eq!(&restored, bus);
    restored
}

fn dropped_then_committed_half(bus: &mut Bus) -> bool {
    let before = bus.clone();
    let epoch = bus.vdp.dma_activity_epoch();
    discard_half(bus);
    assert_eq!(*bus, before);
    assert_eq!(bus.vdp.dma_activity_epoch(), epoch);
    bus.owned_context().unwrap().half().unwrap();
    bus.vdp.dma_activity_epoch() != epoch
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_external_dma_dropped_halves_preserve_hardware_and_saved_continuation() {
    let (mut bus, expected) = before_external();
    let start = bus.cpu_clock;
    bus.owned_context()
        .unwrap()
        .write_cycle(
            BusAccess {
                logical_address: 0xc00004,
                bus_address: 0xc00004,
                lanes: ByteLanes::Both,
                kind: AccessKind::Data,
                function_code: FunctionCode::SupervisorData,
            },
            0x0080,
            start,
        )
        .result
        .unwrap();
    let mut restored: Option<Bus> = None;
    let mut committed_activity = false;
    let mut finished = false;
    for _ in 0..32_768 {
        let epoch = bus.vdp.dma_activity_epoch();
        committed_activity |= dropped_then_committed_half(&mut bus);
        if let Some(other) = &mut restored {
            let restored_epoch = other.vdp.dma_activity_epoch();
            dropped_then_committed_half(other);
            assert_eq!(&bus, other);
            assert_eq!(
                bus.vdp.dma_activity_epoch().wrapping_sub(epoch),
                other.vdp.dma_activity_epoch().wrapping_sub(restored_epoch)
            );
        }
        let vram = bus.vdp.memory().vram();
        if restored.is_none()
            && vram[0x400..0x402] == expected[..2]
            && vram[0x47e..0x480] != expected[126..]
        {
            restored = Some(restored_vdp(&bus));
        }
        let registers = bus.vdp.owned_observation().unwrap().unwrap().registers;
        if vram[0x400..0x480] == expected && registers[19..24] == [0, 0, 0xc0, 1, 0] {
            finished = true;
            break;
        }
    }
    assert!(finished, "external DMA did not finish the known payload");
    assert!(
        committed_activity,
        "no committed external activity after a drop"
    );
    let mut restored = restored.unwrap();
    let mut expected_vram = vec![0; 65_536];
    expected_vram[0x400..0x480].copy_from_slice(&expected);
    assert_eq!(bus.vdp.memory().vram(), expected_vram);
    let epoch = bus.vdp.dma_activity_epoch();
    let restored_epoch = restored.vdp.dma_activity_epoch();
    for _ in 0..512 {
        dropped_then_committed_half(&mut bus);
        dropped_then_committed_half(&mut restored);
        assert_eq!(bus, restored);
        assert_eq!(bus.vdp.dma_activity_epoch(), epoch);
        assert_eq!(restored.vdp.dma_activity_epoch(), restored_epoch);
    }
    assert_eq!(bus.vdp.memory().vram(), expected_vram);
    assert_eq!(bus.owned_fault(), None);
}
