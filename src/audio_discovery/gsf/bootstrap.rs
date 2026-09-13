use anyhow::{Result, ensure};

#[derive(Clone, Copy)]
pub(crate) struct Callbacks {
    pub(crate) init: u32,
    pub(crate) init_r0: Option<u32>,
    pub(crate) select: u32,
    pub(crate) main: u32,
    pub(crate) prime_main_after_init: bool,
    pub(crate) main_in_vblank: bool,
    pub(crate) vsync: Option<u32>,
    pub(crate) dma1: Option<u32>,
    pub(crate) dma2: Option<u32>,
}

pub(crate) struct Code {
    pub(crate) bytes: Vec<u8>,
    pub(crate) song_literal_offset: usize,
    pub(crate) wait_offset: usize,
    pub(crate) irq_offset: usize,
}

pub(crate) fn build(base: u32, song: u32, callbacks: Callbacks) -> Result<Code> {
    ensure!(base.is_multiple_of(4), "GSF bootstrap is not word aligned");
    let mut code = ArmCode::default();
    code.emit(0xE3A0_00D2); // Separate IRQ and system stacks, with interrupts masked during setup.
    code.emit(0xE121_F000);
    code.literal(13, 0x0300_7FA0);
    code.emit(0xE3A0_00DF);
    code.emit(0xE121_F000);
    code.literal(13, 0x0300_7F00);
    if let Some(init_r0) = callbacks.init_r0 {
        code.literal(0, init_r0);
    }
    code.call(callbacks.init);
    if callbacks.prime_main_after_init {
        code.call(callbacks.main);
    }

    code.literal(0, 0x0300_7FFC);
    let irq_literal = code.literal(1, 0);
    code.emit(0xE580_1000);
    code.emit(0xE3A0_2000);
    code.emit(0xE500_2004);
    let irq_mask = 1
        | (u32::from(callbacks.dma1.is_some()) << 9)
        | (u32::from(callbacks.dma2.is_some()) << 10);
    let gate_main = irq_mask != 1 && !callbacks.main_in_vblank;
    if gate_main {
        code.literal(0, 0x0300_7FF4);
        code.emit(0xE580_2000);
    }
    code.literal(0, 0x0400_0004);
    code.emit(0xE3A0_1008);
    code.emit(0xE1C0_10B0);
    code.literal(0, 0x0400_0200);
    if irq_mask == 1 {
        code.emit(0xE3A0_1001);
    } else {
        code.literal(1, irq_mask);
    }
    code.emit(0xE1C0_10B0);
    code.emit(0xE1C0_10B2);
    if irq_mask != 1 {
        code.emit(0xE3A0_1001);
    }
    code.emit(0xE1C0_10B8);
    code.emit(0xE3A0_001F);
    code.emit(0xE121_F000);
    let song_literal = code.literal(0, song);
    code.call(callbacks.select);
    let wait_offset;
    if irq_mask == 1 {
        code.emit(0xEF05_0000); // The driver starts its timer at line 159; begin mixing on the next VBlank.
        let loop_word = code.words.len();
        code.call(callbacks.main);
        wait_offset = code.words.len() * 4;
        code.emit(0xEF05_0000); // ARM SWI numbers occupy bits 16..23.
        let loop_branch = code.emit(0);
        code.branch(loop_branch, loop_word, 0xEA00_0000)?;
    } else {
        let wait_word = code.words.len();
        wait_offset = wait_word * 4;
        code.emit(0xEF05_0000);
        if gate_main {
            code.literal(0, 0x0300_7FF4);
            code.emit(0xE590_1000);
            code.emit(0xE351_0000);
            let wait_if_dma = code.emit(0);
            code.emit(0xE3A0_1000);
            code.emit(0xE580_1000);
            code.call(callbacks.main);
            code.branch(wait_if_dma, wait_word, 0x0A00_0000)?;
        }
        let loop_branch = code.emit(0);
        code.branch(loop_branch, wait_word, 0xEA00_0000)?;
    }

    let irq_offset = code.words.len() * 4;
    code.literals[irq_literal].1 = base + irq_offset as u32;
    code.emit(0xE92D_501F);
    code.literal(2, 0x0400_0200);
    code.emit(0xE592_3000);
    code.emit(0xE003_4823);
    if irq_mask == 1 {
        if let Some(vsync) = callbacks.vsync {
            code.emit(0xE314_0001);
            let skip_callback = code.emit(0);
            code.call(vsync);
            let acknowledge = code.words.len();
            code.branch(skip_callback, acknowledge, 0x0A00_0000)?;
        }
        code.literal(2, 0x0400_0200);
        code.emit(0xE1C2_40B2);
    } else {
        // Match the game dispatcher: service one source and acknowledge it before its callback.
        let mut finished = Vec::new();
        if let Some(vsync) = callbacks.vsync {
            code.emit(0xE314_0001);
            let next = code.emit(0);
            code.emit(0xE3A0_0001);
            code.emit(0xE1C2_00B2);
            code.emit(0xE1A0_4000);
            code.call(vsync);
            if callbacks.main_in_vblank {
                code.call(callbacks.main);
            } else if gate_main {
                code.literal(0, 0x0300_7FF4);
                code.emit(0xE3A0_1001);
                code.emit(0xE580_1000);
            }
            finished.push(code.emit(0));
            let next_callback = code.words.len();
            code.branch(next, next_callback, 0x0A00_0000)?;
        }
        for (mask, callback) in [(0x200, callbacks.dma1), (0x400, callbacks.dma2)] {
            if let Some(callback) = callback {
                code.literal(0, mask);
                code.emit(0xE114_0000);
                let next = code.emit(0);
                code.emit(0xE1C2_00B2);
                code.emit(0xE1A0_4000);
                code.call(callback);
                finished.push(code.emit(0));
                let next_callback = code.words.len();
                code.branch(next, next_callback, 0x0A00_0000)?;
            }
        }
        let finish = code.words.len();
        for branch in finished {
            code.branch(branch, finish, 0xEA00_0000)?;
        }
    }
    code.literal(2, 0x0300_7FF8);
    code.emit(0xE592_3000);
    code.emit(0xE183_3004); // Preserve other BIOS interrupt flags used by IntrWait.
    code.emit(0xE582_3000);
    code.emit(0xE8BD_501F);
    code.emit(0xE12F_FF1E);

    let song_literal_offset = (code.words.len() + song_literal) * 4;
    Ok(Code {
        bytes: code.finish()?,
        song_literal_offset,
        wait_offset,
        irq_offset,
    })
}

