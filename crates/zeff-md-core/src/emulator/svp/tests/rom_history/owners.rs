use super::*;

fn assert_rejected(donor: &Emulator, raw: &[u8]) {
    let frontend = bad_front(raw, &donor.encode_frontend_state().unwrap());
    let owned = bad_owned(&frontend, &donor.encode_debugger_checkpoint().unwrap());
    for wrapper in 0..3 {
        let mut receiver = donor.clone();
        install_current(&mut receiver, Some(0xb6));
        let before = receiver.clone();
        let result = match wrapper {
            0 => receiver.decode_state(raw),
            1 => receiver.decode_frontend_state(&frontend),
            _ => receiver.decode_debugger_checkpoint(&owned),
        };
        assert!(result.is_err());
        assert_eq!(receiver, before);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn historical_owner_markers_missing_witness_and_all_legacy_outer_reject_atomically() {
    let mut donor = historical(Region::NtscUs, true);
    donor.install_rom_patches(&[patch(0x1001, 0xa5)]).unwrap();
    donor.bus.sound.set_reset(false);
    donor.sound_cpu = zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x9000, 0x1ffe));
    donor.sound_schedule.pending = Some(
        crate::hardware::sound::schedule::PendingSound::prepare(
            &donor.sound_cpu,
            &donor.bus.sound,
            &donor.bus.cartridge,
            donor.bus.now.get(),
        )
        .unwrap(),
    );
    let raw = donor.encode_state().unwrap();
    let component = raw.windows(8).position(|part| part == b"ZSINST02").unwrap();
    let history = raw.windows(8).position(|part| part == b"ZSVPROM2").unwrap();
    let sound = raw.windows(8).position(|part| part == b"ZSNDROM2").unwrap();
    let mut wrong = raw.clone();
    wrong[component..component + 8].copy_from_slice(b"ZSINST01");
    assert_rejected(&donor, &wrong);
    let mut missing = raw.clone();
    missing[history + 8] = 0;
    assert_rejected(&donor, &missing);
    let mut wrong = raw.clone();
    wrong[sound..sound + 8].copy_from_slice(b"ZSNDLEG1");
    assert_rejected(&donor, &wrong);
    let mut r = StateReader::new(&raw);
    r.set_position(sound + 16);
    let mut cpu = donor.sound_cpu.clone();
    let mut state = zeff_z80::BusState::default();
    cpu.read_continuation(&mut state, &mut r).unwrap();
    r.read_bool().unwrap();
    r.read_u8().unwrap();
    r.read_bool().unwrap();
    r.read_u8().unwrap();
    r.read_bool().unwrap();
    r.read_u64().unwrap();
    crate::hardware::sound::SoundMemory::read_state(&mut r).unwrap();
    let count = r.position();
    assert_eq!(raw[count], 2);
    let mut missing = raw.clone();
    missing[count] = 0;
    assert_rejected(&donor, &missing);
    let mut extra = raw.clone();
    extra[count] = 7;
    assert_rejected(&donor, &extra);

    let plain = fixture(Region::NtscUs, false);
    let mut all_legacy = plain.encode_state().unwrap();
    let outer = all_legacy
        .windows(8)
        .position(|part| part == b"ZMDSVPS1")
        .unwrap();
    all_legacy[outer..outer + 8].copy_from_slice(b"ZMDSVPA2");
    all_legacy.extend_from_slice(&0_u32.to_le_bytes());
    let mut receiver = plain.clone();
    install_current(&mut receiver, Some(0xb6));
    let before = receiver.clone();
    assert!(receiver.decode_state(&all_legacy).is_err());
    assert_eq!(receiver, before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn functional_serviced_port_prefix_restores_sampled_read_after_current_mask_clears() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for sound in [false, true] {
            let base = fixture(region, sound);
            let mut rom = base.rom_bytes().to_vec();
            let code = [
                0x08e0_u16, 0x500, 0x08e0, 0x0800, 0x000c, 0x00b8, 0x001b, 0x4c00, 0x405,
            ];
            for (index, word) in code.into_iter().enumerate() {
                rom[0x800 + index * 2..0x802 + index * 2].copy_from_slice(&word.to_be_bytes());
            }
            let mut irom = vec![0; 1024];
            irom[0x3fc] = 0x400;
            let mut donor = Emulator::new_functional_svp(
                rom,
                LoadConfig {
                    region,
                    concurrent_sound: sound,
                },
                irom,
                FunctionalProfile::new(0x507, 1, 3, [1, 1, 64, 2, 1, 1]).unwrap(),
            )
            .unwrap();
            donor.install_rom_patches(&[patch(0x80b, 0xbc)]).unwrap();
            for _ in 0..10000 {
                if donor
                    .synthetic_svp()
                    .unwrap()
                    .pending_service()
                    .unwrap()
                    .is_some_and(|request| {
                        request.role == ServiceRole::PortRead(4)
                            && request.resource == crate::svp::ServiceResource::SharedRom
                    })
                {
                    break;
                }
                advance(&mut donor, 1);
            }
            let request = donor
                .synthetic_svp()
                .unwrap()
                .pending_service()
                .unwrap()
                .unwrap();
            assert_eq!(
                (
                    request.role,
                    request.resource,
                    request.address,
                    request.ordinal
                ),
                (
                    ServiceRole::PortRead(4),
                    crate::svp::ServiceResource::SharedRom,
                    0x500,
                    1
                )
            );
            donor
                .install_rom_patches(&[patch(0xa00, 0xab), patch(0xa01, 0xcd)])
                .unwrap();
            for _ in 0..192 {
                advance(&mut donor, 1);
                if donor
                    .synthetic_svp()
                    .unwrap()
                    .pending_service()
                    .unwrap()
                    .is_some_and(|request| request.role == ServiceRole::PortWrite(3))
                {
                    break;
                }
            }
            let request = donor
                .synthetic_svp()
                .unwrap()
                .pending_service()
                .unwrap()
                .unwrap();
            assert_eq!(
                (request.role, request.ordinal, request.value),
                (ServiceRole::PortWrite(3), 2, 0xabcd)
            );
            assert_eq!(donor.synthetic_svp().unwrap().peek_result(), 0);
            donor.clear_rom_patches();
            for wrapper in 0..3 {
                let mut receiver = donor.clone();
                receiver
                    .install_rom_patches(&[patch(0xa00, 0x55), patch(0xa01, 0xaa)])
                    .unwrap();
                restore(&mut receiver, &donor, wrapper);
                assert_eq!(
                    receiver.synthetic_svp().unwrap().pending_service().unwrap(),
                    Some(request)
                );
                advance(&mut receiver, 3);
                assert_eq!(receiver.synthetic_svp().unwrap().peek_result(), 0xabcd);
                assert_eq!(receiver.peek(0xa00), Some(0x55));
                assert_eq!(receiver.peek(0xa01), Some(0xaa));
            }
        }
    }
}
