use super::*;
use crate::hardware::vdp::{VdpInterruptError, VdpReadError, VdpTimingError, VdpWriteError};

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

fn slot(vdp: &Vdp, region: Region) -> MasterTicks {
    vdp.state
        .interrupts
        .next_write_slot(vdp.now(), region, &vdp.state.registers)
        .unwrap()
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

fn fixture(region: Region, horizontal: u8, display: bool) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    for (index, value) in [
        (AUTO_INCREMENT_REGISTER, 2),
        (DISPLAY_SIZE_REGISTER, horizontal),
        (SPRITE_TABLE_REGISTER, 0),
        (
            MODE_REGISTER,
            MODE5_ENABLE | if display { DISPLAY_ENABLE } else { 0 },
        ),
    ] {
        register(&mut vdp, index, value, region);
    }
    let mut vdp =
        Vdp::from_stable_display_state(*vdp.interrupts(), *vdp.registers(), region).unwrap();
    vdp.advance_to(tick(20 * H32_LINE_TICKS + 101), region)
        .unwrap();
    for (code, data) in [
        (VRAM_WRITE_CODE, 0xa135),
        (CRAM_WRITE_CODE, 0x0ace),
        (VSRAM_WRITE_CODE, 0x0567),
    ] {
        vdp.memory
            .write_word(
                ControlCommand {
                    address: 0x20,
                    code,
                },
                data,
            )
            .unwrap();
    }
    vdp.invalidate_interval();
    vdp
}

pub(crate) fn active_inhibited_fifo_codec_keeps_slot_order_and_memory() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for horizontal in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                let mut vdp = fixture(region, horizontal, display);
                let saved = vdp.memory.clone();
                for (code, data) in [(0x14, 0xbeef), (VRAM_WRITE_CODE, 0xd26b), (0x14, 0x1234)] {
                    command(&mut vdp, 0x20, code, region);
                    vdp.write_data(data, region).unwrap();
                }
                let mut copy = restored(&vdp, region);
                for index in 0..4 {
                    let at = slot(&vdp, region);
                    for before in [tick(at.get() - 1), at] {
                        vdp.advance_to(before, region).unwrap();
                        copy.advance_to(before, region).unwrap();
                        assert_eq!(vdp, copy);
                        copy = restored(&copy, region);
                    }
                    assert_eq!(vdp.fifo().len(), [2, 2, 1, 0][index]);
                    assert_eq!(vdp.state.read_phase, None);
                    assert!(!vdp.read_cache().is_ready());
                    assert_eq!(
                        vdp.command(),
                        ControlCommand {
                            address: 0x22,
                            code: 0x14
                        }
                    );
                    assert_eq!(
                        &vdp.memory.vram()[0x20..0x22],
                        match index {
                            0 => &[0xa1, 0x35],
                            1 => &[0xa1, 0x6b],
                            _ => &[0xd2, 0x6b],
                        }
                    );
                    assert_eq!(vdp.memory.cram(), saved.cram());
                    assert_eq!(vdp.memory.vsram(), saved.vsram());
                }
                let end = tick(vdp.now().get() + H32_LINE_TICKS);
                vdp.advance_to(end, region).unwrap();
                copy.advance_to(end, region).unwrap();
                assert_eq!(vdp, copy);
                restored(&vdp, region);
            }
        }
    }
}

fn fifo_offsets(bytes: &[u8], region: Region) -> (usize, usize) {
    let mut reader = StateReader::new(bytes);
    VdpRegisterPipeline::read_state(&mut reader).unwrap();
    VdpInterrupts::read_core_state(&mut reader, region).unwrap();
    let _ = read_optional(&mut reader, StateReader::read_u16).unwrap();
    read_command(&mut reader).unwrap();
    assert!(reader.read_bool().unwrap());
    let command = reader.position();
    read_command(&mut reader).unwrap();
    reader.read_u16().unwrap();
    for _ in 1..FIFO_CAPACITY {
        let _ = read_optional(&mut reader, |reader| {
            read_command(reader)?;
            reader.read_u16()
        })
        .unwrap();
    }
    reader.read_u8().unwrap();
    reader.read_u8().unwrap();
    (command, reader.position())
}

