use super::*;
use crate::VdpWrite;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn register(vdp: &mut Vdp, index: u8, value: u8, region: Region) {
    vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    let write = vdp.queue_register(index, value, region).unwrap();
    vdp.advance_to(write.publish_at, region).unwrap();
}

fn command(vdp: &mut Vdp, address: u32, code: u8, region: Region) {
    vdp.write_control(
        (address as u16 & 0x3fff) | u16::from(code & 3) << 14,
        region,
    )
    .unwrap();
    vdp.write_control(
        ((address >> 14) as u16 & 7) | u16::from(code & 0x3c) << 2,
        region,
    )
    .unwrap();
    assert_eq!(vdp.command(), ControlCommand { address, code });
}

fn encoded(vdp: &Vdp, region: Region) -> Vec<u8> {
    let mut writer = StateWriter::new();
    vdp.write_state_in_region(&mut writer, region).unwrap();
    writer.into_bytes()
}

fn restored(vdp: &Vdp, region: Region) -> Vdp {
    let bytes = encoded(vdp, region);
    let mut target = Vdp::bootstrap_mode5();
    let mut reader = StateReader::new(&bytes);
    target.read_state(&mut reader, region).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(target, *vdp);
    assert_eq!(encoded(&target, region), bytes);
    target
}

fn slot(vdp: &Vdp, region: Region) -> MasterTicks {
    vdp.state
        .interrupts
        .next_write_slot(vdp.now(), region, &vdp.state.registers)
        .unwrap()
}

fn fixture(region: Region, horizontal: u8, increment: u8) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    for (index, value) in [
        (AUTO_INCREMENT_REGISTER, increment),
        (DISPLAY_SIZE_REGISTER, horizontal),
        (SPRITE_TABLE_REGISTER, 0),
        (
            MODE_REGISTER,
            MODE5_ENABLE | DISPLAY_ENABLE | if region == Region::Pal { V30_ENABLE } else { 0 },
        ),
    ] {
        register(&mut vdp, index, value, region);
    }
    let mut vdp =
        Vdp::from_stable_display_state(*vdp.interrupts(), *vdp.registers(), region).unwrap();
    vdp.advance_to(tick(20 * H32_LINE_TICKS + 101), region)
        .unwrap();
    for (address, data) in [
        (0, 0x369c),
        (0x8000, 0xa135),
        (0x8002, 0x7ed2),
        (0x1fffe, 0xb246),
    ] {
        vdp.memory
            .write_word(
                ControlCommand {
                    address,
                    code: VRAM_WRITE_CODE,
                },
                data,
            )
            .unwrap();
    }
    let mut previous = ControlCommand {
        address: 0,
        code: VRAM_READ_CODE,
    };
    vdp.state
        .cache
        .complete_read(&vdp.memory, &vdp.state.fifo, &mut previous, Some(2))
        .unwrap();
    vdp.invalidate_interval();
    vdp
}

