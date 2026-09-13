use super::{MusyxSong, PreparedMusyxRom, RomSpan};
use crate::gba_bootstrap;

const SPACE: usize = 1024;
const ROM_BASE: u32 = 0x0800_0000;

pub(super) fn placement(bytes: &[u8], song: &MusyxSong) -> Option<usize> {
    let aligned = (bytes.len() + 3) & !3;
    if aligned + SPACE <= super::MAX_ROM_BYTES {
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
            let Some(span) = song.mapped_spans.iter().find(|span| {
                offset < span.effective_offset as usize + span.byte_len as usize
                    && offset + SPACE > span.effective_offset as usize
            }) else {
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

pub(super) fn build(bytes: &[u8], song: &MusyxSong) -> anyhow::Result<PreparedMusyxRom> {
    let offset = placement(bytes, song)
        .ok_or_else(|| anyhow::anyhow!("MusyX bootstrap has no available ROM space"))?;
    let native = &song.native;
    let mut code = Arm::new(ROM_BASE + offset as u32);
    code.emit(0xe8bd_0008);
    code.emit(0xe10f_c000);
    code.emit(0xe38c_c080);
    code.emit(0xe121_f00c);
    code.emit(0xe24d_d010);
    code.emit(0xe890_0070);
    code.emit(0xe88d_0070);
    code.literal(4, song.root.canonical_cpu_address);
    code.emit(0xe58d_4008);
    code.emit(0xe1a0_000d);
    let enter_literal = code.literal(2, 0);
    let leave_literal = code.literal(3, 0);
    let after_literal = code.literal(14, 0);
    // The hook relocates only the verified, position-independent Thumb prologue.
    let prefix = if native.compact_init {
        [
            0xe92d_4070,
            0xe1a0_5000,
            0xe1a0_6001,
            0xe596_0000,
            0xe3a0_1003,
            0xe010_0001,
        ]
    } else {
        [
            0xe92d_40f0,
            0xe1a0_7008,
            0xe92d_0080,
            0xe1a0_5000,
            0xe1a0_6001,
            0xe1a0_8002,
        ]
    };
    for instruction in prefix {
        code.emit(instruction);
    }
    code.literal(12, native.init.canonical_cpu_address + 13);
    code.emit(0xe12f_ff1c);
    code.bind(after_literal);
    code.emit(0xe28d_d010);
    code.stacks();
    code.literal(0, 0x0300_7ffc);
    let irq_literal = code.literal(1, 0);
    code.emit(0xe580_1000);
    for address in [0x0400_00ba, 0x0400_00de] {
        code.literal(0, address);
        code.emit(0xe3a0_1000);
        code.emit(0xe1c0_10b0);
    }
    code.literal(0, gba_bootstrap::READY_ADDRESS);
    code.literal(1, gba_bootstrap::READY_VALUE);
    code.emit(0xe3a0_2000);
    code.emit(0xe580_2004);
    code.emit(0xe580_1000);
    let wait = code.words.len();
    code.emit(0xe590_1004);
    code.emit(0xe351_0000 | gba_bootstrap::ACK_VALUE);
    let branch = code.emit(0);
    code.branch(branch, wait, 1);
    let wait_loop = RomSpan::new(offset + wait * 4, 12);
    code.literal(0, u32::from(song.index));
    code.call(native.select.canonical_cpu_address | 1);
    code.call(native.start.canonical_cpu_address | 1);
    code.literal(0, 0x0400_0200);
    code.emit(0xe3a0_1010);
    code.emit(0xe1c0_10b0);
    code.emit(0xe1c0_10b2);
    code.emit(0xe3a0_1001);
    code.emit(0xe1c0_10b8);
    code.emit(0xe3a0_001f);
    code.emit(0xe121_f000);
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
    code.call(native.update.canonical_cpu_address | 1);
    let branch = code.emit(0);
    code.branch(branch, frame, 14);

    code.bind(irq_literal);
    code.emit(0xe92d_501f);
    code.literal(2, 0x0400_0200);
    code.emit(0xe592_3000);
    code.emit(0xe003_4823);
    code.emit(0xe314_0010);
    let skip = code.emit(0);
    code.emit(0xe3a0_0010);
    code.emit(0xe1c2_00b2);
    code.call(native.timer1_irq.canonical_cpu_address | 1);
    code.branch(skip, code.words.len(), 0);
    code.literal(2, 0x0300_7ff8);
    code.emit(0xe592_3000);
    code.emit(0xe183_3004);
    code.emit(0xe582_3000);
    code.emit(0xe8bd_501f);
    code.emit(0xe12f_ff1e);
    for (label, operation) in [(enter_literal, 0xe3c1_1010), (leave_literal, 0xe381_1010)] {
        code.bind(label);
        code.literal(0, 0x0400_0200);
        code.emit(0xe1d0_10b0);
        code.emit(operation);
        code.emit(0xe1c0_10b0);
        code.emit(0xe12f_ff1e);
    }
    let entry = if let Some(caller) = native.configuration_entry {
        let entry = code.base + code.words.len() as u32 * 4;
        code.stacks();
        code.call(caller.canonical_cpu_address | 1);
        let branch = code.emit(0);
        code.branch(branch, branch, 14);
        entry
    } else {
        native.boot_entry
    };
    let payload = code.finish()?;
    anyhow::ensure!(
        payload.len() <= SPACE,
        "MusyX bootstrap exceeds its reserved space"
    );
    let mut result = bytes.to_vec();
    result.resize(result.len().max(offset + payload.len()), 0);
    result[offset..offset + payload.len()].copy_from_slice(&payload);
    let init = native.init.effective_offset as usize;
    result[init..init + 8].copy_from_slice(&[0x08, 0xb4, 0x01, 0x4b, 0x18, 0x47, 0xc0, 0x46]);
    result[init + 8..init + 12].copy_from_slice(&(ROM_BASE + offset as u32).to_le_bytes());
    let relative = (i64::from(entry) - i64::from(ROM_BASE) - 8) / 4;
    anyhow::ensure!(
        (-0x800000..0x800000).contains(&relative),
        "MusyX boot branch is out of range"
    );
    result[..4].copy_from_slice(&(0xea00_0000 | (relative as u32 & 0x00ff_ffff)).to_le_bytes());
    Ok(PreparedMusyxRom {
        bytes: result,
        wait_loop,
    })
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
    fn bind(&mut self, literal: usize) {
        self.literals[literal].2 = self.base + 4 * self.words.len() as u32;
    }
    fn call(&mut self, address: u32) {
        self.literal(12, address);
        self.emit(0xe1a0_e00f);
        self.emit(0xe12f_ff1c);
    }
    fn stacks(&mut self) {
        self.emit(0xe3a0_00d2);
        self.emit(0xe121_f000);
        self.literal(13, 0x0300_7fa0);
        self.emit(0xe3a0_00df);
        self.emit(0xe121_f000);
        self.literal(13, 0x0300_7f00);
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
                "MusyX bootstrap literal is out of range"
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
