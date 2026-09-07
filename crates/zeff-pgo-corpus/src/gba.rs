//! Independently authored guest programs; all display/audio/RAM setup is
//! performed by ARM instructions, never by host-side emulator mutation.

const ENTRY: usize = 0xC0;
const HOT: usize = 0x800;
const ROM_BASE: u32 = 0x0800_0000;
const EWRAM_DATA: u32 = 0x0200_1000;
const IWRAM_DATA: u32 = 0x0300_4000;
const IWRAM_CODE: u32 = 0x0300_0000;
const STACK: u32 = 0x0300_7000;

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

struct Setup {
    code: Vec<u32>,
    literals: Vec<(usize, u32, u32)>,
}

impl Setup {
    fn new() -> Self {
        Self {
            code: Vec::new(),
            literals: Vec::new(),
        }
    }

    fn load(&mut self, register: u32, value: u32) {
        self.literals.push((self.code.len(), register, value));
        self.code.push(0);
    }

    fn write_halfword(&mut self, address: u32, value: u32) {
        self.load(0, address);
        self.load(1, value);
        self.code.push(0xE1C0_10B0); // STRH r1,[r0]
    }

    fn fill(&mut self, address: u32, value: u32, words: u32) {
        self.load(0, address);
        self.load(1, value);
        self.load(2, words);
        self.code.extend([0xE480_1004, 0xE252_2001, 0x1AFF_FFFC]);
    }

    fn finish(mut self, rom: &mut [u8]) {
        let literal_start = ENTRY + self.code.len() * 4;
        assert!(literal_start + self.literals.len() * 4 <= HOT);
        for (index, (instruction, register, value)) in self.literals.into_iter().enumerate() {
            let address = literal_start + index * 4;
            let pc = ENTRY + instruction * 4 + 8;
            assert!(address >= pc && address - pc <= 0xFFF);
            self.code[instruction] = 0xE59F_0000 | register << 12 | (address - pc) as u32;
            word(rom, address, value);
        }
        for (index, instruction) in self.code.into_iter().enumerate() {
            word(rom, ENTRY + index * 4, instruction);
        }
    }
}

fn header() -> Vec<u8> {
    let mut rom = vec![0; 0x1000];
    // Branch over the cartridge header. Do not place instructions/literals
    // across title, game code, fixed byte or complement checksum fields.
    word(&mut rom, 0, 0xEA00_002E);
    rom[0xA0..0xAB].copy_from_slice(b"ZPGO-CORPUS");
    rom[0xAC..0xB0].copy_from_slice(b"ZPGO");
    rom[0xB2] = 0x96;
    rom[0xBD] = rom[0xA0..0xBD]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_sub(*byte))
        .wrapping_sub(0x19);
    rom
}

fn setup() -> Setup {
    let mut arm = Setup::new();
    arm.write_halfword(0x0400_0000, 0x80); // forced blank during initialization
    // Two distinct text character blocks, deliberately including transparent
    // pixels so both first/second BG and OBJ composition receive real work.
    arm.fill(0x0600_0000, 0x1020_3040, 8);
    arm.fill(0x0600_4000, 0x5060_7080, 8);
    arm.fill(0x0601_0000, 0x9090_A0B0, 8);
    arm.fill(0x0600_C000, 0, 512); // screen blocks 24 and 25 use tile zero
    arm.fill(0x0600_C800, 0, 512);
    arm.fill(0x0700_0000, 0x0000_0200, 256); // disable every unused OBJ
    for index in 1..16 {
        let color = ((index * 2) & 31) | (((31 - index) & 31) << 5) | ((index & 31) << 10);
        arm.write_halfword(0x0500_0000 + index * 2, color);
        arm.write_halfword(0x0500_0200 + index * 2, color ^ 0x3DEF);
    }
    for index in 0..8 {
        arm.write_halfword(0x0700_0000 + index * 8, 20 + index * 16);
        arm.write_halfword(0x0700_0002 + index * 8, 16 + index * 24);
        arm.write_halfword(0x0700_0004 + index * 8, 0); // tile zero, priority zero
    }
    for (address, value) in [
        (0x0400_0008, 0x1801), // BG0: char0, screen24, priority1
        (0x0400_000A, 0x1906), // BG1: char1, screen25, priority2
        (0x0400_0014, 3),      // nonaligned BG1 horizontal scroll
        (0x0400_0084, 0x80),
        (0x0400_0080, 0xFF77),
        (0x0400_0060, 0x08),
        (0x0400_0062, 0xF080),
        (0x0400_0064, 0x83FF), // trigger, length disabled: survives training warmup
        (0x0400_0100, 0xFFC0),
        (0x0400_0102, 0x81),
        (0x0400_0000, 0x1340), // Mode0, BG0+BG1+OBJ, 1D OBJ tiles
    ] {
        arm.write_halfword(address, value);
    }
    arm
}