pub(crate) fn even_fetch_odd_increment_phases_and_codec() {
    for region in [Region::NtscUs, Region::Pal] {
        for horizontal in [0, H40_CLOCK_MASK] {
            for code in [0x14, 0x18, 0x1c] {
                let mut vdp = fixture(region, horizontal, 1);
                command(&mut vdp, 0x8000, code, region);
                assert_eq!(vdp.state.read_phase, None);
                assert!(!vdp.read_cache().is_ready());
                assert_eq!(vdp.read_cache().buffered_word(), Some(0x369c));
                let memory = vdp.memory.clone();
                let mut copy = restored(&vdp, region);
                let idle = tick(vdp.now().get() + H32_LINE_TICKS);
                vdp.advance_to(idle, region).unwrap();
                copy.advance_to(idle, region).unwrap();
                assert_eq!(vdp, copy);
                assert_eq!(
                    vdp.command(),
                    ControlCommand {
                        address: 0x8000,
                        code
                    }
                );
                assert_eq!(vdp.state.read_phase, None);
                assert!(!vdp.read_cache().is_ready());
                assert_eq!(vdp.memory, memory);
                for target in [&mut vdp, &mut copy] {
                    command(target, 0x8000, VRAM_READ_CODE, region);
                }
                assert_eq!(vdp, copy);
                copy = restored(&copy, region);
                let end = vdp.data_read_wait(region).unwrap().unwrap();
                vdp.advance_to(end, region).unwrap();
                copy.advance_to(end, region).unwrap();
                assert_eq!(vdp, copy);
                assert_eq!(vdp.command().address, 0x8001);
                assert_eq!(vdp.state.read_phase, None);
                assert!(vdp.read_cache().is_ready());
                assert_eq!(vdp.read_cache().buffered_word(), Some(0xa135));
                restored(&vdp, region);
            }
            for increment in [1, 0xff] {
                for (address, expected) in [
                    (0x8000, 0xa135),
                    (0x8001, 0xa135),
                    (0x1fffe, 0xb246),
                    (0x1ffff, 0xb246),
                ] {
                    let mut vdp = fixture(region, horizontal, increment);
                    command(&mut vdp, address, VRAM_READ_CODE, region);
                    assert_eq!(vdp.state.read_phase, Some(false));
                    let first = slot(&vdp, region);
                    let end = vdp.data_read_wait(region).unwrap().unwrap();
                    assert!(first < end);
                    let next = (address + u32::from(increment)) & COMMAND_ADDRESS_MASK;
                    let mut copy = restored(&vdp, region);
                    for at in [tick(first.get() - 1), first, tick(end.get() - 1), end] {
                        vdp.advance_to(at, region).unwrap();
                        copy.advance_to(at, region).unwrap();
                        assert_eq!(vdp, copy);
                        let complete = at == end;
                        assert_eq!(vdp.command().address, if complete { next } else { address });
                        assert_eq!(
                            vdp.state.read_phase,
                            if complete { None } else { Some(at >= first) }
                        );
                        assert_eq!(vdp.read_cache().is_ready(), complete);
                        assert_eq!(
                            vdp.read_cache().buffered_word(),
                            Some(if complete { expected } else { 0x369c })
                        );
                        copy = restored(&vdp, region);
                    }
                    let memory = vdp.memory.clone();
                    let at = tick(end.get() + H32_LINE_TICKS);
                    vdp.advance_to(at, region).unwrap();
                    copy.advance_to(at, region).unwrap();
                    assert_eq!(vdp, copy);
                    assert_eq!(vdp.memory, memory);
                    assert_eq!(vdp.command().address, next);
                    assert_eq!(vdp.state.read_phase, None);
                    assert_eq!(vdp.data_read_wait(region), Ok(None));
                    assert_eq!(vdp.read_data(region).unwrap(), expected);
                    assert_eq!(copy.read_data(region).unwrap(), expected);
                    assert_eq!(vdp, copy);
                    let first = slot(&vdp, region);
                    let end = vdp.data_read_wait(region).unwrap().unwrap();
                    let expected_next = match (increment, address) {
                        (1, 0x8000) => 0xa135,
                        (1, 0x8001) => 0x7ed2,
                        (1, 0x1fffe) => 0xb246,
                        (1, 0x1ffff) => 0x369c,
                        (0xff, _) => 0,
                        _ => unreachable!(),
                    };
                    for at in [first, end] {
                        vdp.advance_to(at, region).unwrap();
                        copy.advance_to(at, region).unwrap();
                        assert_eq!(vdp, copy);
                        copy = restored(&copy, region);
                    }
                    assert_eq!(vdp.read_cache().buffered_word(), Some(expected_next));
                    assert_eq!(
                        vdp.command().address,
                        (next + u32::from(increment)) & COMMAND_ADDRESS_MASK
                    );
                }
            }
        }
    }
}

