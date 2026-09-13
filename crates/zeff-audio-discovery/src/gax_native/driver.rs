use anyhow::{Context, ensure};

use super::{GaxNativeLayout, GaxNativeProfile, GaxNativeSong, placement, v1};

#[path = "secondary.rs"]
mod secondary;

const ROM_BASE: u32 = 0x0800_0000;
const RESERVED_IRQ_BYTES: u32 = 0x9c;

pub(super) fn build(bytes: &[u8], song: &GaxNativeSong) -> anyhow::Result<Vec<u8>> {
    let mut code = Program::new();
    let start = code.emit(0);

    code.emit(0xe3a0_00ff);
    code.emit(0xef01_0000);
    code.emit(0xe3a0_0012);
    code.emit(0xe121_f000);
    code.literal(13, 0x0300_7fa0);
    code.emit(0xe3a0_001f);
    code.emit(0xe121_f000);
    code.literal(13, 0x0300_7f00);

    for copy in &song.native.ram_copies {
        code.literal(0, copy.source.canonical_cpu_address);
        code.literal(1, copy.destination);
        code.literal(2, copy.destination + copy.source.byte_len);
        let copy_word = code.words.len();
        code.emit(0xe490_3004);
        code.emit(0xe481_3004);
        code.emit(0xe151_0002);
        let more = code.emit(0);
        code.branch(more, copy_word, 3);
    }

    // Match the GBA wait-state setup required before the original ROM routines run.
    code.literal(0, 0x0400_0204);
    code.literal(1, 0x0000_4014);
    code.emit(0xe1c0_10b0);

    match song.native.layout {
        GaxNativeLayout::V1_99 => v1::initialize(&mut code, song)?,
        GaxNativeLayout::V2Current | GaxNativeLayout::V2_01 => {
            initialize_v2(&mut code, bytes, song)?
        }
        GaxNativeLayout::V3Legacy | GaxNativeLayout::V3Modern => {
            anyhow::bail!("GAX bootstrap layout is not supported by this driver");
        }
    }

    code.literal(0, 0x0300_7ffc);
    let handler_pointer = code.literal(1, 0);
    code.emit(0xe580_1000);
    code.literal(3, 0x0400_0200);
    code.emit(0xe3a0_1001);
    code.emit(0xe1c3_10b8);
    code.emit(0xe1c3_10b0);
    code.literal(0, 0x0400_0004);
    code.emit(0xe3a0_1008);
    code.emit(0xe1c0_10b0);

    let wait = code.words.len();
    code.emit(0xef02_0000);
    let wait_again = code.emit(0);
    code.branch(wait_again, wait, 14);

    let handler = code.words.len();
    code.literal(3, 0x0400_0200);
    code.emit(0xe593_2000);
    code.emit(0xe1d3_10b8);
    code.emit(0xe14f_0000);
    code.emit(0xe92d_400f);
    code.emit(0xe3a0_0000);
    code.emit(0xe1c3_00b8);
    code.emit(0xe002_1822);
    code.emit(0xe311_0001);
    code.emit(0xe1c3_10b2);
    code.emit(0xe10f_3000);
    code.emit(0xe3c3_30df);
    code.emit(0xe383_301f);
    code.emit(0xe121_f003);
    code.emit(0xe92d_4000);
    code.call(song.native.mix.cpu_address);
    code.call(song.native.play.cpu_address);
    code.emit(0xe8bd_4000);
    code.emit(0xe10f_3000);
    code.emit(0xe3c3_30df);
    code.emit(0xe383_3092);
    code.emit(0xe121_f003);
    code.emit(0xe8bd_400f);
    code.emit(0xe1c3_20b0);
    code.emit(0xe1c3_10b8);
    code.emit(0xe169_f000);
    code.emit(0xe12f_ff1e);
    let length = code.byte_len().context("GAX bootstrap length overflow")?;
    let offset = placement::choose(bytes, song, length)
        .context("GAX bootstrap has no validated ROM space")?;
    let base = ROM_BASE + offset as u32;
    code.replace_literal(handler_pointer, base + handler as u32 * 4)?;

    code.branch(start, 1, 14);
    let payload = code.finish()?;
    placement::install(bytes, offset, &payload)
}

