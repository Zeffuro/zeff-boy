use super::{
    MAX_ROM_BYTES, PreparedRadriverRom, RadriverLayout, RadriverSong, RadriverSongKind, RomSpan,
    word,
};
use crate::gba_bootstrap;

const SPACE: usize = 1024;

pub(super) fn placement(bytes: &[u8]) -> Option<usize> {
    let aligned = bytes.len().checked_add(3)? & !3;
    (aligned.checked_add(SPACE)? <= MAX_ROM_BYTES).then_some(aligned)
}

pub(super) fn build(bytes: &[u8], song: &RadriverSong) -> anyhow::Result<PreparedRadriverRom> {
    let offset = placement(bytes)
        .ok_or_else(|| anyhow::anyhow!("RADriver bootstrap has no available ROM space"))?;
    let native = &song.native;
    let mut code = Arm::new(0x0800_0000 + offset as u32);
    code.write(0x0400_0208, 0, true);
    code.emit(0xe3a0_c0d2);
    code.emit(0xe121_f00c);
    code.literal(13, 0x0300_7fa0);
    code.emit(0xe3a0_c0df);
    code.emit(0xe121_f00c);
    code.literal(13, 0x0300_7e00);
    code.literal(0, 0x0300_7ffc);
    let handler = code.literal(1, 0);
    code.emit(0xe580_1000);
    for address in [
        0x0400_00ba,
        0x0400_00d2,
        0x0400_00de,
        0x0400_010a,
        0x0400_010e,
        0x0400_0004,
    ] {
        code.write(address, 0, true);
    }
    code.literal(0, u32::from(song.index));
    code.literal(
        1,
        if native.layout == RadriverLayout::GlobalState {
            60
        } else {
            960
        },
    );
    let play = match song.kind {
        RadriverSongKind::Effect => native.play_effect,
        RadriverSongKind::CompressedMusic => native.play_music.unwrap(),
    };
    code.call(play.canonical_cpu_address | 1);
    code.emit(0xe350_0000);
    let mut failures = vec![code.emit(0)];
    let channel_pointer = if native.layout == RadriverLayout::GlobalState {
        native.state + 0x10
    } else {
        native.state
    };
    code.literal(0, channel_pointer);
    code.emit(0xe590_0000);
    if native.layout == RadriverLayout::ContextState {
        code.emit(0xe590_0024);
    }
    let expected = if let Some(order) = song.order {
        let first = super::half(bytes, order.effective_offset as usize).unwrap();
        word(
            bytes,
            song.blocks.unwrap().effective_offset as usize + usize::from(first) * 4,
        )
        .unwrap()
    } else {
        song.samples[0].header.canonical_cpu_address
    };
    code.emit(0xe590_1000);
    code.literal(2, expected);
    code.emit(0xe151_0002);
    failures.push(code.emit(0));
    code.emit(0xe5d0_1016);
    code.emit(0xe351_0001);
    failures.push(code.emit(0));
    if song.kind == RadriverSongKind::CompressedMusic {
        for (field, expected) in [(0x1c, song.blocks.unwrap()), (0x20, song.order.unwrap())] {
            code.emit(0xe590_1000 | field);
            code.literal(2, expected.canonical_cpu_address);
            code.emit(0xe151_0002);
            failures.push(code.emit(0));
        }
    }
    code.write(0x0400_0004, 8, true);
    code.write(0x0400_0200, 0x11, true);
    code.write(0x0400_0202, 0x3fff, true);
    code.write(gba_bootstrap::ACK_ADDRESS, 0, false);
    code.write(
        gba_bootstrap::READY_ADDRESS,
        gba_bootstrap::READY_VALUE,
        false,
    );
    code.literal(0, gba_bootstrap::ACK_ADDRESS);
    let wait = code.words.len();
    code.emit(0xe590_1000);
    code.emit(0xe351_0000 | gba_bootstrap::ACK_VALUE);
    let branch = code.emit(0);
    code.branch(branch, wait, 1);
    let wait_loop = RomSpan::new(offset + wait * 4, 12);
    code.write(0x0400_0208, 1, true);
    code.emit(0xe3a0_c01f);
    code.emit(0xe121_f00c);
    let update_loop = code.words.len();
    code.write(0x0300_7fe8, 0, false);
    let halt = code.words.len();
    code.emit(0xef02_0000);
    code.literal(0, 0x0300_7fe8);
    code.emit(0xe590_1000);
    code.emit(0xe351_0000);
    let branch = code.emit(0);
    code.branch(branch, halt, 0);
    // Equal rounded timer counts mean a full wrap to this driver.
    code.call(native.update.canonical_cpu_address | 1);
    let branch = code.emit(0);
    code.branch(branch, update_loop, 14);
    let failed = code.words.len();
    let branch = code.emit(0);
    code.branch(branch, failed, 14);
    for branch in failures {
        code.branch(branch, failed, 1);
    }
    code.bind(handler);
    code.emit(0xe92d_4010);
    code.literal(3, 0x0400_0200);
    code.emit(0xe593_2000);
    code.emit(0xe002_4822);
    code.emit(0xe1c3_40b2);
    code.emit(0xe314_0010);
    let skip = code.emit(0);
    code.call(native.timer_irq.canonical_cpu_address | 1);
    code.branch(skip, code.words.len(), 0);
    code.emit(0xe314_0001);
    code.literal(0, 0x0300_7fe8);
    code.emit(0x13a0_1001);
    code.emit(0x1580_1000);
    code.emit(0xe8bd_4010);
    code.emit(0xe12f_ff1e);
    let payload = code.finish()?;
    anyhow::ensure!(
        payload.len() <= SPACE,
        "RADriver bootstrap exceeds its reserved space"
    );
    let mut result = bytes.to_vec();
    result.resize(offset + payload.len(), 0);
    result[offset..].copy_from_slice(&payload);
    let hook = native.handoff.effective_offset as usize;
    let literal = (hook + 7) & !3;
    let op = 0x4b00 | ((literal - ((hook + 4) & !3)) / 4) as u16;
    result[hook..hook + 2].copy_from_slice(&op.to_le_bytes());
    result[hook + 2..hook + 4].copy_from_slice(&0x4718_u16.to_le_bytes());
    result[literal..literal + 4].copy_from_slice(&(0x0800_0000 + offset as u32).to_le_bytes());
    Ok(PreparedRadriverRom {
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
    fn write(&mut self, address: u32, value: u32, half: bool) {
        self.literal(0, address);
        self.literal(1, value);
        self.emit(if half { 0xe1c0_10b0 } else { 0xe580_1000 });
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
                "RADriver bootstrap literal is out of range"
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
