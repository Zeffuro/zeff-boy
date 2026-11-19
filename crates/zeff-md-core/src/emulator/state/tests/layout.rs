use super::*;

const HEADER_BYTES: usize = 52;

pub(in crate::emulator) fn capabilities_offset(current: &[u8]) -> usize {
    cartridge_section_offset(current, 3)
}

pub(in crate::emulator) fn sf001_offset(current: &[u8]) -> usize {
    cartridge_section_offset(current, 2)
}

pub(in crate::emulator) fn download_offset(current: &[u8]) -> usize {
    cartridge_section_offset(current, 8)
}

fn cartridge_section_offset(current: &[u8], preceding: usize) -> usize {
    assert_eq!(
        &current[8..12],
        &crate::save_state::STATE_VERSION.to_le_bytes()
    );
    let mut reader = StateReader::new(current);
    reader.set_position(HEADER_BYTES);
    for _ in 0..3 {
        reader.read_slice(MAX_STATE_BYTES).unwrap();
    }
    reader.read_bool().unwrap();
    for _ in 0..preceding {
        reader.read_slice(MAX_STATE_BYTES).unwrap();
    }
    let at = reader.position() + 4;
    assert!(!reader.read_slice(MAX_STATE_BYTES).unwrap().is_empty());
    at
}

pub(super) fn bus_prefix(reader: &mut StateReader<'_>) {
    bus_header(reader);
    read_prefetch(reader);
    reader.set_position(reader.position() + crate::constants::WORK_RAM_SIZE);
}

fn bus_header(reader: &mut StateReader<'_>) {
    reader.read_u64().unwrap();
    reader.read_u64().unwrap();
    reader.read_u8().unwrap();
    reader.read_u64().unwrap();
}

pub(in crate::emulator) fn prefetch_offset(current: &[u8]) -> usize {
    let mut outer = StateReader::new(current);
    outer.set_position(HEADER_BYTES);
    outer.read_slice(MAX_STATE_BYTES).unwrap();
    let bus_at = outer.position() + 4;
    let bus = outer.read_slice(MAX_STATE_BYTES).unwrap();
    let mut reader = StateReader::new(bus);
    bus_header(&mut reader);
    bus_at + reader.position()
}

fn read_prefetch(reader: &mut StateReader<'_>) -> crate::hardware::bus::Prefetch {
    use crate::hardware::bus::Prefetch;
    match reader.read_u8().unwrap() {
        0 => Prefetch::Empty,
        1 => Prefetch::UpperOnly(reader.read_u8().unwrap()),
        2 => Prefetch::Full(reader.read_u16().unwrap()),
        tag => panic!("invalid current prefetch tag {tag}"),
    }
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

pub(in crate::emulator) fn uart_history_offset(input: &[u8]) -> usize {
    after_serial(input).position() + 4
}

fn after_serial(input: &[u8]) -> StateReader<'_> {
    let mut field = StateReader::new(input);
    for _ in 0..crate::constants::CONTROLLER_PORT_COUNT {
        for _ in 0..3 {
            field.read_u8().unwrap();
        }
        field.read_bool().unwrap();
        field.read_u8().unwrap();
        if field.read_bool().unwrap() {
            field.read_u64().unwrap();
        }
    }
    for _ in 0..crate::constants::IO_PORT_COUNT * 4 {
        field.read_u8().unwrap();
    }
    field.read_u64().unwrap();
    field.read_u32().unwrap();
    for _ in 0..3 {
        field.read_u8().unwrap();
    }
    if field.read_bool().unwrap() {
        field.read_u8().unwrap();
    }
    field
}
