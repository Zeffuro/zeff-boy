#[path = "support.rs"]
mod support;

use support::*;
use zeff_md_core::{
    ControlCommand, ControlWrite, Emulator, LoadConfig, VdpPortError, VdpRegisterError,
};

pub fn register_writes_abandon_incomplete_fetches_at_scheduled_boundaries() {
    for region in REGIONS {
        for h40 in [false, true] {
            for active in [false, true] {
                let seeded = fixture(region, h40, active);
                for word in [
                    0x8020,
                    0x8164,
                    0x8f02,
                    0x8f04,
                    0x9800,
                    0x9f00,
                    if active { 0x8104 } else { 0x8144 },
                    if h40 { 0x8c00 } else { 0x8c81 },
                ] {
                    for boundary in 0..6 {
                        let mut vdp = seeded.clone();
                        control(&mut vdp, 0x3ffe, 0, region);
                        let start = vdp.now();
                        let first = first_slot(&vdp, region);
                        let end = vdp.data_read_wait(region).unwrap().unwrap();
                        assert!(start < first && first < end);
                        let at = [
                            start,
                            tick(first.get() - 1),
                            first,
                            tick(end.get() - 1),
                            end,
                            tick(end.get() + 1),
                        ][boundary];
                        vdp.advance_to(at, region).unwrap();
                        let completed = boundary >= 4;
                        assert_eq!(vdp.read_cache().is_ready(), completed);
                        assert_eq!(
                            vdp.command().address,
                            if completed { 0x4000 } else { 0x3ffe }
                        );
                        let buffered = if completed { 0xe781 } else { 0x369c };
                        assert_eq!(vdp.read_cache().buffered_word(), Some(buffered));
                        let memory = vdp.memory().clone();
                        let fifo = vdp.fifo().clone();
                        let index = ((word >> 8) & 0x1f) as u8;
                        let old_registers = *vdp.registers().registers();
                        assert_eq!(
                            vdp.write_control(word, region),
                            Ok(ControlWrite::Register {
                                index,
                                value: word as u8
                            })
                        );
                        assert_eq!(vdp.now(), at);
                        assert_eq!(vdp.registers().registers(), &old_registers);
                        let publication_at = tick((at.get() / 20 + 1) * 20);
                        if matches!(index, 24 | 31) {
                            assert_eq!(vdp.registers().pending(), None);
                        } else {
                            let publication = vdp.registers().pending().unwrap();
                            assert_eq!(publication.publish_at, publication_at);
                            assert_eq!(publication.index, index);
                            assert_eq!(publication.value, word as u8);
                        }
                        assert!(!vdp.read_cache().is_ready());
                        assert_eq!(vdp.read_cache().buffered_word(), Some(buffered));
                        let address = if completed { 0x4000 } else { 0 } | u32::from(word & 0x3fff);
                        assert_eq!(vdp.command().address, address);
                        assert_eq!(vdp.control().pending_word(), None);
                        assert_eq!(vdp.memory(), &memory);
                        assert_eq!(vdp.fifo(), &fifo);
                        vdp.advance_to(tick(publication_at.get() - 1), region)
                            .unwrap();
                        assert_eq!(vdp.registers().registers(), &old_registers);
                        vdp.advance_to(publication_at, region).unwrap();
                        if matches!(index, 24 | 31) {
                            assert_eq!(vdp.registers().registers(), &old_registers);
                        } else {
                            assert_eq!(
                                vdp.registers().registers()[usize::from(index)],
                                Some(word as u8)
                            );
                        }
                        vdp.advance_to(tick(end.get().max(vdp.now().get()) + 3420), region)
                            .unwrap();
                        assert_eq!(vdp.command().address, address);
                        assert_eq!(vdp.read_cache().buffered_word(), Some(buffered));
                        assert!(!vdp.read_cache().is_ready());
                        assert_eq!(vdp.memory(), &memory);
                        fresh_read(&mut vdp, region, if word == 0x8f04 { 4 } else { 2 });
                    }
                }
            }
        }
    }
}

