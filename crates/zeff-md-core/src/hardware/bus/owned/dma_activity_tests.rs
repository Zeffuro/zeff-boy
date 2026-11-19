use super::*;
use zeff_m68k::timing::PrefetchCpu;

fn active_local(fill: bool) -> Bus {
    let mut program = Vec::new();
    for word in [
        0x8114,
        0x8f01,
        0x9340,
        0x9400,
        0x9500,
        0x9600,
        if fill { 0x9780 } else { 0x97c0 },
        if fill { 0x4400 } else { 0x0400 },
        if fill { 0x0080 } else { 0x00c0 },
    ] {
        program.extend([0x33fc, word, 0x00c0, 4]);
    }
    if fill {
        program.extend([0x33fc, 0, 0x00c0, 0]);
    }
    program.extend([0x60fe, 0x4e71]);
    bus_at_prefix(&program, 9 + usize::from(fill))
}

fn bus_at_prefix(program: &[u16], instructions: usize) -> Bus {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut bus = Bus::new_owned(Cartridge::new(rom).unwrap(), Region::NtscUs).unwrap();
    let mut cpu = PrefetchCpu::default();
    cpu.registers = bus.cartridge.effective_initial();
    cpu.prime(&mut bus.owned_context().unwrap()).unwrap();
    for _ in 0..instructions {
        cpu.step_boundary(&mut bus.owned_context().unwrap())
            .unwrap();
    }
    bus
}

fn discard_half(bus: &mut Bus) {
    let owner = bus.owned.as_ref().unwrap();
    let boundary = boundary::Boundary {
        previous: (),
        physical: (),
        fifo_witness: None,
        work_witness: None,
        board: owner.board.clone(),
        sound: owner.sound.clone(),
        audio: &bus.audio,
        read: protocol::ReadContext {
            region: bus.region,
            input: &bus.input,
            prefetch: bus.prefetch,
        },
    };
    let cartridge = &bus.cartridge;
    let _prepared = bus
        .vdp
        .prepare_owned_half(boundary, &mut bus.ram, |address| {
            cartridge
                .read(address)
                .ok_or_else(|| anyhow::anyhow!("unmapped source"))
        })
        .unwrap();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_dropped_real_local_service_half_is_atomic_and_commit_is_visible() {
    for fill in [false, true] {
        let mut bus = active_local(fill);
        let mut found = false;
        for _ in 0..4096 {
            let saved = bus.clone();
            let epoch = bus.vdp.dma_activity_epoch();
            discard_half(&mut bus);
            assert_eq!(bus, saved);
            assert_eq!(bus.vdp.dma_activity_epoch(), epoch);
            bus.owned_context().unwrap().half().unwrap();
            if bus.vdp.dma_activity_epoch() != epoch {
                assert_eq!(bus.vdp.dma_activity_epoch(), epoch.wrapping_add(1));
                found = true;
                break;
            }
        }
        assert!(found, "no actual local service half");
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
        for _ in 0..128 {
            let original_epoch = bus.vdp.dma_activity_epoch();
            let restored_epoch = restored.vdp.dma_activity_epoch();
            bus.owned_context().unwrap().half().unwrap();
            restored.owned_context().unwrap().half().unwrap();
            assert_eq!(bus, restored);
            assert_eq!(
                bus.vdp.dma_activity_epoch().wrapping_sub(original_epoch),
                restored
                    .vdp
                    .dma_activity_epoch()
                    .wrapping_sub(restored_epoch)
            );
        }
    }
}

mod external;
mod instruction_origin;

mod checkpoint;
