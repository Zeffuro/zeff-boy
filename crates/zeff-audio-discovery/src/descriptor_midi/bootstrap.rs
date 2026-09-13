use super::{DescriptorMidiSong, MAX_ROM_BYTES, PreparedDescriptorMidiRom, RomSpan, half, word};
use crate::gba_bootstrap;

const SPACE: usize = 1024;
const ROM_BASE: u32 = 0x0800_0000;
const IRQ_STACK: u32 = 0x0300_7fa0;
const SYSTEM_STACK: u32 = 0x0300_7e00;
const IRQ_VECTOR: u32 = 0x0300_7ffc;
const VBLANK_FLAG: u32 = 0x0300_7fe8;
const INTERRUPT_ENABLE: u32 = 0x0400_0200;
const INTERRUPT_MASTER_ENABLE: u32 = 0x0400_0208;
const AUDIO_INTERRUPTS: u32 = 0x0401;

pub(super) fn placement(bytes: &[u8]) -> Option<usize> {
    let aligned = bytes.len().checked_add(3)? & !3;
    (aligned.checked_add(SPACE)? <= MAX_ROM_BYTES).then_some(aligned)
}

pub(super) fn build(
    bytes: &[u8],
    song: &DescriptorMidiSong,
) -> anyhow::Result<PreparedDescriptorMidiRom> {
    let offset = placement(bytes)
        .ok_or_else(|| anyhow::anyhow!("Descriptor MIDI bootstrap has no available ROM space"))?;
    let native = &song.native;
    let slot = native.song_table.effective_offset as usize + usize::from(song.index) * 8;
    let player = &native.players[half(bytes, slot + 4).unwrap() as usize];
    let flags = word(bytes, song.header.effective_offset as usize + 4).unwrap();
    let bank = word(
        bytes,
        native.bank_table.effective_offset as usize + ((flags as usize & 0x7fff) >> 5) * 4,
    )
    .unwrap();
    let mut code = Arm::new(ROM_BASE + offset as u32);
    code.literal(0, INTERRUPT_MASTER_ENABLE);
    code.emit(0xe3a0_1000);
    code.emit(0xe1c0_10b0);
    code.emit(0xe3a0_c0d2);
    code.emit(0xe121_f00c);
    code.literal(13, IRQ_STACK);
    code.emit(0xe3a0_c0df);
    code.emit(0xe121_f00c);
    code.literal(13, SYSTEM_STACK);
    code.literal(4, IRQ_VECTOR);
    let handler_literal = code.literal(5, 0);
    code.emit(0xe584_5000);
    code.literal(0, u32::from(song.index));
    code.call(native.play.canonical_cpu_address | 1);
    code.literal(0, player.state);
    code.emit(0xe590_100c);
    code.literal(2, song.header.canonical_cpu_address);
    code.emit(0xe151_0002);
    let failed_selection = code.emit(0);
    code.literal(0, player.voice_state);
    code.emit(0xe590_1010);
    code.literal(2, bank);
    code.emit(0xe151_0002);
    let failed_bank = code.emit(0);
    for address in [0x0400_00ba, 0x0400_00de] {
        code.literal(0, address);
        code.emit(0xe3a0_1000);
        code.emit(0xe1c0_10b0);
    }
    code.literal(0, 0x0400_0004);
    code.emit(0xe3a0_1008);
    code.emit(0xe1c0_10b0);
    code.literal(0, INTERRUPT_ENABLE);
    code.literal(1, AUDIO_INTERRUPTS);
    code.emit(0xe1c0_10b0);
    code.emit(0xe1c0_10b2);
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
    let update_loop = code.words.len();
    code.literal(0, VBLANK_FLAG);
    code.emit(0xe3a0_1000);
    code.emit(0xe580_1000);
    let halt = code.words.len();
    code.emit(0xef02_0000);
    code.literal(0, VBLANK_FLAG);
    code.emit(0xe590_1000);
    code.emit(0xe351_0000);
    let branch = code.emit(0);
    code.branch(branch, halt, 0);
    code.call(native.update.canonical_cpu_address | 1);
    let branch = code.emit(0);
    code.branch(branch, update_loop, 14);
    let failed = code.words.len();
    let branch = code.emit(0);
    code.branch(branch, failed, 14);
    code.branch(failed_selection, failed, 1);
    code.branch(failed_bank, failed, 1);

    // The BIOS saves r0-r3/r12; the native DMA callback preserves r4-r11.
    code.bind(handler_literal);
    code.emit(0xe92d_4010);
    code.literal(3, INTERRUPT_ENABLE);
    code.emit(0xe593_2000);
    code.emit(0xe002_4822);
    code.emit(0xe1c3_40b2);
    code.emit(0xe314_0b01);
    let skip_dma = code.emit(0);
    code.call(native.dma_irq.canonical_cpu_address | 1);
    let after_dma = code.words.len();
    code.branch(skip_dma, after_dma, 0);
    code.emit(0xe314_0001);
    code.literal(0, VBLANK_FLAG);
    code.emit(0x13a0_1001);
    code.emit(0x1580_1000);
    code.emit(0xe8bd_4010);
    code.emit(0xe12f_ff1e);
    let payload = code.finish()?;
    anyhow::ensure!(
        payload.len() <= SPACE,
        "Descriptor MIDI bootstrap exceeds its reserved space"
    );
    let mut result = bytes.to_vec();
    result.resize(offset + payload.len(), 0);
    result[offset..].copy_from_slice(&payload);
    let hook = native.handoff.effective_offset as usize;
    result[hook..hook + 8].copy_from_slice(&[0x01, 0x4b, 0x18, 0x47, 0xc0, 0x46, 0xc0, 0x46]);
    result[hook + 8..hook + 12].copy_from_slice(&(ROM_BASE + offset as u32).to_le_bytes());
    Ok(PreparedDescriptorMidiRom {
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
                "Descriptor MIDI bootstrap literal is out of range"
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
