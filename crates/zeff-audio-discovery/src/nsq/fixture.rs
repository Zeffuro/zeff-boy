use super::signatures;

pub(super) const BANK_ENTRY: usize = 0x1000;
pub(super) const TABLE: usize = 0x4000;
pub(super) const FILESYSTEM: usize = 0x4800;
pub(super) const INSTRUMENTS: usize = 0x4900;
pub(super) const SEQUENCE: usize = 0x6200;
pub(super) const SAMPLE: usize = 0x6300;
pub(super) const CALL: usize = 0x304;
pub(super) const PREFIX: usize = 0x4100;
pub(super) const PATH: usize = 0x4140;
pub(super) const RELEASE_PREFIX: usize = 0x41c0;

pub(super) fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn string(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
    bytes[offset + value.len()] = 0;
}

pub(super) fn literal_slot(at: usize, instruction: u16) -> usize {
    ((at + 4) & !3) + usize::from(instruction & 255) * 4
}

pub(super) fn build(layout_index: usize) -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    let layout = &signatures::LAYOUTS[layout_index];
    for (&offset, pattern) in layout.offsets.iter().zip(&layout.patterns) {
        for (index, &instruction) in pattern.iter().enumerate() {
            let at = BANK_ENTRY + offset + index * 2;
            bytes[at..at + 2].copy_from_slice(&instruction.to_le_bytes());
            if instruction & 0xf800 == 0x4800 {
                put(&mut bytes, literal_slot(at, instruction), 0x0300_1000);
            }
        }
    }
    for (index, instruction) in layout.release_copy.iter().enumerate() {
        let at = BANK_ENTRY + layout.release_load + index * 2;
        bytes[at..at + 2].copy_from_slice(&instruction.to_le_bytes());
    }
    put(
        &mut bytes,
        literal_slot(BANK_ENTRY + layout.release_load, layout.release_copy[0]),
        0x0800_0000 + RELEASE_PREFIX as u32,
    );
    string(&mut bytes, RELEASE_PREFIX, b"audio\\patches\\midi");
    bytes[CALL - 4..CALL].copy_from_slice(&[0x3f, 0x48, 0x40, 0x49]);
    let relative = (BANK_ENTRY as i32 - CALL as i32 - 4) as u32;
    let high = 0xf000 | ((relative >> 12) & 2047) as u16;
    let low = 0xf800 | ((relative >> 1) & 2047) as u16;
    bytes[CALL..CALL + 2].copy_from_slice(&high.to_le_bytes());
    bytes[CALL + 2..CALL + 4].copy_from_slice(&low.to_le_bytes());
    put(&mut bytes, 0x400, 0x0800_0000 + PREFIX as u32);
    put(&mut bytes, 0x404, 0x0800_0000 + PATH as u32);
    string(&mut bytes, PREFIX, b"audio\\patches");
    string(&mut bytes, PATH, b"audio\\instruments.npf");
    string(&mut bytes, 0x4180, b"audio\\song.nsq");
    for (index, selector) in [108, 115].into_iter().enumerate() {
        put(&mut bytes, TABLE + index * 8, selector);
        put(&mut bytes, TABLE + index * 8 + 4, 0x0800_4180);
    }
    put(&mut bytes, TABLE + 16, u32::MAX);
    put(&mut bytes, FILESYSTEM, 3);
    put(&mut bytes, FILESYSTEM + 4, 10);
    for (index, (name, offset, length)) in [
        ("audio\\instruments.npf", INSTRUMENTS, 6144),
        ("audio\\song.nsq", SEQUENCE, 56),
        ("audio\\patches\\midi3.raw", SAMPLE, 32),
    ]
    .into_iter()
    .enumerate()
    {
        let hash = name.bytes().fold(0u32, |value, byte| {
            value.wrapping_mul(10).wrapping_add(u32::from(byte))
        });
        let record = FILESYSTEM + 8 + index * 16;
        put(&mut bytes, record, hash);
        put(&mut bytes, record + 8, length);
        put(&mut bytes, record + 12, (offset - FILESYSTEM) as u32);
    }
    for index in 0..128 {
        put(&mut bytes, INSTRUMENTS + index * 48 + 4, u32::MAX);
    }
    put(&mut bytes, INSTRUMENTS, u32::MAX);
    put(&mut bytes, INSTRUMENTS + 4, 3);
    put(&mut bytes, INSTRUMENTS + 8, u32::MAX);
    put(&mut bytes, INSTRUMENTS + 12, 4096);
    bytes[INSTRUMENTS + 26..INSTRUMENTS + 28].copy_from_slice(&128u16.to_le_bytes());
    bytes[SEQUENCE..SEQUENCE + 4].copy_from_slice(b"NSQ\0");
    for (index, (tick, opcode)) in [(0u16, 9u8), (12, 8), (24, 47)].into_iter().enumerate() {
        let event = SEQUENCE + 8 + index * 16;
        bytes[event..event + 2].copy_from_slice(&tick.to_le_bytes());
        bytes[event + 2] = opcode;
        bytes[event + 4] = 60;
        put(&mut bytes, event + 8, 96);
    }
    bytes[SAMPLE..SAMPLE + 32].fill(64);
    bytes
}