#[derive(Default)]
struct ArmCode {
    words: Vec<u32>,
    literals: Vec<(usize, u32)>,
}

impl ArmCode {
    fn emit(&mut self, word: u32) -> usize {
        let offset = self.words.len();
        self.words.push(word);
        offset
    }

    fn literal(&mut self, register: u32, value: u32) -> usize {
        let instruction = self.emit(0xE59F_0000 | (register << 12));
        let index = self.literals.len();
        self.literals.push((instruction, value));
        index
    }

    fn call(&mut self, address: u32) {
        self.literal(12, address);
        self.emit(0xE1A0_E00F);
        self.emit(0xE12F_FF1C);
    }

    fn branch(&mut self, from: usize, to: usize, opcode: u32) -> Result<()> {
        let displacement = to as i64 - from as i64 - 2;
        ensure!(
            (-0x80_0000..0x80_0000).contains(&displacement),
            "GSF bootstrap branch is out of range"
        );
        self.words[from] = opcode | (displacement as u32 & 0x00FF_FFFF);
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>> {
        for (instruction, value) in self.literals {
            let displacement = self.words.len() * 4 - (instruction * 4 + 8);
            ensure!(displacement <= 0xFFF, "GSF literal pool is out of range");
            self.words[instruction] |= displacement as u32;
            self.words.push(value);
        }
        Ok(self.words.into_iter().flat_map(u32::to_le_bytes).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(rom: &mut [u8], at: usize, words: &[u32]) {
        for (slot, value) in rom[at..].as_chunks_mut::<4>().0.iter_mut().zip(words) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
    }

    #[test]
    fn independent_arm_bootstrap_calls_driver_and_services_vblank() -> Result<()> {
        let mut rom = vec![0; 0x1000];
        rom[0xB2] = 0x96;
        // ARM stubs store initialization, the selected number, main calls, and VBlank calls.
        store(
            &mut rom,
            0x200,
            &[
                0xE59F_201C,
                0xE1D2_00B0,
                0xE350_009F,
                0x1AFF_FFFC,
                0xE59F_1010,
                0xE3A0_0001,
                0xE581_0000,
                0xE12F_FF1E,
                0xE1A0_0000,
                0x0400_0006,
                0x0200_0000,
            ],
        );
        store(
            &mut rom,
            0x240,
            &[0xE59F_1004, 0xE581_0004, 0xE12F_FF1E, 0x0200_0000],
        );
        for (start, slot) in [(0x280, 8u32), (0x2C0, 12u32)] {
            store(
                &mut rom,
                start,
                &[
                    0xE59F_1010,
                    0xE591_0000 | slot,
                    0xE280_0001,
                    0xE581_0000 | slot,
                    0xE12F_FF1E,
                    0xE1A0_0000,
                    0x0200_0000,
                ],
            );
        }
        let callbacks = Callbacks {
            init: 0x0800_0200,
            init_r0: None,
            select: 0x0800_0240,
            main: 0x0800_0280,
            prime_main_after_init: false,
            main_in_vblank: false,
            vsync: Some(0x0800_02C0),
            dma1: None,
            dma2: None,
        };
        for song in [7, 257] {
            let code = build(0x0800_1000, song, callbacks)?;
            let mut image = rom.clone();
            image[..4].copy_from_slice(&0xEA00_03FEu32.to_le_bytes());
            image.extend_from_slice(&code.bytes);
            let mut emulator = zeff_gba_core::emulator::Emulator::new(&image, 48_000)?;
            let mut main_cycles = Vec::new();
            let mut acknowledged = 0;
            for _ in 0..2_000_000 {
                if emulator.cpu_cycles() >= 280_896 * 8 {
                    break;
                }
                if let Some(instruction) = emulator.step_instruction() {
                    if instruction.pc == callbacks.main {
                        main_cycles.push(emulator.cpu_cycles());
                    }
                    if instruction.pc == 0x0800_1000 + code.irq_offset as u32 + 64 {
                        assert_eq!(emulator.cpu_peek16(0x0400_0202) & 1, 0);
                        assert_eq!(emulator.cpu_peek32(0x0300_7FF8) & 1, 1);
                        acknowledged += 1;
                    }
                }
            }
            assert!(main_cycles.len() >= 6 && acknowledged >= 6);
            assert!(
                main_cycles
                    .windows(2)
                    .all(|pair| pair[1] - pair[0] >= 280_800)
            );
            assert_eq!(emulator.cpu_peek32(0x0200_0000), 1);
            assert_eq!(emulator.cpu_peek32(0x0200_0004), song);
            assert!(emulator.cpu_peek32(0x0200_0008) >= 6);
            assert!(emulator.cpu_peek32(0x0200_000C) >= 6);
            assert_eq!(
                emulator.cpu_peek32(0x0300_7FFC),
                0x0800_1000 + code.irq_offset as u32
            );
            assert_eq!(emulator.cpu_peek16(0x0400_0200), 1);
            assert_eq!(emulator.cpu_peek16(0x0400_0208), 1);
            assert!(
                emulator
                    .cpu_pc()
                    .abs_diff(0x0800_1000 + code.wait_offset as u32)
                    <= 8
            );
        }
        Ok(())
    }

    #[test]
    fn dma_irq_during_wait_does_not_advance_main_before_vblank() -> Result<()> {
        let mut rom = vec![0; 0x2000];
        rom[0xB2] = 0x96;
        store(&mut rom, 0x200, &[0xE12F_FF1E]);
        // Select stores the song and schedules one HBlank DMA transfer after the wait begins.
        store(
            &mut rom,
            0x240,
            &[
                0xE59F_2020,
                0xE582_000C,
                0xE59F_101C,
                0xE59F_001C,
                0xE581_00BC,
                0xE59F_0018,
                0xE581_00C0,
                0xE59F_0014,
                0xE581_00C4,
                0xE12F_FF1E,
                0x0200_0000,
                0x0400_0000,
                0x0200_0020,
                0x0200_0024,
                0xE000_0001,
            ],
        );
        for (start, slot) in [(0x300, 0u32), (0x340, 4u32), (0x380, 8u32)] {
            store(
                &mut rom,
                start,
                &[
                    0xE59F_1010,
                    0xE591_0000 | slot,
                    0xE280_0001,
                    0xE581_0000 | slot,
                    0xE12F_FF1E,
                    0xE1A0_0000,
                    0x0200_0000,
                ],
            );
        }
        for main_in_vblank in [false, true] {
            let prime_main_after_init = main_in_vblank;
            let code = build(
                0x0800_2000,
                19,
                Callbacks {
                    init: 0x0800_0200,
                    init_r0: None,
                    select: 0x0800_0240,
                    main: 0x0800_0300,
                    prime_main_after_init,
                    main_in_vblank,
                    vsync: Some(0x0800_0340),
                    dma1: Some(0x0800_0380),
                    dma2: None,
                },
            )?;
            let mut image = rom.clone();
            image[..4].copy_from_slice(&0xEA00_07FEu32.to_le_bytes());
            image.extend_from_slice(&code.bytes);
            let mut emulator = zeff_gba_core::emulator::Emulator::new(&image, 48_000)?;
            for _ in 0..3_000_000 {
                if emulator.cpu_cycles() >= 280_896 * 8 {
                    break;
                }
                emulator.step_instruction();
            }
            let main_calls = emulator.cpu_peek32(0x0200_0000);
            let vblank_calls = emulator.cpu_peek32(0x0200_0004);
            assert!(vblank_calls >= 6, "main_in_vblank={main_in_vblank}");
            assert_eq!(
                main_calls,
                vblank_calls + u32::from(prime_main_after_init),
                "main_in_vblank={main_in_vblank}"
            );
            assert_eq!(emulator.cpu_peek32(0x0200_0008), 1);
            assert_eq!(emulator.cpu_peek32(0x0200_000C), 19);
        }
        Ok(())
    }

    #[test]
    fn optional_prime_consumes_setup_before_selecting_the_requested_song() -> Result<()> {
        let mut rom = vec![0; 0x1000];
        rom[0xB2] = 0x96;
        // Init queues setup state 1. Main records a consumed state at base + state * 4.
        store(
            &mut rom,
            0x200,
            &[
                0xE59F_1008,
                0xE3A0_0001,
                0xE581_0000,
                0xE12F_FF1E,
                0x0200_0000,
            ],
        );
        store(
            &mut rom,
            0x240,
            &[
                0xE59F_100C,
                0xE3A0_2002,
                0xE581_2000,
                0xE581_000C,
                0xE12F_FF1E,
                0x0200_0000,
            ],
        );
        store(
            &mut rom,
            0x280,
            &[
                0xE59F_1010,
                0xE591_0000,
                0xE781_0100,
                0xE3A0_2000,
                0xE581_2000,
                0xE12F_FF1E,
                0x0200_0000,
            ],
        );
        store(&mut rom, 0x2C0, &[0xE12F_FF1E]);
        let code = build(
            0x0800_1000,
            19,
            Callbacks {
                init: 0x0800_0200,
                init_r0: None,
                select: 0x0800_0240,
                main: 0x0800_0280,
                prime_main_after_init: true,
                main_in_vblank: true,
                vsync: Some(0x0800_02C0),
                dma1: Some(0x0800_02C0),
                dma2: None,
            },
        )?;
        let mut image = rom;
        image[..4].copy_from_slice(&0xEA00_03FEu32.to_le_bytes());
        image.extend_from_slice(&code.bytes);
        let mut emulator = zeff_gba_core::emulator::Emulator::new(&image, 48_000)?;
        while emulator.cpu_cycles() < 280_896 * 2 {
            emulator.step_instruction();
        }
        assert_eq!(emulator.cpu_peek32(0x0200_0004), 1);
        assert_eq!(emulator.cpu_peek32(0x0200_0008), 2);
        assert_eq!(emulator.cpu_peek32(0x0200_000C), 19);
        Ok(())
    }
}