pub(crate) fn cancellation_keeps_only_completed_progress() {
    for region in [Region::NtscUs, Region::Pal] {
        for horizontal in [0, H40_CLOCK_MASK] {
            for increment in [1, 0xff] {
                let mut seed = fixture(region, horizontal, increment);
                command(&mut seed, 0x8000, VRAM_READ_CODE, region);
                let first = slot(&seed, region);
                let end = seed.data_read_wait(region).unwrap().unwrap();
                for at in [
                    seed.now(),
                    tick(first.get() - 1),
                    first,
                    tick(end.get() - 1),
                    end,
                    tick(end.get() + 1),
                ] {
                    for replacement in [false, true] {
                        let mut vdp = seed.clone();
                        vdp.advance_to(at, region).unwrap();
                        let complete = at >= end;
                        let buffered = Some(if complete { 0xa135 } else { 0x369c });
                        assert_eq!(
                            vdp.command().address,
                            if complete {
                                0x8000 + u32::from(increment)
                            } else {
                                0x8000
                            }
                        );
                        let memory = vdp.memory.clone();
                        if replacement {
                            command(&mut vdp, 0x240, VRAM_WRITE_CODE, region);
                        } else {
                            vdp.write_control(0x8f02, region).unwrap();
                            let write = vdp.registers().pending().unwrap();
                            assert_eq!(vdp.registers().auto_increment(), Some(increment));
                            let mut copy = restored(&vdp, region);
                            vdp.advance_to(write.publish_at, region).unwrap();
                            copy.advance_to(write.publish_at, region).unwrap();
                            assert_eq!(vdp, copy);
                            assert_eq!(vdp.registers().auto_increment(), Some(2));
                        }
                        assert_eq!(vdp.state.read_phase, None);
                        assert!(!vdp.read_cache().is_ready());
                        assert_eq!(vdp.read_cache().buffered_word(), buffered);
                        let address = vdp.command().address;
                        let mut copy = restored(&vdp, region);
                        let after = tick(vdp.now().get().max(end.get()) + H32_LINE_TICKS);
                        vdp.advance_to(after, region).unwrap();
                        copy.advance_to(after, region).unwrap();
                        assert_eq!(vdp, copy);
                        assert_eq!(vdp.memory, memory);
                        assert_eq!(vdp.command().address, address);
                        assert_eq!(vdp.read_cache().buffered_word(), buffered);
                    }
                }
            }
        }
    }
}

pub(crate) fn fifo_words_retire_before_both_read_slots() {
    VdpWriteFifo::verify_byte_order_and_legacy_migration();
    odd_word_publication_preserves_cache_until_retirement();
    for region in [Region::NtscUs, Region::Pal] {
        for horizontal in [0, H40_CLOCK_MASK] {
            for address in [0x8000, 0x8001] {
                let mut vdp = fixture(region, horizontal, 1);
                command(&mut vdp, 0x8000, VRAM_WRITE_CODE, region);
                vdp.write_data(0xd26b, region).unwrap();
                command(&mut vdp, address, VRAM_READ_CODE, region);
                let mut copy = restored(&vdp, region);
                for index in 0..4 {
                    let at = slot(&vdp, region);
                    vdp.advance_to(at, region).unwrap();
                    copy.advance_to(at, region).unwrap();
                    assert_eq!(vdp, copy);
                    assert_eq!(
                        vdp.state.read_phase,
                        match index {
                            0 | 1 => Some(false),
                            2 => Some(true),
                            _ => None,
                        }
                    );
                    assert_eq!(vdp.read_cache().is_ready(), index == 3);
                    assert_eq!(
                        vdp.command().address,
                        if index == 3 { address + 1 } else { address }
                    );
                    assert_eq!(
                        &vdp.memory.vram()[0x8000..0x8002],
                        if index == 0 {
                            &[0xa1, 0x6b]
                        } else {
                            &[0xd2, 0x6b]
                        }
                    );
                    copy = restored(&vdp, region);
                }
                assert!(vdp.fifo().is_empty());
                assert_eq!(vdp.read_cache().buffered_word(), Some(0xd26b));
            }
        }
    }
}