pub fn parser_priority_and_rejected_registers_preserve_transactions() {
    for region in REGIONS {
        for h40 in [false, true] {
            for active in [false, true] {
                let seeded = fixture(region, h40, active);
                for phase in 0..3 {
                    let mut vdp = seeded.clone();
                    control(&mut vdp, 0x3ffe, 0, region);
                    let first = first_slot(&vdp, region);
                    let end = vdp.data_read_wait(region).unwrap().unwrap();
                    if phase != 0 {
                        vdp.advance_to(if phase == 1 { first } else { end }, region)
                            .unwrap();
                    }
                    let before = vdp.clone();
                    assert_eq!(
                        vdp.queue_register(24, 0, region),
                        Err(VdpRegisterError::UnsupportedRegister(24).into())
                    );
                    assert_eq!(vdp, before);
                    for word in [0x8100, 0x8c03, 0x8c80, 0x8c04] {
                        let before = vdp.clone();
                        let error = VdpRegisterError::UnsupportedModeChange {
                            index: ((word >> 8) & 0x1f) as u8,
                            value: word as u8,
                        };
                        assert_eq!(
                            vdp.write_control(word, region),
                            Err(VdpPortError::Schedule(error.into()))
                        );
                        assert_eq!(vdp, before);
                        let mut advanceable = vdp.clone();
                        advanceable.advance_to(end, region).unwrap();
                        assert!(advanceable.read_cache().is_ready());
                        assert_eq!(advanceable.command().address, 0x4000);
                    }
                    let mut busy = vdp.clone();
                    if busy.now().get().is_multiple_of(20) {
                        busy.advance_to(tick(busy.now().get() + 1), region).unwrap();
                    }
                    busy.queue_register(0, 0x20, region).unwrap();
                    let pending = busy.registers().pending().unwrap();
                    let before = busy.clone();
                    assert!(busy.write_control(0x8f04, region).is_err());
                    assert_eq!(busy, before);
                    assert_eq!(
                        busy.write_control(0x9f00, region),
                        Err(VdpPortError::Schedule(
                            VdpRegisterError::UnsupportedRegister(31).into()
                        ))
                    );
                    assert_eq!(busy, before);
                    busy.advance_to(tick(end.get().max(pending.publish_at.get())), region)
                        .unwrap();
                    assert!(busy.read_cache().is_ready());
                    assert_eq!(busy.command().address, 0x4000);
                    let mut odd = vdp.clone();
                    odd.write_control(0x8f01, region).unwrap();
                    let publication = odd.registers().pending().unwrap();
                    odd.advance_to(publication.publish_at, region).unwrap();
                    assert_eq!(odd.registers().auto_increment(), Some(1));
                    odd.write_control(0x0241, region).unwrap();
                    let before = odd.clone();
                    assert_eq!(
                        odd.write_control(0, region),
                        Ok(ControlWrite::Command(ControlCommand {
                            address: 0x241,
                            code: 0,
                        }))
                    );
                    assert_eq!(odd.memory(), before.memory());
                    assert_eq!(odd.fifo(), before.fifo());
                    let expected = u16::from_be_bytes([
                        odd.memory().vram()[0x240],
                        odd.memory().vram()[0x241],
                    ]);
                    let end = odd.data_read_wait(region).unwrap().unwrap();
                    odd.advance_to(end, region).unwrap();
                    assert_eq!(odd.read_data(region), Ok(expected));
                    odd.write_control(0x0241, region).unwrap();
                    assert_eq!(
                        odd.write_control(0x0040, region),
                        Ok(ControlWrite::Command(ControlCommand {
                            address: 0x241,
                            code: 0x10,
                        }))
                    );
                    assert_eq!(odd.control().pending_word(), None);
                    assert_eq!(odd.registers().auto_increment(), Some(1));
                    odd.write_control(0x8f02, region).unwrap();
                    let publication = odd.registers().pending().unwrap();
                    odd.advance_to(publication.publish_at, region).unwrap();
                    fresh_read(&mut odd, region, 2);
                    if phase != 2 {
                        for (index, value) in [
                            (1, if active { 4 } else { 0x44 }),
                            (12, if h40 { 0 } else { 0x81 }),
                            (15, 2),
                            (15, 4),
                        ] {
                            let before = vdp.clone();
                            assert_eq!(
                                vdp.queue_register(index, value, region),
                                Err(VdpRegisterError::UnsupportedModeChange { index, value }.into())
                            );
                            assert_eq!(vdp, before);
                        }
                    }
                    vdp.write_control(0, region).unwrap();
                    let registers = *vdp.registers().registers();
                    vdp.advance_to(tick(end.get() + 3420), region).unwrap();
                    assert_eq!(vdp.control().pending_word(), Some(0));
                    assert!(!vdp.read_cache().is_ready());
                    assert_eq!(
                        vdp.write_control(0x9f02, region),
                        Ok(ControlWrite::Command(ControlCommand {
                            address: 0x8000,
                            code: 0
                        }))
                    );
                    assert_eq!(vdp.registers().registers(), &registers);
                    assert_eq!(vdp.registers().pending(), None);
                    let first = first_slot(&vdp, region);
                    let end = vdp.data_read_wait(region).unwrap().unwrap();
                    vdp.advance_to(first, region).unwrap();
                    assert!(!vdp.read_cache().is_ready());
                    vdp.advance_to(end, region).unwrap();
                    assert_eq!(vdp.read_data(region), Ok(0));
                }
            }
        }
    }
}

