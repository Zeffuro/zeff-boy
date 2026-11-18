use super::*;
use crate::cartridge::{extended_ssf::ExtendedSsf, realtec::Realtec, sf001, sf002, sf004};

struct Owner {
    name: &'static str,
    cart: Cartridge,
    logical: u32,
    source: u32,
}

fn image(bytes: usize) -> Cartridge {
    let mut cart = plain();
    let mut image = cart.bytes.to_vec();
    image.resize(bytes, 0);
    cart.bytes = image.into();
    cart
}

fn eligible_name(name: &str) -> bool {
    !matches!(
        name,
        "OrBank32KiB"
            | "LockOnSaveLower"
            | "LockOnSaveRom"
            | "LockOnSaveOff"
            | "SpecialSaveExpansion"
            | "SpecialSaveBytes"
            | "SpecialSaveOdd"
            | "SpecialSaveSwitched"
            | "SpecialSaveReserved"
    )
}

fn owners() -> Vec<Owner> {
    let mut rows = vec![Owner {
        name: "Plain",
        cart: plain(),
        logical: 0x01000200,
        source: 0x200,
    }];
    for (name, save, address) in crate::cartridge::ram::profile_tests::origin::fixtures() {
        let mut cart = image(0x300000);
        cart.save = Some(save);
        rows.push(Owner {
            name,
            cart,
            logical: 0x01000000 | address,
            source: address,
        });
    }
    let mut cart = crate::cartridge::ssf2::tests::cartridge(&[]);
    assert!(
        cart.ssf2
            .as_mut()
            .unwrap()
            .control(crate::cartridge::ssf2::tests::access(0xa130f3), 9,)
    );
    rows.push(Owner {
        name: "Ssf2",
        cart,
        logical: 0x01080200,
        source: 0x480200,
    });

    let mut cart = image(0x800000);
    cart.extended_ssf = Some(ExtendedSsf::new(cart.bytes.len()));
    cart.extended_ssf.as_mut().unwrap().control(0xa130f2, 13);
    rows.push(Owner {
        name: "ExtendedSsf",
        cart,
        logical: 0x01080200,
        source: 0x680200,
    });

    for (name, revision) in [
        ("Sf001Hidden", sf001::Revision::HiddenSave),
        ("Sf001Upper", sf001::Revision::UpperSave),
    ] {
        let mut cart = Cartridge::new(sf001::tests::rom(revision)).unwrap();
        assert!(cart.sf001.as_mut().unwrap().control(0xe00, 0x80));
        rows.push(Owner {
            name,
            cart,
            logical: 0x01000200,
            source: 0x380200,
        });
    }
    for (name, mirrored, save) in [
        ("Sf002Linear", false, false),
        ("Sf002Mirrored", true, false),
        ("Sf002WithSave", true, true),
    ] {
        let mut cart = image(0x220000);
        cart.sf002 = Some(sf002::Sf002::new());
        let write = crate::cartridge::ssf2::tests::access(0xe01);
        assert!(cart.sf002.as_mut().unwrap().control(write, 0));
        if mirrored {
            assert!(cart.sf002.as_mut().unwrap().control(write, 0x81));
        }
        if save {
            cart.save = Some(crate::cartridge::ram::profile_tests::origin::sf002_save());
        }
        rows.push(Owner {
            name,
            cart,
            logical: 0x01200200,
            source: if mirrored { 0x200 } else { 0x200200 },
        });
    }
    for (name, expanded, address, source) in [
        ("Sf004Bank", false, 0x80200, 0xc0200),
        ("Sf004Expanded", true, 0x80200, 0x140200),
    ] {
        let mut cart = image(0x200000);
        cart.sf004 = Some(sf004::Sf004::new());
        let board = cart.sf004.as_mut().unwrap();
        assert!(board.write(0xf01, 0x30));
        if expanded {
            assert!(board.write(0xe01, 0xc0));
        }
        rows.push(Owner {
            name,
            cart,
            logical: 0x01000000 | address,
            source,
        });
    }
    for (name, committed, source) in [
        ("RealtecBoot", false, 0x7e200),
        ("RealtecCommitted", true, 0x40200),
    ] {
        let mut cart = image(0x80000);
        let vectors = cart.bytes[..8].to_vec();
        std::sync::Arc::make_mut(&mut cart.bytes)[0x7e000..0x7e008].copy_from_slice(&vectors);
        cart.realtec = Some(Realtec::new());
        if committed {
            let board = cart.realtec.as_mut().unwrap();
            board.write_control(0x402000, 2);
            board.write_control(0x404000, 2);
            board.write_control(0x400000, 1);
        }
        rows.push(Owner {
            name,
            cart,
            logical: 0x01000200,
            source,
        });
    }
    for (name, mapping, bytes, address, source) in [
        ("LockOnEmpty", LockOn::Empty, 0x200000, 0x100200, 0x100200),
        ("LockOnHalf", LockOn::HalfMiB, 0x280000, 0x280200, 0x200200),
        ("LockOnMiB", LockOn::MiB, 0x340000, 0x300200, 0x200200),
        ("LockOnPatch", LockOn::MiB, 0x340000, 0x300200, 0x300200),
    ] {
        let mut cart = image(bytes);
        cart.lock_on = Some(mapping);
        if name == "LockOnPatch" {
            assert!(cart.lock_on.as_mut().unwrap().write_control(1));
        }
        rows.push(Owner {
            name,
            cart,
            logical: 0x01000000 | address,
            source,
        });
    }
    for (name, marker, size) in [
        ("UnprotectedRom", b"UNPROTECTED_TEST", 0x100000),
        ("UnprotectedTime", b"UNPROTECTED_TIME", 0x200000),
    ] {
        let mut bytes = vec![0; size];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x80..0x90].copy_from_slice(marker);
        bytes[0x200..0x208].copy_from_slice(&[0x4e, 0x70, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71]);
        let cart = Cartridge::new(bytes).unwrap();
        assert!(cart.unprotected_rom.is_some());
        rows.push(Owner {
            name,
            cart,
            logical: 0x01000200,
            source: 0x200,
        });
    }
    for (name, size) in [("LinearHalf", 0x80000), ("LinearMiB", 0x100000)] {
        rows.push(Owner {
            name,
            cart: crate::cartridge::linear_rom::fixture_with_len(
                &[0x4e70, 0x4e71, 0x4e71, 0x4e71],
                size,
            ),
            logical: 0x01000200,
            source: 0x200,
        });
    }
    for (name, profile) in [
        ("FixedSequential4A", 0),
        ("FixedSequential4B", 1),
        ("FixedSequential2", 2),
        ("FixedSequential2MiB", 3),
        ("FixedSparse", 4),
        ("LatchA", 5),
        ("LatchB", 6),
        ("LatchC", 7),
        ("LatchMiB", 8),
        ("OrBank32KiB", 9),
    ] {
        let mut cart = crate::cartridge::fixed_register_fixture::cartridge(
            profile,
            &[0x4e70, 0x4e71, 0x4e71, 0x4e71],
        );
        if profile == 9 {
            cart.registers.as_mut().unwrap().write_bank(3);
        }
        rows.push(Owner {
            name,
            cart,
            logical: 0x01000200,
            source: if profile == 9 { 0x18200 } else { 0x200 },
        });
    }
    for (name, logical, enabled) in [
        ("SaveLatchLower", 0x01000200, true),
        ("SaveLatchOffMirror", 0x200200, false),
        ("SaveLatchOutside", 0x210200, true),
    ] {
        let mut bytes =
            crate::cartridge::save_latch::fixture_image(&[0x4e70, 0x4e71, 0x4e71, 0x4e71]);
        let source = if logical == 0x210200 { 0x10200 } else { 0x200 };
        bytes[source..source + 8]
            .copy_from_slice(&[0x4e, 0x70, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71]);
        let mut cart = Cartridge::new(bytes).unwrap();
        cart.save.as_mut().unwrap().enabled = enabled;
        rows.push(Owner {
            name,
            cart,
            logical,
            source: source as u32,
        });
    }
    for (name, logical, source, enabled) in [
        ("LockOnSaveLower", 0x01000200, 0x200, true),
        ("LockOnSaveRom", 0x01202200, 0x202200, true),
        ("LockOnSaveOff", 0x01300200, 0x300200, false),
    ] {
        let mut bytes = crate::cartridge::registers::tests::normal_rom_fixture(LockOn::TwoMiB);
        bytes[source..source + 8]
            .copy_from_slice(&[0x4e, 0x70, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71]);
        let mut cart = crate::cartridge::registers::tests::inserted_save_fixture_with_rom(bytes);
        cart.lock_on_save.as_mut().unwrap().enabled = enabled;
        rows.push(Owner {
            name,
            cart,
            logical,
            source: source as u32,
        });
    }
    use crate::cartridge::ram::profile_tests::overlay_fixture::{Profile, matrix_cartridge};
    for (profile, name, logical, source) in [
        (
            Profile::Expansion,
            "SpecialSaveExpansion",
            0x01000200,
            0x200,
        ),
        (Profile::Bytes, "SpecialSaveBytes", 0x01200200, 0x200200),
        (Profile::Odd, "SpecialSaveOdd", 0x01000200, 0x200),
        (
            Profile::Switched,
            "SpecialSaveSwitched",
            0x01200200,
            0x200200,
        ),
        (
            Profile::Reserved,
            "SpecialSaveReserved",
            0x01200200,
            0x200200,
        ),
        (Profile::Split, "SpecialSaveSplit", 0x01300200, 0x300200),
    ] {
        rows.push(Owner {
            name,
            cart: matrix_cartridge(profile, source),
            logical,
            source: source as u32,
        });
    }
    for row in &mut rows {
        let at = row.source as usize;
        let words = [0x4e, 0x70, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71];
        if row.name == "Ssf2" {
            let mut page = vec![9; 0x208];
            page[0x200..].copy_from_slice(&words);
            crate::cartridge::ssf2::tests::page_program(&mut row.cart, 9, &page);
        } else if row.cart.linear_rom.is_some()
            || row.cart.save_latch.is_some()
            || row.name.starts_with("SpecialSave")
            || row.cart.lock_on_save.is_some()
            || row.cart.protection.is_some()
            || row.cart.registers.is_some()
        {
            assert_eq!(&row.cart.bytes[at..at + 8], &words);
        } else {
            std::sync::Arc::make_mut(&mut row.cart.bytes)[at..at + 8].copy_from_slice(&words);
        }
        if !eligible_name(row.name) {
            let mut ordinary = Cartridge::new(row.cart.bytes.to_vec()).unwrap();
            patches(&mut ordinary, row.source + 1, 0x71);
            row.cart.rom_overlay = ordinary.rom_overlay;
        } else {
            patches(&mut row.cart, row.source + 1, 0x71);
        }
        assert_eq!(
            row.cart.supports_rom_overlay(),
            eligible_name(row.name),
            "{}",
            row.name
        );
    }
    assert_eq!(rows.len(), 58);
    rows
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn every_eligible_owner_returns_sampled_storage_through_all_timed_routes() {
    for owner in owners() {
        let machine = Emulator::new_with_cartridge(
            owner.cart.clone(),
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: false,
            },
        )
        .unwrap();
        assert_eq!(
            machine.supports_rom_cheats(),
            eligible_name(owner.name),
            "{}",
            owner.name
        );
        for route in 0..3 {
            for port_history in [false, true] {
                for observed in [false, true] {
                    let mut bus = irq_bus(owner.cart.clone(), port_history);
                    let result = fetch(
                        route,
                        &mut bus,
                        access(owner.logical, ByteLanes::Both),
                        observed,
                    );
                    let input = result.cycle.result.unwrap();
                    assert_eq!(input.value, 0x4e71, "{} route{route}", owner.name);
                    assert_eq!(
                        offsets(input),
                        [
                            Some(u64::from(owner.source)),
                            Some(u64::from(owner.source + 1))
                        ],
                        "{} route{route}",
                        owner.name
                    );
                    assert_eq!(owner.cart.bytes[owner.source as usize + 1], 0x70);
                    assert_eq!(bus.cartridge.read(owner.logical & 0xffffff), Some(0x4e));
                    assert_eq!(
                        bus.cartridge.read((owner.logical & 0xffffff) + 1),
                        Some(0x71)
                    );
                    assert_eq!(result.cycle.wait_clocks, 0, "{}", owner.name);
                    assert_eq!(bus.cpu_clock, CpuClocks(4), "{}", owner.name);
                    assert_eq!(bus.now.get(), 28, "{}", owner.name);
                    assert_eq!(
                        result.samples.map(|samples| samples.levels()),
                        (observed && !port_history).then_some((4, 4)),
                        "{}",
                        owner.name
                    );
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn every_eligible_owner_consumes_the_sampled_opcode_with_current_overlay() {
    for owner in owners() {
        for concurrent_sound in [false, true] {
            let mut machine = Emulator::new_with_cartridge(
                owner.cart.clone(),
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound,
                },
            )
            .unwrap();
            assert_eq!(
                machine.supports_rom_cheats(),
                eligible_name(owner.name),
                "{}",
                owner.name
            );
            machine.cpu.registers.pc = owner.logical;
            if concurrent_sound {
                machine
                    .cpu
                    .prime(&mut ScheduledBus::new(
                        &mut machine.bus,
                        &mut machine.sound_cpu,
                        &mut machine.sound_schedule,
                    ))
                    .unwrap();
            } else {
                machine.cpu.prime(&mut machine.bus).unwrap();
            }
            let queued = machine.cpu.queued_instruction_inputs().unwrap()[0];
            assert_eq!(queued.value, 0x4e71, "{}", owner.name);
            assert_eq!(
                offsets(queued),
                [
                    Some(u64::from(owner.source)),
                    Some(u64::from(owner.source + 1))
                ],
                "{}",
                owner.name
            );
            let mut capture = Capture::default();
            let start = machine.cpu_clock().0;
            let outcome = machine.step_instruction_traced(&mut capture).unwrap();
            let TimedOutcome::Instruction(step) = outcome.cpu else {
                panic!("{} did not execute", owner.name);
            };
            assert_eq!(step.opcode, 0x4e71, "{}", owner.name);
            assert_eq!(step.exception, None, "{}", owner.name);
            assert_eq!(machine.cpu_clock().0 - start, 4, "{}", owner.name);
            assert_eq!(capture.inputs, [(owner.logical, queued)], "{}", owner.name);
            assert_eq!(capture.words, [(owner.logical, 0x4e71)], "{}", owner.name);
            assert!(capture.memory.is_empty(), "{}", owner.name);
            assert_eq!(machine.registers().pc, owner.logical + 2, "{}", owner.name);
        }
    }
}