fn arm_hot_loop() -> Vec<u32> {
    let mut code = vec![
        0xE794_0006, // LDR r0,[r4,r6]
        0xE280_0001, // ADD r0,r0,#1
        0xE020_11A0, // EOR r1,r0,r0,LSR #3
        0xE091_2087, // ADDS r2,r1,r7,LSL #1
        0x1022_2000, // EORNE r2,r2,r0
        0xE784_2006, // STR r2,[r4,r6]
        0xE085_3006, // ADD r3,r5,r6
        0xE1C3_20B0, // STRH r2,[r3]
        0xE1D3_10B0, // LDRH r1,[r3]
        0xE1D3_20F0, // LDRSH r2,[r3]
        0xE583_1004, // STR r1,[r3,#4]
        0xE593_0004, // LDR r0,[r3,#4]
        0xE883_0007, // STMIA r3,{r0-r2}: bounded three-word block
        0xE893_0007, // LDMIA r3,{r0-r2}
        0xE000_0192, // MUL r0,r2,r1
        0xE58D_0000, // STR r0,[sp]
        0xE59D_2000, // LDR r2,[sp]
        0xE287_7001, // ADD r7,r7,#1
        0xE317_0001, // TST r7,#1
        0x0280_0003, // ADDEQ r0,r0,#3
        0x1280_0007, // ADDNE r0,r0,#7
        0xE286_6004, // ADD r6,r6,#4
        0xE006_6008, // AND r6,r6,r8: bounded offset, not an escaping pointer
    ];
    let displacement = -(code.len() as i32 + 2);
    code.push(0xEA00_0000 | (displacement as u32 & 0x00FF_FFFF));
    code
}

fn arm_rom(ram_code: bool) -> Vec<u8> {
    let mut rom = header();
    let hot = arm_hot_loop();
    for (index, instruction) in hot.iter().enumerate() {
        word(&mut rom, HOT + index * 4, *instruction);
    }
    let mut arm = setup();
    if ram_code {
        arm.load(0, ROM_BASE + HOT as u32);
        arm.load(1, IWRAM_CODE);
        arm.load(2, hot.len() as u32);
        arm.code
            .extend([0xE490_3004, 0xE481_3004, 0xE252_2001, 0x1AFF_FFFB]);
    }
    for (register, value) in [
        (4, EWRAM_DATA),
        (5, IWRAM_DATA),
        (6, 0),
        (7, 0),
        (8, 0x3FC),
        (13, STACK),
    ] {
        arm.load(register, value);
    }
    arm.load(
        0,
        if ram_code {
            IWRAM_CODE
        } else {
            ROM_BASE + HOT as u32
        },
    );
    arm.code.push(0xE12F_FF10);
    arm.finish(&mut rom);
    rom
}

pub(super) fn arm_active_rom() -> Vec<u8> {
    arm_rom(false)
}
pub(super) fn memory_rom() -> Vec<u8> {
    arm_rom(true)
}

