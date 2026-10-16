use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_m68k::timing::TimingFault;
use zeff_md_core::{
    BusError, Emulator, LoadConfig, MachineFault, MachineStep, Region, VdpDmaError, VdpPortError,
    VdpScheduleError,
};

pub struct Program {
    pub image: Vec<u8>,
    pub triggers: [u32; 2],
}

fn register(words: &mut Vec<u16>, index: u16, value: u8) {
    words.extend([0x33fc, 0x8000 | index << 8 | u16::from(value), 0x00c0, 4]);
}

fn command(words: &mut Vec<u16>, address: u32, code: u8) -> u32 {
    let pc = 0x100 + words.len() as u32 * 2;
    let first = (address as u16 & 0x3fff) | u16::from(code & 3) << 14;
    let second = ((address >> 14) as u16 & 7) | u16::from(code & 0x3c) << 2;
    words.extend([0x23fc, first, second, 0x00c0, 4]);
    pc
}

pub fn program(h40: bool, enabled: bool, source_high: u8) -> Program {
    program_height(h40, enabled, source_high, false)
}

fn program_height(h40: bool, enabled: bool, source_high: u8, tall: bool) -> Program {
    let mut words = Vec::new();
    for (index, value) in [
        (12, if h40 { 0x81 } else { 0 }),
        (
            1,
            (if enabled { 0x14 } else { 4 }) | if tall { 8 } else { 0 },
        ),
        (15, 2),
        (19, 4),
        (20, 0),
        (21, 0),
        (22, 8),
        (23, source_high),
    ] {
        register(&mut words, index, value);
    }
    let first = command(&mut words, 0x2001, 0x21);
    words.extend([
        0x33fc, 0x0eee, 0x00ff, 0x1000, 0x33fc, 0x000e, 0x00ff, 0x1002,
    ]);
    for (index, value) in [(19, 2), (20, 0), (21, 0), (22, 0x88), (23, 0x7f)] {
        register(&mut words, index, value);
    }
    let second = command(&mut words, 0, 0x23);
    register(
        &mut words,
        1,
        (if enabled { 0x54 } else { 0x44 }) | if tall { 8 } else { 0 },
    );
    words.extend([0x33fc, 0xbeef, 0x00ff, 0x2000, 0x60fe, 0x4e71]);
    let mut image = vec![0; 0x4000];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.into_iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    image[0x1000..0x1008].copy_from_slice(&[0x12, 0x34, 0xab, 0xcd, 0x56, 0x78, 0x9a, 0xbc]);
    Program {
        image,
        triggers: [first, second],
    }
}

fn setup(
    machine: &mut Emulator,
    triggers: [u32; 2],
    step: &mut impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) -> [u64; 2] {
    let mut clocks = [0; 2];
    for _ in 0..100 {
        let pc = machine.registers().pc;
        let result = step(machine).unwrap();
        if let Some(index) = triggers.iter().position(|&trigger| trigger == pc) {
            clocks[index] = result.elapsed.get();
        }
        if machine.peek(0xff2000) == Some(0xbe) {
            return clocks;
        }
    }
    panic!("DMA program did not complete");
}

pub fn actual_rom_and_ram_dma_to_frame_with(
    step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    actual_rom_and_ram_dma_to_frame_in_region(Region::NtscUs, step);
}

pub fn actual_rom_and_ram_dma_to_frame_in_region(
    region: Region,
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent_sound in [false, true] {
        for h40 in [false, true] {
            for tall in [false, true] {
                if tall && region != Region::Pal {
                    continue;
                }
                let disabled = program_height(h40, false, 0, tall);
                let config = LoadConfig {
                    region,
                    concurrent_sound,
                };
                let mut baseline = Emulator::new(disabled.image, config).unwrap();
                let nominal = setup(&mut baseline, disabled.triggers, &mut step);
                let program = program_height(h40, true, 0, tall);
                let mut machine = Emulator::new(program.image, config).unwrap();
                let elapsed = setup(&mut machine, program.triggers, &mut step);
                assert!(
                    elapsed
                        .iter()
                        .zip(nominal)
                        .all(|(&actual, normal)| actual > normal)
                );
                while machine.frame_count() < 2 {
                    step(&mut machine).unwrap();
                }
                assert_eq!(
                    &machine.vdp().memory().vram()[0x2000..0x2008],
                    &[0x34, 0x12, 0xcd, 0xab, 0x78, 0x56, 0xbc, 0x9a]
                );
                assert_eq!(&machine.vdp().memory().cram()[..2], &[0x0eee, 0x000e]);
                assert_eq!(machine.peek(0xff2001), Some(0xef));
                assert_eq!(
                    machine.framebuffer_dimensions(),
                    (if h40 { 320 } else { 256 }, if tall { 240 } else { 224 })
                );
                assert!(
                    machine
                        .framebuffer()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|pixel| *pixel == [255; 4])
                );
                assert!(machine.video_error().is_none());
                assert!(machine.vdp().fifo().is_empty());
                let mut copy = machine.clone();
                assert_eq!(step(&mut machine), step(&mut copy));
                assert_eq!(machine, copy);
                let before = machine.psg_snapshot();
                let dimensions = machine.framebuffer_dimensions();
                machine.reset_cpu().unwrap();
                assert_eq!(machine.framebuffer_dimensions(), dimensions);
                assert_eq!(machine.psg_snapshot().volumes, before.volumes);
                Reset::reset(&mut machine);
                assert!(machine.vdp().memory().vram().iter().all(|&byte| byte == 0));
                assert_eq!(machine.timing_snapshot().now().get(), 56);
            }
        }
    }
}

pub fn actual_rom_and_ram_dma_to_frame() {
    actual_rom_and_ram_dma_to_frame_with(Emulator::step_instruction);
}

pub fn pal_rom_and_ram_dma_to_frame() {
    actual_rom_and_ram_dma_to_frame_in_region(Region::Pal, Emulator::step_instruction);
}

pub fn unsupported_source_freezes_without_deposit() {
    for concurrent_sound in [false, true] {
        let program = program(false, true, 0x20);
        let mut machine = Emulator::new(
            program.image,
            LoadConfig {
                concurrent_sound,
                ..Default::default()
            },
        )
        .unwrap();
        while machine.registers().pc != program.triggers[0] {
            machine.step_instruction().unwrap();
        }
        assert!(matches!(
            machine.step_instruction(),
            Err(MachineFault::Main(TimingFault::Bus {
                source: BusError::VdpPort(VdpPortError::Schedule(VdpScheduleError::Dma(
                    VdpDmaError::UnsupportedSource(0x401000)
                ))),
                ..
            }))
        ));
        assert!(machine.vdp().memory().vram().iter().all(|&byte| byte == 0));
        let saved = machine.clone();
        assert_eq!(
            machine.step_instruction(),
            Err(MachineFault::Main(TimingFault::Frozen))
        );
        assert_eq!(machine, saved);
        machine.reset_cpu().unwrap();
        assert_eq!(machine.registers().pc, 0x100);
    }
}
