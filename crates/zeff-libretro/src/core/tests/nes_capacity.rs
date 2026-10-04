use super::*;
use crate::api::retro_game_info;
use crate::callbacks::{ABI_TEST_LOCK, CORE, lock};

struct LoadedCore;

impl Drop for LoadedCore {
    fn drop(&mut self) {
        crate::game::retro_unload_game();
    }
}

#[test]
fn nes_serialization_capacity_excludes_mutable_disk_media() {
    let mut rom = nes_rom();
    rom[6] = 0x40;
    rom[7] = 0x10;
    let state = CoreState::from_rom(&rom, "disk.nes").unwrap();
    assert_eq!(state.fixed_serialize_size(), None);
}

#[test]
fn nes_serialization_capacity_survives_incompressible_output() {
    let _abi = lock(&ABI_TEST_LOCK);
    check_capacity(&nes_rom());
}

#[test]
fn nes_serialization_capacity_covers_large_loaded_cartridges() {
    let _abi = lock(&ABI_TEST_LOCK);
    for mapper in [0_u8, 5, 34, 69, 85] {
        let chr_len = if mapper == 0 { 3 * 1024 * 1024 } else { 0x2000 };
        let mut rom = vec![0; 16 + 0x10000 + chr_len];
        rom[..4].copy_from_slice(b"NES\x1A");
        rom[4] = 4;
        rom[5] = if mapper == 0 { 128 } else { 1 };
        rom[6] = (mapper & 15) << 4;
        rom[7] = (mapper & 0xF0) | 8;
        rom[9] = if mapper == 0 { 0x10 } else { 0 };
        rom[10] = 14;
        check_capacity(&rom);
    }
}

fn check_capacity(rom: &[u8]) {
    let info = retro_game_info {
        path: c"capacity.nes".as_ptr(),
        data: rom.as_ptr().cast(),
        size: rom.len(),
        meta: std::ptr::null(),
    };
    assert!(crate::game::retro_load_game(&info));
    let _loaded = LoadedCore;
    let initial = lock(&CORE).as_ref().unwrap().encode_state().unwrap();
    let capacity = crate::serialization::retro_serialize_size();

    let mut raw = lz4_flex::decompress_size_prepended(&initial[12..]).unwrap();
    let framebuffer_start = raw.len() - zeff_nes_core::hardware::ppu::FRAMEBUFFER_SIZE;
    let mut random = 0xD6E8_FEB8_6659_FD93_u64;
    for pixel in raw[framebuffer_start..].as_chunks_mut::<4>().0 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        pixel.copy_from_slice(&[random as u8, (random >> 8) as u8, (random >> 16) as u8, 255]);
    }
    let mut noisy_state = initial[..12].to_vec();
    noisy_state.extend(lz4_flex::compress_prepend_size(&raw));
    assert!(noisy_state.len() > initial.len() + 4096);
    lock(&CORE)
        .as_mut()
        .unwrap()
        .load_state(&noisy_state)
        .unwrap();

    assert_eq!(crate::serialization::retro_serialize_size(), capacity);
    let mut buffer = vec![0xA5; capacity];
    assert!(!crate::serialization::retro_serialize(
        buffer.as_mut_ptr().cast(),
        capacity - 1
    ));
    assert!(buffer.iter().all(|byte| *byte == 0xA5));
    assert!(crate::serialization::retro_serialize(
        buffer.as_mut_ptr().cast(),
        capacity
    ));
    assert!(crate::serialization::retro_unserialize(
        buffer.as_ptr().cast(),
        capacity
    ));
    assert_eq!(
        lock(&CORE).as_ref().unwrap().encode_state().unwrap(),
        noisy_state
    );
    assert_eq!(crate::serialization::retro_serialize_size(), capacity);
    {
        use zeff_nes_core::hardware::controller::ControllerType;
        let mut core = lock(&CORE);
        let ActiveCore::Nes(emu) = &mut core.as_mut().unwrap().core else {
            unreachable!()
        };
        let bus = emu.bus_mut();
        for controller in [&mut bus.controller1, &mut bus.controller2] {
            controller.set_type(ControllerType::Zapper {
                trigger: true,
                hit: true,
            });
        }
        bus.cpu_write(0x4014, 0x02);
    }
    assert!(crate::serialization::retro_serialize(
        buffer.as_mut_ptr().cast(),
        capacity
    ));
    assert_eq!(crate::serialization::retro_serialize_size(), capacity);
}