pub(super) fn thumb_active_rom() -> Vec<u8> {
    let mut rom = header();
    let mut arm = setup();
    for (register, value) in [
        (4, EWRAM_DATA),
        (5, IWRAM_DATA),
        (6, 0),
        (7, 0),
        (13, STACK),
    ] {
        arm.load(register, value);
    }
    arm.load(0, ROM_BASE + HOT as u32 + 1);
    arm.code.push(0xE12F_FF10);
    arm.finish(&mut rom);
    let mut hot = vec![
        0x19A3u16, // ADD r3,r4,r6 (EWRAM bounded indexed address)
        0x8818,    // LDRH r0,[r3]
        0x3001,    // ADD r0,#1
        0x08C1,    // LSR r1,r0,#3
        0x4048,    // EOR r0,r1
        0x3701,    // ADD r7,#1
        0x007A,    // LSL r2,r7,#1
        0x1880,    // ADD r0,r0,r2
        0x420F,    // TST r7,r1
        0xD000,    // BEQ skips one arithmetic instruction
        0x4078,    // EOR r0,r7
        0x8018,    // STRH r0,[r3]
        0x8058,    // STRH r0,[r3,#2]
        0x8859,    // LDRH r1,[r3,#2]
        0x9100,    // STR r1,[sp]
        0x9A00,    // LDR r2,[sp]
        0x19AB,    // ADD r3,r5,r6 (IWRAM bounded indexed address)
        0x8819,    // LDRH r1,[r3]
        0x1809,    // ADD r1,r1,r0
        0x8019,    // STRH r1,[r3]
        0x3604,    // ADD r6,#4
        0x22FC,    // MOV r2,#0xFC
        0x4016,    // AND r6,r2 (bounded 256-byte working set)
    ];
    let displacement = -(hot.len() as i32 + 2);
    hot.push(0xE000 | (displacement as u16 & 0x7FF));
    for (index, instruction) in hot.into_iter().enumerate() {
        rom[HOT + index * 2..HOT + index * 2 + 2].copy_from_slice(&instruction.to_le_bytes());
    }
    rom
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_gba_core::emulator::Emulator;
    use zeff_gba_core::hardware::bus::DebugTraceEvent;

    #[test]
    fn gba_owned_programs_keep_header_and_complement_checksum_intact() {
        for rom in [arm_active_rom(), thumb_active_rom(), memory_rom()] {
            assert_eq!(&rom[0xA0..0xAB], b"ZPGO-CORPUS");
            assert_eq!(&rom[0xAC..0xB0], b"ZPGO");
            assert_eq!(rom[0xB2], 0x96);
            let sum = rom[0xA0..=0xBD]
                .iter()
                .fold(0x19u8, |sum, byte| sum.wrapping_add(*byte));
            assert_eq!(sum, 0);
            let entry = u32::from_le_bytes(rom[..4].try_into().unwrap());
            assert_eq!(8 + (entry & 0xFFFFFF) * 4, ENTRY as u32);
        }
    }

    #[test]
    fn gba_guest_hot_loops_stay_bounded_and_exercise_ram_display_and_audio() {
        for (rom, thumb, ram_code) in [
            (arm_active_rom(), false, false),
            (thumb_active_rom(), true, false),
            (memory_rom(), false, true),
        ] {
            let mut emu = Emulator::from_rom_data(&rom).unwrap();
            emu.set_apu_sample_generation_enabled(true);
            let code_base = if ram_code {
                IWRAM_CODE
            } else {
                ROM_BASE + HOT as u32
            };
            let mut audio = Vec::new();
            let mut nonzero_audio = false;
            for frame in 0..122 {
                emu.step_frame();
                let regs = emu.cpu_registers();
                assert!(
                    (code_base..code_base + 0x80).contains(&emu.cpu_pc()),
                    "unexpected PC {:08X}",
                    emu.cpu_pc()
                );
                assert_eq!(emu.cpu_thumb_state(), thumb);
                assert_eq!(regs[4], EWRAM_DATA);
                assert_eq!(regs[5], IWRAM_DATA);
                assert_eq!(regs[13], STACK);
                assert!(regs[6] <= if thumb { 0x100 } else { 0x400 });
                assert!(
                    regs[3] >= EWRAM_DATA && regs[3] < EWRAM_DATA + 0x400
                        || regs[3] >= IWRAM_DATA && regs[3] < IWRAM_DATA + 0x400
                );
                emu.drain_audio_samples_into(&mut audio);
                nonzero_audio |= audio.iter().any(|sample| *sample != 0.0);
                if frame >= 119 {
                    assert!(
                        audio.iter().any(|sample| *sample != 0.0),
                        "PSG must stay audible after warmup"
                    );
                }
            }
            assert!(nonzero_audio);
            assert_eq!(emu.cpu_peek16(0x0400_0000), 0x1340);
            assert_eq!(emu.cpu_peek16(0x0400_0008), 0x1801);
            assert_eq!(emu.cpu_peek16(0x0400_000A), 0x1906);
            let colors: std::collections::BTreeSet<_> = emu
                .framebuffer()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| [pixel[0], pixel[1], pixel[2]])
                .collect();
            assert!(
                colors.len() >= 4,
                "Mode0 layers should produce varied colors"
            );

            // Inspect actual bus events after guest setup. This independently
            // catches an opcode/addressing error or a pointer escaping the
            // intended data sets, instead of trusting an encoded branch label.
            let mut ram_reads = [0; 2];
            let mut ram_writes = [0; 2];
            for _ in 0..2048 {
                let (instruction, events) = emu.step_instruction_with_bus_trace(true, true);
                let instruction = instruction.expect("hot loop remains executable");
                assert!((code_base..code_base + 0x80).contains(&instruction.pc));
                for event in events {
                    let address = event.addr();
                    let region = if (EWRAM_DATA..EWRAM_DATA + 0x40C).contains(&address) {
                        Some(0)
                    } else if (IWRAM_DATA..IWRAM_DATA + 0x40C).contains(&address) {
                        Some(1)
                    } else {
                        None
                    };
                    match event {
                        DebugTraceEvent::Write { .. } => {
                            assert!(
                                region.is_some() || (STACK..STACK + 4).contains(&address),
                                "write escaped RAM: {address:08X}"
                            );
                            if let Some(region) = region {
                                ram_writes[region] += 1;
                            }
                        }
                        DebugTraceEvent::Read { .. } => {
                            assert!(
                                region.is_some()
                                    || (STACK..STACK + 4).contains(&address)
                                    || (code_base..code_base + 0x88).contains(&address),
                                "read escaped data/code: {address:08X}"
                            );
                            if let Some(region) = region {
                                ram_reads[region] += 1;
                            }
                        }
                    }
                }
            }
            assert!(ram_reads.into_iter().all(|count| count > 0));
            assert!(ram_writes.into_iter().all(|count| count > 0));
        }
    }
}
