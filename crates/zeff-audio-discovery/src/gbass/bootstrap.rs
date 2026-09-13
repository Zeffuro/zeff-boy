use super::{GbassSchedule, GbassSong, MAX_ROM_BYTES, RomSpan};
use crate::gba_bootstrap;

pub struct PreparedGbassRom {
    pub bytes: Vec<u8>,
    pub wait_loop: RomSpan,
}

pub(super) fn build(bytes: &[u8], song: &GbassSong) -> anyhow::Result<PreparedGbassRom> {
    let offset = bytes
        .len()
        .checked_add(3)
        .map(|len| len & !3)
        .filter(|&len| len <= MAX_ROM_BYTES - 512)
        .ok_or_else(|| anyhow::anyhow!("GBASS bootstrap has no available ROM space"))?;
    let mut code = Arm::new(0x0800_0000 + offset as u32);
    if let Some(module) = song.native.module {
        code.literal(6, module.source.byte_len);
        code.literal(7, module.source.canonical_cpu_address);
        code.call(module.loader.canonical_cpu_address | 1);
    }
    let entry = 0x0800_0000 + offset as u32 + code.words.len() as u32 * 4;
    let runtime = |routine: RomSpan| {
        song.native
            .module
            .map_or(Ok(routine.canonical_cpu_address), |module| {
                module
                    .runtime_address(routine)
                    .ok_or_else(|| anyhow::anyhow!("GBASS routine is outside its loaded module"))
            })
    };
    code.write(0x0400_0208, 0, true);
    if song.native.schedule == GbassSchedule::VblankIrq || song.native.module.is_some() {
        code.write(0x0400_0004, 8, true);
    }
    code.write(0x0400_0200, 1, true);
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
    code.emit(0xe351_0001);
    let branch = code.emit(0);
    code.branch(branch, wait, 1);
    if !song.native.hardware_started_before_handoff {
        code.call(runtime(song.native.hardware_start)? | 1);
    }
    code.literal(0, u32::from(song.index));
    if let Some(bank) = song.native.bank {
        code.literal(1, u32::from(bank.index));
    }
    code.call(runtime(song.native.play)? | 1);
    code.write(0x0400_0208, 1, true);
    let update = code.words.len();
    code.call(runtime(song.native.vblank_wait)? | 1);
    if song.native.schedule == GbassSchedule::VblankThenMain {
        code.call(runtime(song.native.update_wrapper)? | 1);
    }
    let branch = code.emit(0);
    code.branch(branch, update, 14);
    let payload = code.finish()?;
    anyhow::ensure!(
        payload.len() <= 512,
        "GBASS bootstrap exceeds its reserved space"
    );
    let mut result = bytes.to_vec();
    result.resize(offset + payload.len(), 0);
    result[offset..].copy_from_slice(&payload);
    patch(&mut result, song.native.handoff, entry, bytes.len())?;
    if let Some(module) = song.native.module {
        patch(
            &mut result,
            module.loader_handoff,
            0x0800_0000 + offset as u32,
            bytes.len(),
        )?;
    }
    Ok(PreparedGbassRom {
        bytes: result,
        wait_loop: RomSpan::new(offset + wait * 4, 12),
    })
}

fn patch(
    result: &mut [u8],
    handoff: RomSpan,
    entry: u32,
    original_len: usize,
) -> anyhow::Result<()> {
    let hook = handoff.effective_offset as usize;
    let literal = (hook + 7) & !3;
    anyhow::ensure!(
        literal + 4 <= hook + handoff.byte_len as usize && literal + 4 <= original_len,
        "GBASS startup handoff is too small"
    );
    let op = 0x4b00 | ((literal - ((hook + 4) & !3)) / 4) as u16;
    result[hook..hook + 2].copy_from_slice(&op.to_le_bytes());
    result[hook + 2..hook + 4].copy_from_slice(&0x4718_u16.to_le_bytes());
    result[literal..literal + 4].copy_from_slice(&entry.to_le_bytes());
    Ok(())
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
    fn literal(&mut self, register: u32, value: u32) {
        let at = self.emit(0);
        self.literals.push((at, register, value));
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
                "GBASS bootstrap literal is out of range"
            );
            self.words[at] = 0xe59f_0000 | register << 12 | relative as u32;
            self.words.push(value);
        }
        anyhow::ensure!(
            self.base >= 0x0800_0000,
            "GBASS bootstrap address is invalid"
        );
        Ok(self
            .words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect())
    }
}
