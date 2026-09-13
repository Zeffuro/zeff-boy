use super::{MAX_ROM_BYTES, NsqSong, PreparedNsqRom, RomSpan};
use crate::gba_bootstrap;

const SPACE: usize = 2048;
const ROM_BASE: u32 = 0x0800_0000;
const IRQ_STACK: u32 = 0x0300_7fa0;
const SYSTEM_STACK: u32 = 0x0300_7f00;
const IRQ_VECTOR: u32 = 0x0300_7ffc;
const DMA0_CONTROL: u32 = 0x0400_00ba;
const DMA3_CONTROL: u32 = 0x0400_00de;
const INTERRUPT_ENABLE: u32 = 0x0400_0200;
const INTERRUPT_MASTER_ENABLE: u32 = 0x0400_0208;

pub(super) fn placement(bytes: &[u8]) -> Option<usize> {
    let aligned = bytes.len().checked_add(3)? & !3;
    (aligned.checked_add(SPACE)? <= MAX_ROM_BYTES).then_some(aligned)
}

pub(super) fn build(bytes: &[u8], song: &NsqSong) -> anyhow::Result<PreparedNsqRom> {
    let offset = placement(bytes)
        .ok_or_else(|| anyhow::anyhow!("NSQ bootstrap has no available ROM space"))?;
    let native = &song.native;
    let mut code = Arm::new(ROM_BASE + offset as u32);
    code.emit(0xe8bd_0008);
    let mut accepted = Vec::new();
    for &[directory, path] in &native.bank_arguments {
        code.literal(12, directory);
        code.emit(0xe150_000c);
        let next = code.emit(0);
        code.literal(12, path);
        code.emit(0xe151_000c);
        accepted.push(code.emit(0));
        code.branch(next, code.words.len(), 1);
    }
    let rejected_arguments = code.emit(0);
    for branch in accepted {
        code.branch(branch, code.words.len(), 0);
    }
    // Original startup initializes the heap and file system before loading this bank.
    code.emit(0xe3a0_c0d2);
    code.emit(0xe121_f00c);
    code.literal(13, IRQ_STACK);
    code.emit(0xe3a0_c0df);
    code.emit(0xe121_f00c);
    code.literal(13, SYSTEM_STACK);
    code.literal(4, INTERRUPT_MASTER_ENABLE);
    code.emit(0xe3a0_5000);
    code.emit(0xe1c4_50b0);
    code.literal(4, IRQ_VECTOR);
    let handler_literal = code.literal(5, 0);
    code.emit(0xe584_5000);
    let config_literal = code.literal(12, 0);
    code.emit(0xe1a0_e00f);
    code.emit(0xe12f_ff1c);
    code.literal(0, 0);
    code.literal(1, native.song_table.canonical_cpu_address);
    code.call(native.load_songs.canonical_cpu_address | 1);
    code.literal(0, u32::from(song.index));
    code.call(native.play.canonical_cpu_address | 1);
    // DMA1/2 belong to audio; disable unrelated graphics transfers after startup.
    for address in [DMA0_CONTROL, DMA3_CONTROL] {
        code.literal(0, address);
        code.emit(0xe3a0_1000);
        code.emit(0xe1c0_10b0);
    }
    code.literal(0, INTERRUPT_ENABLE);
    code.emit(0xe3a0_1001);
    code.emit(0xe1c0_10b0);
    code.emit(0xe1c0_10b2);
    code.literal(0, 0x0400_0004);
    code.emit(0xe3a0_1008);
    code.emit(0xe1c0_10b0);
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
    code.literal(0, INTERRUPT_MASTER_ENABLE);
    code.emit(0xe3a0_1001);
    code.emit(0xe1c0_10b0);
    code.emit(0xe3a0_c01f);
    code.emit(0xe121_f00c);
    let halt = code.words.len();
    code.emit(0xef02_0000);
    let branch = code.emit(0);
    code.branch(branch, halt, 14);
    let failed = code.words.len();
    let branch = code.emit(0);
    code.branch(branch, failed, 14);
    code.branch(rejected_arguments, failed, 14);

    code.bind(handler_literal);
    code.literal(3, INTERRUPT_ENABLE);
    code.emit(0xe593_2000);
    code.emit(0xe1d3_10b8);
    code.emit(0xe14f_0000);
    code.emit(0xe92d_400f);
    code.emit(0xe3a0_0000);
    code.emit(0xe1c3_00b8);
    code.emit(0xe002_1822);
    code.emit(0xe1c3_10b2);
    code.emit(0xe10f_3000);
    code.emit(0xe3c3_30df);
    code.emit(0xe383_301f);
    code.emit(0xe121_f003);
    code.emit(0xe92d_4000);
    code.call(native.vblank.canonical_cpu_address | 1);
    code.call(native.mix.canonical_cpu_address | 1);
    code.emit(0xe8bd_4000);
    code.emit(0xe10f_3000);
    code.emit(0xe3c3_30df);
    code.emit(0xe383_3092);
    code.emit(0xe121_f003);
    code.emit(0xe8bd_400f);
    code.emit(0xe1c3_20b0);
    code.emit(0xe1c3_10b8);
    code.emit(0xe169_f000);
    // The BIOS callback epilogue performs the exception return after this BX LR.
    code.emit(0xe12f_ff1e);
    let trampoline = code.base + (code.words.len() + code.literals.len()) as u32 * 4;
    code.literals[config_literal].2 = trampoline | 1;
    let mut payload = code.finish()?;
    let init = native.load_bank.effective_offset as usize;
    payload.extend_from_slice(&bytes[init..init + 12]);
    for half in [0xb408u16, 0x4b02, 0x469c, 0xbc08, 0x4760, 0x46c0] {
        payload.extend_from_slice(&half.to_le_bytes());
    }
    payload.extend_from_slice(&(native.load_bank.canonical_cpu_address + 13).to_le_bytes());
    anyhow::ensure!(
        payload.len() <= SPACE,
        "NSQ bootstrap exceeds its reserved space"
    );
    let mut result = bytes.to_vec();
    result.resize(offset + payload.len(), 0);
    result[offset..].copy_from_slice(&payload);
    result[init..init + 8].copy_from_slice(&[0x08, 0xb4, 0x01, 0x4b, 0x18, 0x47, 0xc0, 0x46]);
    result[init + 8..init + 12].copy_from_slice(&(ROM_BASE + offset as u32).to_le_bytes());
    Ok(PreparedNsqRom {
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
    fn emit(&mut self, value: u32) -> usize {
        let at = self.words.len();
        self.words.push(value);
        at
    }
    fn literal(&mut self, register: u32, value: u32) -> usize {
        let at = self.emit(0);
        self.literals.push((at, register, value));
        self.literals.len() - 1
    }
    fn bind(&mut self, literal: usize) {
        self.literals[literal].2 = self.base + self.words.len() as u32 * 4;
    }
    fn call(&mut self, address: u32) {
        self.literal(12, address);
        self.emit(0xe1a0_e00f);
        self.emit(0xe12f_ff1c);
    }
    fn branch(&mut self, at: usize, target: usize, condition: u32) {
        self.words[at] =
            condition << 28 | 0x0a00_0000 | ((target as i32 - at as i32 - 2) as u32 & 0x00ff_ffff);
    }
    fn finish(mut self) -> anyhow::Result<Vec<u8>> {
        for &(at, register, value) in &self.literals {
            let relative = (self.words.len() as i32 - at as i32 - 2) * 4;
            anyhow::ensure!(
                (0..4096).contains(&relative),
                "NSQ bootstrap literal is out of range"
            );
            self.words[at] = 0xe59f_0000 | register << 12 | relative as u32;
            self.words.push(value);
        }
        Ok(self
            .words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect())
    }
}
