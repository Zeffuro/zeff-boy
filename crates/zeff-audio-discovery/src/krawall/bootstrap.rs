use super::{KrawallSong, RomSpan};

const SPACE: usize = 512;
const ROM_BASE: u32 = 0x0800_0000;

pub(super) fn placement(bytes: &[u8], song: &KrawallSong) -> Option<usize> {
    if bytes.len() < 0xc0 || bytes.len() > super::super::MAX_ROM_BYTES {
        return None;
    }
    let aligned = (bytes.len() + 3) & !3;
    if aligned + SPACE <= super::super::MAX_ROM_BYTES {
        return Some(aligned);
    }
    let end = bytes.len() & !3;
    let lower = end.saturating_sub(65536);
    let mut cursor = end;
    while cursor > lower {
        let value = bytes[cursor - 1];
        if !matches!(value, 0 | 255) {
            cursor -= 1;
            continue;
        }
        let run_end = cursor;
        while cursor > lower && bytes[cursor - 1] == value {
            cursor -= 1;
        }
        if run_end - cursor < SPACE {
            continue;
        }
        let mut offset = (run_end - SPACE) & !3;
        while offset >= cursor {
            let Some(span) = song.mapped_spans.iter().find(|span| overlaps(offset, span)) else {
                return Some(offset);
            };
            let Some(previous) = (span.effective_offset as usize).checked_sub(SPACE) else {
                break;
            };
            offset = previous & !3;
        }
    }
    None
}

fn overlaps(offset: usize, span: &RomSpan) -> bool {
    offset < span.effective_offset as usize + span.byte_len as usize
        && offset + SPACE > span.effective_offset as usize
}

pub(super) fn build(bytes: &[u8], song: &KrawallSong) -> anyhow::Result<Vec<u8>> {
    let offset = placement(bytes, song)
        .ok_or_else(|| anyhow::anyhow!("Krawall bootstrap has no validated ROM space"))?;
    let native = &song.native;
    let mut code = Arm::new(ROM_BASE + offset as u32);
    code.emit(0xe3a0_00d2);
    code.emit(0xe121_f000);
    code.literal(13, 0x0300_7fa0);
    code.emit(0xe3a0_00df);
    code.emit(0xe121_f000);
    code.literal(13, 0x0300_7f00);
    for copy in &native.ram_copies {
        code.literal(0, copy.source.canonical_cpu_address);
        code.literal(1, copy.destination);
        code.literal(2, copy.source.byte_len / 4);
        let start = code.words.len();
        code.emit(0xe490_3004);
        code.emit(0xe481_3004);
        code.emit(0xe252_2001);
        let branch = code.emit(0);
        code.branch(branch, start, 1);
    }
    code.literal(0, 0x0300_7ffc);
    let irq_literal = code.literal(1, 0);
    code.emit(0xe580_1000);
    code.emit(0xe3a0_0001);
    code.call(native.init.cpu_address);
    code.literal(0, 0x0400_0200);
    code.emit(0xe3a0_1010);
    code.emit(0xe1c0_10b0);
    code.emit(0xe1c0_10b2);
    code.emit(0xe3a0_1001);
    code.emit(0xe1c0_10b8);
    code.emit(0xe3a0_001f);
    code.emit(0xe121_f000);
    code.literal(0, song.header.canonical_cpu_address);
    code.emit(0xe3a0_1003);
    code.emit(0xe3a0_2000 | u32::from(song.subsong));
    code.call(native.play.cpu_address);
    code.literal(4, 0x0400_0006);
    let frame = code.words.len();
    code.emit(0xe1d4_00b0);
    code.emit(0xe350_0000);
    let branch = code.emit(0);
    code.branch(branch, frame, 0);
    let zero = code.words.len();
    code.emit(0xe1d4_00b0);
    code.emit(0xe350_0000);
    let branch = code.emit(0);
    code.branch(branch, zero, 1);
    code.call(native.instrument_update.cpu_address);
    code.call(native.mixer.cpu_address);
    let branch = code.emit(0);
    code.branch(branch, frame, 14);

    code.literals[irq_literal].2 = code.base + 4 * code.words.len() as u32;
    code.emit(0xe92d_501f);
    code.literal(2, 0x0400_0200);
    code.emit(0xe592_3000);
    code.emit(0xe003_4823);
    code.emit(0xe314_0010);
    let skip = code.emit(0);
    code.emit(0xe3a0_0010);
    code.emit(0xe1c2_00b2);
    code.call(native.timer1_irq.cpu_address);
    code.branch(skip, code.words.len(), 0);
    code.literal(2, 0x0300_7ff8);
    code.emit(0xe592_3000);
    code.emit(0xe183_3004);
    code.emit(0xe582_3000);
    code.emit(0xe8bd_501f);
    code.emit(0xe12f_ff1e);
    let payload = code.finish()?;
    anyhow::ensure!(
        payload.len() <= SPACE,
        "Krawall bootstrap exceeds its reserved space"
    );
    let mut result = bytes.to_vec();
    result.resize(result.len().max(offset + payload.len()), 0);
    result[offset..offset + payload.len()].copy_from_slice(&payload);
    result[..4].copy_from_slice(&(0xea00_0000 | ((offset as u32 - 8) / 4)).to_le_bytes());
    Ok(result)
}

struct Arm {
    base: u32,
    words: Vec<u32>,
    literals: Vec<(usize, u32, u32)>,
}

impl Arm {
    fn new(base: u32) -> Self {
        Self {
            base,
            words: Vec::new(),
            literals: Vec::new(),
        }
    }

    fn emit(&mut self, word: u32) -> usize {
        let index = self.words.len();
        self.words.push(word);
        index
    }

    fn literal(&mut self, register: u32, value: u32) -> usize {
        let instruction = self.emit(0);
        self.literals.push((instruction, register, value));
        self.literals.len() - 1
    }

    fn call(&mut self, address: u32) {
        self.literal(12, address);
        self.emit(0xe1a0_e00f);
        self.emit(0xe12f_ff1c);
    }

    fn branch(&mut self, instruction: usize, target: usize, condition: u32) {
        let relative = target as i32 - instruction as i32 - 2;
        self.words[instruction] = (condition << 28) | 0x0a00_0000 | (relative as u32 & 0x00ff_ffff);
    }

    fn finish(mut self) -> anyhow::Result<Vec<u8>> {
        for &(instruction, register, value) in &self.literals {
            let relative = (self.words.len() as i32 - instruction as i32 - 2) * 4;
            anyhow::ensure!(
                (0..4096).contains(&relative),
                "Krawall bootstrap literal is out of range"
            );
            self.words[instruction] = 0xe59f_0000 | register << 12 | relative as u32;
            self.words.push(value);
        }
        Ok(self
            .words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect())
    }
}