fn odd_word_publication_preserves_cache_until_retirement() {
    for region in [Region::NtscUs, Region::Pal] {
        for horizontal in [0, H40_CLOCK_MASK] {
            let mut vdp = fixture(region, horizontal, 2);
            let initial = VdpWrite {
                command: ControlCommand {
                    address: 0,
                    code: VRAM_WRITE_CODE,
                },
                data: 0x369c,
            };
            vdp.state
                .sprites
                .complete_write(&mut vdp.memory, &vdp.state.registers, initial)
                .unwrap();
            let sprites = vdp.state.sprites;
            assert_eq!(sprites.entry_words(0).unwrap()[0], Some(0x029c));
            command(&mut vdp, 1, VRAM_WRITE_CODE, region);
            vdp.write_data(0xabcd, region).unwrap();
            vdp.advance_to(slot(&vdp, region), region).unwrap();
            assert_eq!(&vdp.memory.vram()[0..2], &[0xcd, 0x9c]);
            assert_eq!(vdp.state.sprites, sprites);
            assert_eq!(vdp.fifo().len(), 1);
            vdp = restored(&vdp, region);
            vdp.advance_to(slot(&vdp, region), region).unwrap();
            assert_eq!(&vdp.memory.vram()[0..2], &[0xcd, 0xab]);
            assert_eq!(vdp.state.sprites.entry_words(0).unwrap()[0], None);
            assert!(vdp.fifo().is_empty());
        }
    }
}

fn malformed_offsets(bytes: &[u8], region: Region) -> (usize, usize, usize) {
    let mut reader = StateReader::new(bytes);
    reader.read_u64().unwrap();
    reader.read_u64().unwrap();
    assert!(!reader.read_bool().unwrap());
    let mut increment = 0;
    for index in 0..REGISTER_COUNT {
        let at = reader.position();
        let _ = read_optional(&mut reader, StateReader::read_u8).unwrap();
        if index == AUTO_INCREMENT_REGISTER {
            increment = at;
        }
    }
    VdpInterrupts::read_core_state(&mut reader, region).unwrap();
    let _ = read_optional(&mut reader, StateReader::read_u16).unwrap();
    let command = reader.position();
    read_command(&mut reader).unwrap();
    VdpWriteFifo::read_state(&mut reader).unwrap();
    assert!(reader.read_bool().unwrap());
    reader.read_u16().unwrap();
    (increment, command, reader.position())
}

pub(crate) fn malformed_active_fetches_still_reject_transactionally() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut vdp = fixture(region, 0, 1);
        command(&mut vdp, 0x8000, VRAM_READ_CODE, region);
        for phase in [false, true] {
            if phase {
                vdp.advance_to(slot(&vdp, region), region).unwrap();
            }
            let bytes = encoded(&vdp, region);
            let (increment, command, ready) = malformed_offsets(&bytes, region);
            let mut invalid = Vec::new();
            let mut unknown = bytes.clone();
            unknown[increment] = 0;
            unknown.remove(increment + 1);
            invalid.push(unknown);
            let mut pending = bytes.clone();
            let publication = vdp.now().get() + H32_INTERNAL_CLOCK_TICKS
                - vdp.now().get() % H32_INTERNAL_CLOCK_TICKS;
            let mut payload = vec![1, AUTO_INCREMENT_REGISTER, 1];
            payload.extend_from_slice(&publication.to_le_bytes());
            pending.splice(16..17, payload);
            invalid.push(pending);
            let mut conflict = bytes.clone();
            conflict[ready] = 1;
            invalid.push(conflict);
            if phase {
                let mut color = bytes.clone();
                color[command + 4] = CRAM_READ_CODE;
                invalid.push(color);
            }
            for data in invalid {
                let mut target = fixture(region, 0, 0xff);
                let before = target.clone();
                assert!(
                    target
                        .read_state(&mut StateReader::new(&data), region)
                        .is_err()
                );
                assert_eq!(target, before);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn even_fetch_odd_increment_phases_and_codec_native() {
    even_fetch_odd_increment_phases_and_codec();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cancellation_keeps_only_completed_progress_native() {
    cancellation_keeps_only_completed_progress();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fifo_words_retire_before_both_read_slots_native() {
    fifo_words_retire_before_both_read_slots();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn malformed_active_fetches_still_reject_transactionally_native() {
    malformed_active_fetches_still_reject_transactionally();
}
