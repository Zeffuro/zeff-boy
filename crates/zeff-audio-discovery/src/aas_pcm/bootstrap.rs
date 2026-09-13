use super::{AasPcmSong, MAX_ROM_BYTES, RomSpan};
use crate::gba_bootstrap;

pub struct PreparedAasPcmRom {
    pub bytes: Vec<u8>,
    pub wait_loop: RomSpan,
}

pub(super) fn build(bytes: &[u8], song: &AasPcmSong) -> anyhow::Result<PreparedAasPcmRom> {
    let offset = bytes
        .len()
        .checked_add(3)
        .map(|len| len & !3)
        .filter(|&len| len <= MAX_ROM_BYTES - 512)
        .ok_or_else(|| anyhow::anyhow!("AAS PCM bootstrap has no available ROM space"))?;
    let mut code = Arm::new();
    code.write(0x0400_0208, 0, true);
    code.write(0x0400_0200, 0x11, true);
    code.write(0x0400_0202, 0x3fff, true);
    code.write(0x0400_0004, 8, true);
    for address in [0x0400_00ba, 0x0400_00de] {
        code.write(address, 0, true);
    }
    code.write(
        song.native.vblank_slot_address,
        song.native.update.canonical_cpu_address | 1,
        false,
    );
    code.write(gba_bootstrap::ACK_ADDRESS, 0, false);
    code.write(
        gba_bootstrap::READY_ADDRESS,
        gba_bootstrap::READY_VALUE,
        false,
    );
    code.literal(0, gba_bootstrap::ACK_ADDRESS);
    let wait = code.words.len();
    code.emit(0xe590_1000);
    code.emit(0xe351_0001);
    code.emit(0x1aff_fffc);
    code.emit(0xe24d_d008);
    let start = song.sample_data.canonical_cpu_address - song.native.sample_bank_address;
    code.literal(0, start + song.sample_data.byte_len);
    code.emit(0xe58d_0000);
    code.literal(0, song.loop_start.map_or(0, |offset| start + offset));
    code.emit(0xe58d_0004);
    for (register, value) in [
        u32::from(song.native.playback_channel),
        u32::from(song.native.volume),
        song.sample_rate,
        start,
    ]
    .into_iter()
    .enumerate()
    {
        code.literal(register as u32, value);
    }
    code.call(song.native.play.canonical_cpu_address | 1);
    code.emit(0xe28d_d008);
    code.write(0x0400_0208, 1, true);
    code.emit(0xef02_0000);
    code.emit(0xeaff_fffd);
    let payload = code.finish()?;
    anyhow::ensure!(
        payload.len() <= 512,
        "AAS PCM bootstrap exceeds its reserved space"
    );
    let mut result = bytes.to_vec();
    result.resize(offset + payload.len(), 0);
    result[offset..].copy_from_slice(&payload);
    let hook = song.native.handoff.effective_offset as usize;
    let literal = (hook + 7) & !3;
    anyhow::ensure!(
        literal + 4 <= hook + song.native.handoff.byte_len as usize && literal + 4 <= bytes.len(),
        "AAS PCM startup handoff is too small"
    );
    let op = 0x4b00 | ((literal - ((hook + 4) & !3)) / 4) as u16;
    result[hook..hook + 2].copy_from_slice(&op.to_le_bytes());
    result[hook + 2..hook + 4].copy_from_slice(&0x4718_u16.to_le_bytes());
    result[literal..literal + 4].copy_from_slice(&(0x0800_0000 + offset as u32).to_le_bytes());
    Ok(PreparedAasPcmRom {
        bytes: result,
        wait_loop: RomSpan::new(offset + wait * 4, 12),
    })
}

struct Arm {
    words: Vec<u32>,
    literals: Vec<(usize, u32, u32)>,
}

impl Arm {
    fn new() -> Self {
        Self {
            words: Vec::new(),
            literals: Vec::new(),
        }
    }
    fn emit(&mut self, value: u32) {
        self.words.push(value);
    }
    fn literal(&mut self, register: u32, value: u32) {
        self.literals.push((self.words.len(), register, value));
        self.emit(0);
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
    fn finish(mut self) -> anyhow::Result<Vec<u8>> {
        for &(at, register, value) in &self.literals {
            let relative = (self.words.len() as i32 - at as i32 - 2) * 4;
            anyhow::ensure!(
                (0..4096).contains(&relative),
                "AAS PCM bootstrap literal is out of range"
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
