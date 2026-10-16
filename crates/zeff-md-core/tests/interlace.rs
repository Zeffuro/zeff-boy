use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::{Emulator, LoadConfig, Region};

struct Rom {
    bytes: Vec<u8>,
    pc: usize,
}

impl Rom {
    fn word(&mut self, value: u16) {
        self.bytes[self.pc..self.pc + 2].copy_from_slice(&value.to_be_bytes());
        self.pc += 2;
    }

    fn long(&mut self, value: u32) {
        self.word((value >> 16) as u16);
        self.word(value as u16);
    }

    fn write(&mut self, address: u32, value: u16) {
        self.word(0x33fc);
        self.word(value);
        self.long(address);
    }

    fn register(&mut self, index: u16, value: u16) {
        self.write(0xc00004, 0x8000 | index << 8 | value);
    }

    fn command(&mut self, address: u16, code: u16) {
        self.write(0xc00004, address & 0x3fff | (code & 3) << 14);
        self.write(0xc00004, address >> 14 | (code & 0x3c) << 2);
    }
}

fn rom(h40: bool, v30: bool) -> Vec<u8> {
    let mut rom = Rom {
        bytes: vec![0; 0x4000],
        pc: 0x200,
    };
    rom.bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, value) in [
        (1, 4),
        (15, 2),
        (12, if h40 { 0x87 } else { 6 }),
        (2, 0x20),
        (3, 0x2c),
        (4, 5),
        (5, 0x60),
        (13, 0x34),
        (16, 1),
    ] {
        rom.register(index, value);
    }
    rom.command(0, 3);
    for color in [0, 0x000e, 0x0e00, 0x00e0] {
        rom.write(0xc00000, color);
    }
    rom.command(64, 1);
    for row in 0..16 {
        let value = if row % 2 == 0 { 0x1111 } else { 0x2222 };
        rom.write(0xc00000, value);
        rom.write(0xc00000, value);
    }
    for _ in 0..32 {
        rom.write(0xc00000, 0x3333);
    }
    rom.command(0x8000, 1);
    rom.word(0x303c);
    rom.word(2047);
    let fill = rom.pc;
    rom.write(0xc00000, 1);
    let branch = rom.pc;
    rom.word(0x51c8);
    rom.word((fill as i32 - (branch + 2) as i32) as i16 as u16);
    rom.command(0xc000, 1);
    for word in [257, 0, 0x8002, 136] {
        rom.write(0xc00000, word);
    }
    rom.command(0, 5);
    rom.write(0xc00000, 1);
    for byte in [0x81, 0x10, 0x90] {
        rom.word(0x13fc);
        rom.word(byte);
        rom.long(0xc00011);
    }
    rom.register(1, if v30 { 0x4c } else { 0x44 });
    rom.write(0xff0000, 0x1234);
    rom.word(0x60fe);
    rom.bytes
}

fn machine(region: Region, h40: bool, v30: bool) -> Emulator {
    Emulator::new(
        rom(h40, v30),
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap()
}

fn field(machine: &mut Emulator) {
    let initial = machine.pacing_field_count();
    for _ in 0..100_000 {
        machine.step_instruction().unwrap();
        if machine.pacing_field_count() != initial {
            assert_eq!(machine.video_error(), None);
            return;
        }
    }
    panic!("field did not complete");
}

fn ready(machine: &mut Emulator) {
    for _ in 0..100_000 {
        machine.step_instruction().unwrap();
        if machine.peek(0xff0000) == Some(0x12) && machine.peek(0xff0001) == Some(0x34) {
            for _ in 0..4 {
                field(machine);
            }
            return;
        }
    }
    panic!("guest did not finish video initialization");
}

fn pixel(machine: &Emulator, x: usize, y: usize) -> &[u8] {
    let offset = (y * machine.framebuffer_dimensions().0 + x) * 4;
    &machine.framebuffer()[offset..offset + 4]
}

fn picture(machine: &Emulator, width: usize, height: usize) {
    assert_eq!(machine.framebuffer_dimensions(), (width, height));
    for y in 0..height {
        assert_eq!(
            pixel(machine, 0, y),
            if y % 2 == 0 {
                &[0, 0, 255, 255]
            } else {
                &[255, 0, 0, 255]
            }
        );
        if (1..=16).contains(&y) {
            assert_eq!(pixel(machine, 8, y), &[0, 255, 0, 255]);
        }
    }
}

#[test]
fn original_rom_enters_mode2_weaves_fields_and_restores_midfield() {
    let mut first = machine(Region::NtscUs, true, false);
    ready(&mut first);
    picture(&first, 320, 448);
    let mut discarded = Vec::new();
    first.drain_audio_samples_into(&mut discarded);
    assert!(discarded.iter().any(|sample| *sample != 0.0));
    for _ in 0..2000 {
        first.step_instruction().unwrap();
    }
    let bytes = first.encode_state().unwrap();
    assert!(bytes.len() < zeff_md_core::constants::MAX_STATE_BYTES);
    let mut restored = machine(Region::NtscUs, true, false);
    restored.decode_state(&bytes).unwrap();
    // Loading restarts host audio, so compare both continuations at that boundary.
    first.decode_state(&bytes).unwrap();
    assert!(first == restored);
    for _ in 0..4000 {
        assert_eq!(first.step_instruction(), restored.step_instruction());
    }
    assert!(first == restored);
    assert_eq!(first.framebuffer(), restored.framebuffer());
    let (mut a, mut b) = (Vec::new(), Vec::new());
    first.drain_audio_samples_into(&mut a);
    restored.drain_audio_samples_into(&mut b);
    assert_eq!(a, b);
    assert!(!a.is_empty());
    assert_eq!(
        first.encode_state().unwrap(),
        restored.encode_state().unwrap()
    );
    let before = first.timing_snapshot().now();
    loop {
        if first.queue_vdp_register(12, 0x81).is_ok() {
            break;
        }
        first.step_instruction().unwrap();
        assert!(first.timing_snapshot().now().get() - before.get() < 1000);
    }
    for _ in 0..3 {
        field(&mut first);
    }
    assert_eq!(first.framebuffer_dimensions(), (320, 224));
    for y in 0..224 {
        assert_eq!(pixel(&first, 0, y), &[0, 0, 0, 255]);
    }
    first.reset();
    assert_eq!(first.pacing_field_count(), 0);
    assert_eq!(first.framebuffer_dimensions(), (256, 224));
}

#[test]
fn original_rom_mode2_covers_regions_widths_and_pal_height() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for v30 in [false, true] {
                if v30 && region != Region::Pal {
                    continue;
                }
                let mut machine = machine(region, h40, v30);
                ready(&mut machine);
                picture(
                    &machine,
                    if h40 { 320 } else { 256 },
                    if v30 { 480 } else { 448 },
                );
                let bytes = machine.encode_state().unwrap();
                let mut restored = machine.clone();
                restored.decode_state(&bytes).unwrap();
                machine.decode_state(&bytes).unwrap();
                field(&mut machine);
                field(&mut restored);
                assert!(machine == restored);
            }
        }
    }
}
