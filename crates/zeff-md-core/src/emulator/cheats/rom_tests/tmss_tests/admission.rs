use super::*;
use crate::cartridge::{Cartridge, registers::LockOn};

fn config(region: Region, concurrent_sound: bool) -> LoadConfig {
    LoadConfig {
        region,
        concurrent_sound,
    }
}

fn save_image() -> Vec<u8> {
    let mut rom = image(&[0x60fe, 0x4e71]);
    rom[0x1b0..0x1bc]
        .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    rom
}

fn download_image() -> Vec<u8> {
    let mut rom = image(&[0x60fe, 0x4e71]);
    rom.resize(0x300000, 0);
    rom[0x80..0x90].copy_from_slice(b"DOWNLOAD_RAM_TST");
    rom[0x1b0..0x1bc].fill(b' ');
    rom
}

fn refused(mut machine: Emulator) {
    assert!(!machine.supports_rom_cheats());
    let before = machine.clone();
    assert!(
        machine
            .install_rom_patches(&[patch(DATA, 0xa5), patch(0, 0)])
            .is_err()
    );
    assert_eq!(machine, before);
}

pub(super) fn check(region: Region, concurrent: bool) {
    let (_, firmware) = images(&[0x60fe, 0x4e71]);
    for size in [8, 0x400000] {
        let mut rom = vec![0; size];
        rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
        let address = if size == 8 { 7 } else { size - 1 };
        let value = if size == 8 { 2 } else { 0xa5 };
        let mut m =
            Emulator::new_with_tmss(rom, config(region, concurrent), firmware.clone()).unwrap();
        assert!(m.supports_rom_cheats());
        let before = states(&m);
        m.install_rom_patches(&[patch(address as u32, value)])
            .unwrap();
        assert_eq!(m.bus.cartridge.read(address as u32), Some(value));
        assert_eq!(states(&m), before);
        let accepted = m.clone();
        for invalid in [size as u32, 0x400000, 0x1000000, u32::MAX] {
            assert!(
                m.install_rom_patches(&[patch(address as u32, value), patch(invalid, 1)])
                    .is_err()
            );
            assert_eq!(m, accepted);
        }
    }
    for size in [0, 7, 0x400001] {
        let mut rom = image(&[0x60fe, 0x4e71]);
        rom.resize(size, 0);
        assert!(
            Emulator::new_with_tmss(rom, config(region, concurrent), firmware.clone()).is_err()
        );
    }
    let mut extended = image(&[0x60fe, 0x4e71]);
    extended.resize(0x600000, 0);
    extended[0x100..0x108].copy_from_slice(b"SEGA SSF");
    assert!(
        Cartridge::new(extended.clone())
            .unwrap()
            .extended_ssf
            .is_some()
    );
    let mut factory_negatives = vec![
        save_image(),
        download_image(),
        crate::cartridge::save_latch::fixture_image(&[0x60fe, 0x4e71]),
        extended,
    ];
    for revision in [
        crate::cartridge::sf001::Revision::HiddenSave,
        crate::cartridge::sf001::Revision::UpperSave,
    ] {
        factory_negatives.push(crate::cartridge::sf001::tests::rom(revision));
    }
    for profile in 0..9 {
        factory_negatives.push(
            crate::cartridge::fixed_register_fixture::cartridge(profile, &[0x60fe, 0x4e71])
                .bytes
                .to_vec(),
        );
    }
    for rom in factory_negatives {
        assert!(
            Emulator::new_with_tmss(rom, config(region, concurrent), firmware.clone()).is_err()
        );
    }
    for owner in 0..19 {
        let mut m = machine(&[0x60fe, 0x4e71], region, concurrent);
        let cart = &mut m.bus.cartridge;
        match owner {
            0 => cart.save = Cartridge::new(save_image()).unwrap().save,
            1 => {
                cart.save_latch = Cartridge::new(crate::cartridge::save_latch::fixture_image(&[
                    0x60fe, 0x4e71,
                ]))
                .unwrap()
                .save_latch
            }
            2 => cart.download = Cartridge::new(download_image()).unwrap().download,
            3 => {
                cart.linear_rom =
                    crate::cartridge::linear_rom::fixture(&[0x60fe, 0x4e71]).linear_rom
            }
            4 => {
                let mut rom = image(&[0x60fe, 0x4e71]);
                rom.resize(0x100000, 0);
                rom[0x80..0x90].copy_from_slice(b"UNPROTECTED_TEST");
                cart.unprotected_rom = Cartridge::new(rom).unwrap().unprotected_rom;
            }
            5 => {
                cart.parallel_adapter =
                    Some(crate::cartridge::parallel_adapter::ParallelAdapter::new(
                        crate::cartridge::parallel_adapter::Model::First,
                    ))
            }
            6 => cart.registers = Some(Default::default()),
            7 => {
                cart.protection =
                    crate::cartridge::fixed_register_fixture::cartridge(0, &[0x60fe, 0x4e71])
                        .protection
            }
            8 => {
                cart.sf001 = Some(crate::cartridge::sf001::Sf001::new(
                    crate::cartridge::sf001::Revision::HiddenSave,
                ))
            }
            9 => cart.sf002 = Some(crate::cartridge::sf002::Sf002::new()),
            10 => cart.sf004 = Some(crate::cartridge::sf004::Sf004::new()),
            11 => {
                cart.extended_ssf = Some(crate::cartridge::extended_ssf::ExtendedSsf::new(0x600000))
            }
            12 => cart.realtec = Some(crate::cartridge::realtec::Realtec::new()),
            13 => cart.ssf2 = Some(crate::cartridge::ssf2::Ssf2::new()),
            14 => cart.lock_on = Some(LockOn::Empty),
            15 => cart.lock_on_save = Some(crate::cartridge::registers::LockOnSave::new()),
            16 => {
                cart.svp = Some(Box::new(
                    crate::svp::InstructionModel::new_synthetic(vec![0; 4], vec![0; 1024]).unwrap(),
                ))
            }
            17 => {
                cart.realtec = Some(crate::cartridge::realtec::Realtec::new());
                cart.save = Cartridge::new(save_image()).unwrap().save;
            }
            18 => Arc::make_mut(&mut cart.bytes)[DATA as usize] ^= 1,
            _ => unreachable!(),
        }
        refused(m);
    }
    if concurrent {
        let mut precise = fixture(&[0x60fe, 0x4e71], TimingModel::Precise, region);
        precise.bus.tmss = machine(&[0x60fe, 0x4e71], region, true).bus.tmss;
        refused(precise);
    }
}