fn initialize_v2(code: &mut Program, bytes: &[u8], song: &GaxNativeSong) -> anyhow::Result<()> {
    let new = song
        .native
        .new
        .context("GAX bootstrap layout requires an original constructor")?;
    code.emit(0xe1a0_400d);
    code.emit(0xe3a0_1010);
    code.emit(0xe3a0_0000);
    let clear = code.words.len();
    code.emit(0xe484_0004);
    code.emit(0xe251_1001);
    let clear_more = code.emit(0);
    code.branch(clear_more, clear, 1);
    code.emit(0xe1a0_400d);
    code.emit(0xe1a0_0004);
    code.call(new.cpu_address);

    let (work_ram, work_ram_bytes) = v2_workspace(bytes, &song.native)
        .context("GAX workspace overlaps startup RAM, driver state or reserved stacks")?;
    code.literal(0, work_ram);
    code.emit(0xe584_0000);
    code.literal(0, work_ram_bytes);
    code.emit(0xe584_0004);
    code.literal(0, u32::from(song.native.sample_rate));
    code.emit(0xe1c4_00b8);
    code.emit(0xe3a0_0000);
    code.emit(0xe1c4_00be);
    match song.native.layout {
        GaxNativeLayout::V2Current => {
            code.emit(0xe3a0_0004);
            code.emit(0xe1c4_01b0);
            code.literal(0, u32::from(song.native.sample_rate));
            code.emit(0xe1c4_01b2);
        }
        GaxNativeLayout::V2_01 => {
            code.literal(0, u32::from(song.native.sample_rate));
            code.emit(0xe1c4_01b0);
        }
        GaxNativeLayout::V1_99 | GaxNativeLayout::V3Legacy | GaxNativeLayout::V3Modern => {
            anyhow::bail!("GAX bootstrap layout is not supported by this driver");
        }
    }
    code.emit(0xe3a0_0000);
    code.emit(0xe584_002c);
    code.literal(0, song.header.canonical_cpu_address);
    code.emit(0xe584_0030);
    if let Some(rate) = secondary::lower_rate(bytes, song)? {
        code.literal(0, u32::from(rate));
        code.emit(0xe1c4_00ba);
    }
    code.emit(0xe1a0_0004);
    code.call(song.native.init.cpu_address);
    Ok(())
}

pub(super) fn workspace(native: &GaxNativeProfile) -> Option<u32> {
    let length = workspace_bytes(native.layout);
    let hint = native.work_ram.saturating_add(RESERVED_IRQ_BYTES);
    let mut candidate =
        if (0x0300_0000..0x0300_7e00).contains(&hint) && hint.checked_add(length)? <= 0x0300_7e00 {
            hint
        } else {
            0x0300_0000 + RESERVED_IRQ_BYTES
        };
    for copy in &native.ram_copies {
        if (0x0300_0000..0x0300_8000).contains(&copy.destination) {
            candidate = candidate.max(
                copy.destination
                    .checked_add(copy.source.byte_len)?
                    .next_multiple_of(256),
            );
        }
    }
    (candidate.checked_add(length)? <= 0x0300_7e00).then_some(candidate)
}

fn workspace_bytes(layout: GaxNativeLayout) -> u32 {
    // Early engines allocate the song's reverb buffer inside this workspace.
    if layout == GaxNativeLayout::V2_01 {
        0x4000
    } else {
        0x2000
    }
}

pub(super) fn v2_workspace(bytes: &[u8], native: &GaxNativeProfile) -> Option<(u32, u32)> {
    let start = workspace(native)?;
    let minimum = workspace_bytes(native.layout);
    if native.layout != GaxNativeLayout::V2Current {
        return Some((start, minimum));
    }
    let (_, state) = super::state_slot(bytes, native.play.source.effective_offset as usize)?;
    let end = if (start..0x0300_7e00).contains(&state) {
        state
    } else {
        0x0300_7e00
    };
    let length = end.checked_sub(start)?;
    (length >= minimum).then_some((start, length))
}

pub(super) struct Program {
    words: Vec<u32>,
    literals: Vec<(usize, u32, u32)>,
}

impl Program {
    pub(super) fn new() -> Self {
        Self {
            words: Vec::new(),
            literals: Vec::new(),
        }
    }

    pub(super) fn emit(&mut self, word: u32) -> usize {
        let at = self.words.len();
        self.words.push(word);
        at
    }

    pub(super) fn literal(&mut self, register: u32, value: u32) -> usize {
        let instruction = self.emit(0);
        self.literals.push((instruction, register, value));
        instruction
    }

    fn replace_literal(&mut self, instruction: usize, value: u32) -> anyhow::Result<()> {
        let Some((_, _, current)) = self
            .literals
            .iter_mut()
            .find(|(candidate, _, _)| *candidate == instruction)
        else {
            anyhow::bail!("GAX bootstrap literal is missing");
        };
        *current = value;
        Ok(())
    }

    pub(super) fn call(&mut self, address: u32) {
        self.literal(12, address | 1);
        self.emit(0xe1a0_e00f);
        self.emit(0xe12f_ff1c);
    }

    fn branch(&mut self, instruction: usize, target: usize, condition: u32) {
        let relative = target as i32 - instruction as i32 - 2;
        self.words[instruction] = (condition << 28) | 0x0a00_0000 | (relative as u32 & 0x00ff_ffff);
    }

    fn byte_len(&self) -> Option<usize> {
        self.words
            .len()
            .checked_add(self.literals.len())?
            .checked_mul(4)
    }

    fn finish(mut self) -> anyhow::Result<Vec<u8>> {
        for &(instruction, register, value) in &self.literals {
            let relative = (self.words.len() as i32 - instruction as i32 - 2) * 4;
            ensure!(
                (0..4096).contains(&relative),
                "GAX bootstrap literal is out of range"
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
