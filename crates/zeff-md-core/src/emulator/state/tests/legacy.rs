use super::*;

const HEADER_BYTES: usize = 52;
const VIDEO_HEADER_BYTES: usize = 15;
const OLD_PIXELS: usize = 307_200;
const VDP_MEMORY_BYTES: usize = 65_536 + 64 * 2 + 40 * 2;

fn bus_prefix(reader: &mut StateReader<'_>) {
    reader.read_u64().unwrap();
    reader.read_u64().unwrap();
    reader.read_u8().unwrap();
    reader.read_u64().unwrap();
    if reader.read_bool().unwrap() {
        reader.read_u8().unwrap();
    }
    reader.set_position(reader.position() + crate::constants::WORK_RAM_SIZE);
}

pub(super) fn section_offsets(current: &[u8]) -> (usize, usize) {
    let mut outer = StateReader::new(current);
    outer.set_position(HEADER_BYTES);
    outer.read_slice(MAX_STATE_BYTES).unwrap();
    let bus_at = outer.position() + 4;
    let bus = outer.read_slice(MAX_STATE_BYTES).unwrap();
    let mut reader = StateReader::new(bus);
    bus_prefix(&mut reader);
    for _ in 0..3 {
        reader.read_slice(MAX_STATE_BYTES).unwrap();
    }
    (bus_at, bus_at + reader.position() + 4)
}

pub(super) fn progressive_state(current: &[u8], version: u32) -> Vec<u8> {
    assert!(
        (MIN_STATE_VERSION..=crate::save_state::STATE_VERSION_WITH_INTERLACE).contains(&version)
    );
    let mut reader = StateReader::new(current);
    reader.set_position(HEADER_BYTES);
    let cpu = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let bus = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let schedule = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let stopped = reader.read_bool().unwrap();
    let save = reader.read_slice(MAX_STATE_BYTES).unwrap();
    assert_eq!(save, &[0]);
    assert!(reader.is_exhausted());
    let mut writer = StateWriter::new();
    writer.write_bytes(&current[..STATE_MAGIC.len()]);
    writer.write_u32(version);
    writer.write_bytes(&current[STATE_MAGIC.len() + 4..HEADER_BYTES]);
    writer.write_vec(cpu);
    if version < crate::save_state::STATE_VERSION_WITH_INTERLACE {
        writer.write_vec(&legacy_bus(bus, version));
    } else {
        writer.write_vec(bus);
    }
    writer.write_vec(schedule);
    writer.write_bool(stopped);
    writer.into_bytes()
}

fn legacy_bus(current: &[u8], version: u32) -> Vec<u8> {
    let mut reader = StateReader::new(current);
    bus_prefix(&mut reader);
    let prefix_end = reader.position();
    let input = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let sound = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let vdp = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let video = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let audio = reader.read_slice(MAX_STATE_BYTES).unwrap();
    assert!(reader.is_exhausted());
    let mut writer = StateWriter::new();
    writer.write_bytes(&current[..prefix_end]);
    let serial_bytes = if version < crate::save_state::STATE_VERSION_WITH_SERIAL_IO {
        crate::constants::IO_PORT_COUNT * 2
    } else {
        0
    };
    writer.write_vec(&input[..input.len() - serial_bytes]);
    writer.write_vec(sound);
    writer.write_vec(&legacy_vdp(vdp));
    writer.write_vec(&legacy_video(video));
    writer.write_vec(&legacy_audio(audio, version));
    writer.into_bytes()
}

fn legacy_vdp(current: &[u8]) -> Vec<u8> {
    let mut reader = StateReader::new(current);
    reader.read_u64().unwrap();
    reader.read_u64().unwrap();
    if reader.read_bool().unwrap() {
        let index = reader.read_u8().unwrap();
        let value = reader.read_u8().unwrap();
        assert!(index != 12 || value & 6 == 0);
        reader.read_u64().unwrap();
    }
    for index in 0..24 {
        if reader.read_bool().unwrap() {
            let value = reader.read_u8().unwrap();
            assert!(index != 12 || value & 6 == 0);
        }
    }
    reader.read_u64().unwrap();
    let calendar_at = reader.position();
    reader.read_u64().unwrap();
    reader.read_u64().unwrap();
    reader.read_u64().unwrap();
    for _ in 0..4 {
        assert!(!reader.read_bool().unwrap());
    }
    assert_eq!(reader.position() - calendar_at, 28);
    // This fixture has never written SAT; old caches must keep their unknown words.
    let memory_at = current.len() - VDP_MEMORY_BYTES;
    assert_eq!(&current[memory_at - 160..memory_at], &[0; 160]);
    let mut old = current.to_vec();
    old.drain(calendar_at..calendar_at + 28);
    old
}

fn legacy_video(current: &[u8]) -> Vec<u8> {
    let mut reader = StateReader::new(current);
    reader.read_u16().unwrap();
    let height = reader.read_u16().unwrap();
    let rows = reader.read_u16().unwrap();
    assert!(height <= 240 && rows <= height);
    reader.set_position(VIDEO_HEADER_BYTES + crate::constants::FRAMEBUFFER_LEN);
    if reader.read_bool().unwrap() {
        reader.read_u64().unwrap();
        assert_eq!(reader.read_u16().unwrap(), height);
        assert!(!reader.read_bool().unwrap());
        assert!(!reader.read_bool().unwrap());
    }
    assert_eq!(reader.read_u8().unwrap(), 0);
    assert!(reader.is_exhausted());
    current[..VIDEO_HEADER_BYTES + OLD_PIXELS].to_vec()
}

fn legacy_audio(current: &[u8], version: u32) -> Vec<u8> {
    let mut reader = StateReader::new(current);
    crate::hardware::audio::Psg::read_state(&mut reader).unwrap();
    let fm_at = reader.position();
    let chip = crate::hardware::audio::Opn2::read_state(&mut reader).unwrap();
    let fm_end = reader.position();
    assert_eq!(current.len() - fm_end, 24);
    let mut writer = StateWriter::new();
    writer.write_bytes(&current[..fm_at]);
    chip.write_state_version(&mut writer, version);
    writer.write_bytes(&current[fm_end..]);
    writer.into_bytes()
}