pub(crate) fn malformed_inhibited_fifo_phases_reject_transactionally() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for horizontal in [0, H40_CLOCK_MASK] {
            let mut vdp = fixture(region, horizontal, true);
            command(&mut vdp, 0x1ffff, 0x14, region);
            vdp.write_data(0xbeef, region).unwrap();
            let bytes = encoded(&vdp, region);
            let (command, phase) = fifo_offsets(&bytes, region);
            for code in [0x14, 0x18, 0x1c, 0x34] {
                let mut malformed = bytes.clone();
                malformed[command + 4] = code;
                if code == 0x14 {
                    malformed[phase] = 1;
                }
                let mut target = fixture(region, horizontal, false);
                let before = target.clone();
                assert!(
                    target
                        .read_state(&mut StateReader::new(&malformed), region)
                        .is_err()
                );
                assert_eq!(target, before);
                assert_eq!(encoded(&target, region), encoded(&before, region));
            }
        }
    }
}

pub(crate) fn inhibited_data_keeps_increment_mode_and_cd5_guards() {
    for region in [Region::NtscUs, Region::Pal] {
        for horizontal in [0, H40_CLOCK_MASK] {
            let seed = fixture(region, horizontal, false);
            let mut cd5 = seed.clone();
            cd5.state.command = ControlCommand {
                address: 0x20,
                code: 0x34,
            };
            let before = cd5.clone();
            assert_eq!(
                cd5.write_data(0xbeef, region),
                Err(VdpPortError::Data(VdpWriteError::UnsupportedCommand(
                    cd5.command()
                )))
            );
            assert_eq!(cd5, before);
            let mut unknown = seed.clone();
            command(&mut unknown, 0x20, 0x14, region);
            let bytes = encoded(&unknown, region);
            let mut reader = StateReader::new(&bytes);
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
            let mut malformed = bytes;
            malformed[increment] = 0;
            malformed.remove(increment + 1);
            unknown
                .read_state(&mut StateReader::new(&malformed), region)
                .unwrap();
            assert_eq!(unknown.registers().auto_increment(), None);
            let before = unknown.clone();
            assert_eq!(
                unknown.write_data(0xbeef, region),
                Err(VdpPortError::Data(VdpWriteError::UnknownIncrement))
            );
            assert_eq!(
                unknown.data_write_wait(region),
                Err(VdpPortError::Data(VdpWriteError::UnknownIncrement))
            );
            assert_eq!(unknown, before);
            let mut pending = seed.clone();
            command(&mut pending, 0x20, 0x14, region);
            pending
                .advance_to(tick(pending.now().get() + 1), region)
                .unwrap();
            pending
                .queue_register(DISPLAY_SIZE_REGISTER, horizontal ^ H40_CLOCK_MASK, region)
                .unwrap();
            let before = pending.clone();
            let error = VdpPortError::Schedule(
                VdpInterruptError::Timing(VdpTimingError::PendingModeChange).into(),
            );
            assert_eq!(pending.write_data(0xbeef, region), Err(error));
            assert_eq!(pending.data_write_wait(region), Err(error));
            assert_eq!(pending, before);
        }
    }
}

pub(crate) fn inhibited_data_cancels_both_unfinished_read_phases() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for horizontal in [0, H40_CLOCK_MASK] {
            for phase in [false, true] {
                let mut vdp = fixture(region, horizontal, true);
                command(&mut vdp, 0x20, VRAM_READ_CODE, region);
                if phase {
                    vdp.advance_to(slot(&vdp, region), region).unwrap();
                }
                assert_eq!(vdp.state.read_phase, Some(phase));
                let memory = vdp.memory.clone();
                command(&mut vdp, 0x20, 0x14, region);
                vdp.write_data(0xbeef, region).unwrap();
                assert_eq!(vdp.state.read_phase, None);
                let mut copy = restored(&vdp, region);
                let at = slot(&vdp, region);
                vdp.advance_to(at, region).unwrap();
                copy.advance_to(at, region).unwrap();
                assert_eq!(vdp, copy);
                assert!(vdp.fifo().is_empty());
                assert_eq!(vdp.memory, memory);
                assert!(!vdp.read_cache().is_ready());
                let before = vdp.clone();
                assert_eq!(
                    vdp.read_data(region),
                    Err(VdpPortError::Read(VdpReadError::Empty))
                );
                assert_eq!(vdp, before);
                restored(&vdp, region);
            }
        }
    }
}

#[test]
fn active_inhibited_fifo_codec_keeps_slot_order_and_memory_native() {
    active_inhibited_fifo_codec_keeps_slot_order_and_memory();
}

#[test]
fn malformed_inhibited_fifo_phases_reject_transactionally_native() {
    malformed_inhibited_fifo_phases_reject_transactionally();
}

#[test]
fn inhibited_data_keeps_increment_mode_and_cd5_guards_native() {
    inhibited_data_keeps_increment_mode_and_cd5_guards();
}

#[test]
fn inhibited_data_cancels_both_unfinished_read_phases_native() {
    inhibited_data_cancels_both_unfinished_read_phases();
}