pub fn cpu_register_writes_and_restored_continuations_agree() {
    for region in REGIONS {
        for concurrent_sound in [false, true] {
            for h40 in [false, true] {
                for word in [0x8164, 0x8f02, 0x8f04, 0x9f00] {
                    let mut phases = 0;
                    'seeds: for lead_padding in 0..16 {
                        for padding in 0..24 {
                            let mut words = vec![
                                0x33fc,
                                if h40 { 0x8c81 } else { 0x8c00 },
                                0x00c0,
                                4,
                                0x33fc,
                                0x8144,
                                0x00c0,
                                4,
                                0x33fc,
                                0x8f02,
                                0x00c0,
                                4,
                            ];
                            words.extend(std::iter::repeat_n(0x4e71, lead_padding));
                            words.extend([0x23fc, 0x3ffe, 0, 0x00c0, 4]);
                            words.extend(std::iter::repeat_n(0x4e71, padding));
                            words.extend([
                                0x33fc, word, 0x00c0, 4, 0x23fc, 0x4240, 0, 0x00c0, 4, 0x33fc,
                                0x12ab, 0x00c0, 0, 0x23fc, 0x0240, 0, 0x00c0, 4, 0x3039, 0x00c0, 0,
                                0x60fe,
                            ]);
                            let mut machine = Emulator::new(
                                image(&words),
                                LoadConfig {
                                    region,
                                    concurrent_sound,
                                },
                            )
                            .unwrap();
                            for _ in 0..4 + lead_padding + padding {
                                machine.step_instruction().unwrap();
                            }
                            let entry_phase = phase(machine.vdp(), region);
                            if phases & (1 << entry_phase) != 0 {
                                continue;
                            }
                            phases |= 1 << entry_phase;
                            let before = machine.encode_state().unwrap();
                            assert_eq!(&before[8..12], &1_u32.to_le_bytes());
                            let mut restored = machine.clone();
                            restored.decode_state(&before).unwrap();
                            assert_eq!(before, restored.encode_state().unwrap());
                            assert_eq!(machine.step_instruction(), restored.step_instruction());
                            assert!(!machine.vdp().read_cache().is_ready());
                            let after = machine.encode_state().unwrap();
                            assert_eq!(after, restored.encode_state().unwrap());
                            restored.decode_state(&after).unwrap();
                            for _ in 0..8 {
                                assert_eq!(machine.step_instruction(), restored.step_instruction());
                                assert_eq!(
                                    machine.encode_state().unwrap(),
                                    restored.encode_state().unwrap()
                                );
                            }
                            assert_eq!(&machine.vdp().memory().vram()[0x240..0x242], &[0x12, 0xab]);
                            assert_eq!(machine.registers().d[0] & 0xffff, 0x12ab);
                            if phases == 7 {
                                break 'seeds;
                            }
                        }
                    }
                    assert_eq!(
                        phases, 7,
                        "{region:?} h40={h40} concurrent={concurrent_sound} word={word:04x}"
                    );
                }
            }
        }
    }
}

pub fn refresh_gap_and_address_wrap_keep_only_completed_progress() {
    for region in REGIONS {
        let mut vdp = fixture(region, false, false);
        let base = (vdp.now().get() / 3420 + 1) * 3420;
        vdp.advance_to(tick(base + 509), region).unwrap();
        control(&mut vdp, 0x3ffe, 0, region);
        assert_eq!(first_slot(&vdp, region), tick(base + 510));
        assert_eq!(vdp.data_read_wait(region).unwrap(), Some(tick(base + 550)));
        vdp.advance_to(tick(base + 530), region).unwrap();
        assert_eq!(phase(&vdp, region), 1);
        vdp.write_control(0x8f02, region).unwrap();
        vdp.advance_to(tick(base + 590), region).unwrap();
        assert_eq!(vdp.command().address, 0x0f02);
        assert_eq!(vdp.read_cache().buffered_word(), Some(0x369c));
        assert!(!vdp.read_cache().is_ready());
        fresh_read(&mut vdp, region, 2);
        for h40 in [false, true] {
            for active in [false, true] {
                let seeded = fixture(region, h40, active);
                for requested_phase in 0..3 {
                    let mut vdp = seeded.clone();
                    control(&mut vdp, 0x1fffe, 0, region);
                    let first = first_slot(&vdp, region);
                    let end = vdp.data_read_wait(region).unwrap().unwrap();
                    if requested_phase != 0 {
                        vdp.advance_to(if requested_phase == 1 { first } else { end }, region)
                            .unwrap();
                    }
                    assert_eq!(phase(&vdp, region), requested_phase);
                    let completed = requested_phase == 2;
                    assert_eq!(vdp.command().address, if completed { 0 } else { 0x1fffe });
                    let memory = vdp.memory().clone();
                    vdp.write_control(0x8f02, region).unwrap();
                    let address = if completed { 0x0f02 } else { 0x1cf02 };
                    vdp.advance_to(tick(end.get() + 3420), region).unwrap();
                    assert_eq!(vdp.command().address, address);
                    assert_eq!(
                        vdp.read_cache().buffered_word(),
                        Some(if completed { 0 } else { 0x369c })
                    );
                    assert!(!vdp.read_cache().is_ready());
                    assert_eq!(vdp.memory(), &memory);
                    fresh_read(&mut vdp, region, 2);
                }
            }
        }
    }
}
