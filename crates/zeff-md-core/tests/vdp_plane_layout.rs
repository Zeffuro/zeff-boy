use zeff_emu_common::save_state::StateReader;
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

    fn write(&mut self, address: u32, value: u16) {
        self.word(0x33fc);
        self.word(value);
        self.word((address >> 16) as u16);
        self.word(address as u16);
    }

    fn register(&mut self, index: u16, value: u16) {
        self.write(0xc00004, 0x8000 | index << 8 | value);
    }

    fn command(&mut self, address: u16, code: u16) {
        self.write(0xc00004, address & 0x3fff | (code & 3) << 14);
        self.write(0xc00004, address >> 14 | (code & 0x3c) << 2);
    }

    fn fill(&mut self, address: u16, count: u16, value: u16) {
        self.command(address, 1);
        self.word(0x303c);
        self.word(count - 1);
        let start = self.pc;
        self.write(0xc00000, value);
        let branch = self.pc;
        self.word(0x51c8);
        self.word((start as i32 - (branch + 2) as i32) as i16 as u16);
    }
}

fn rom(size: u8, h40: bool, interlaced: bool) -> Vec<u8> {
    let mut rom = Rom {
        bytes: vec![0; 0x4000],
        pc: 0x200,
    };
    rom.bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, value) in [
        (1, 4),
        (15, 2),
        (
            12,
            if h40 { 0x81 } else { 0 } | if interlaced { 6 } else { 0 },
        ),
        (2, 0x20),
        (3, 0x34),
        (4, 5),
        (5, 0x30),
        (7, 0),
        (11, 0),
        (13, 0x1c),
        (16, u16::from(size)),
        (17, 0),
        (18, 0),
    ] {
        rom.register(index, value);
    }
    rom.command(0, 3);
    for value in [0, 0x00e, 0xe00, 0x0e0, 0xeee, 0x0ee, 0xee0] {
        rom.write(0xc00000, value);
    }
    let height = if interlaced { 16 } else { 8 };
    for tile in 1..=3 {
        rom.command(tile * height * 4, 1);
        for row in 0..height {
            let color = (tile * 2 - 1 + row % 2) * 0x1111;
            rom.write(0xc00000, color);
            rom.write(0xc00000, color);
        }
    }
    for base in [0x8000, 0xa000] {
        rom.fill(base, 4096, 1);
        if size != 0x21 {
            rom.fill(base + 0x0800, 32, 3);
        }
        rom.fill(base + 0x1000, if size == 0x20 { 1024 } else { 32 }, 2);
    }
    rom.command(0, 5);
    for _ in 0..2 {
        rom.write(0xc00000, if interlaced { 1024 } else { 512 });
    }
    rom.register(1, 0x44);
    rom.write(0xff0000, 0x1234);
    rom.word(0x60fe);
    rom.bytes
}

fn advance_field(machine: &mut Emulator, batched: bool) {
    if batched {
        machine.step_field().unwrap();
    } else {
        let start = machine.pacing_field_count();
        for _ in 0..100_000 {
            machine.step_instruction().unwrap();
            if machine.pacing_field_count() != start {
                assert_eq!(machine.video_error(), None);
                return;
            }
        }
        panic!("field did not complete");
    }
    assert_eq!(machine.video_error(), None);
}

fn ready(machine: &mut Emulator, batched: bool) {
    for _ in 0..100_000 {
        machine.step_instruction().unwrap();
        if machine.peek(0xff0000) == Some(0x12) && machine.peek(0xff0001) == Some(0x34) {
            for _ in 0..4 {
                advance_field(machine, batched);
            }
            return;
        }
    }
    panic!("guest did not finish video initialization");
}

fn picture(machine: &Emulator, size: u8, h40: bool, interlaced: bool) {
    let (width, height) = (
        if h40 { 320 } else { 256 },
        if interlaced { 448 } else { 224 },
    );
    assert_eq!(machine.framebuffer_dimensions(), (width, height));
    for y in 0..height {
        let expected = match (size == 0x20, y % 2) {
            (false, 0) => [255, 0, 0, 255],
            (false, _) => [0, 0, 255, 255],
            (true, 0) => [0, 255, 0, 255],
            (true, _) => [255, 255, 255, 255],
        };
        for x in 0..width {
            let offset = (y * width + x) * 4;
            assert_eq!(
                &machine.framebuffer()[offset..offset + 4],
                &expected,
                "size {size:02x}, ({x},{y})"
            );
        }
    }
}

pub fn scheduled_plane_layout_guests() {
    for size in [0x21, 0x20, 0x22] {
        for h40 in [false, true] {
            for interlaced in [false, true] {
                for concurrent_sound in [false, true] {
                    for batched in [false, true] {
                        let config = LoadConfig {
                            region: Region::NtscUs,
                            concurrent_sound,
                        };
                        let bytes = rom(size, h40, interlaced);
                        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
                        ready(&mut machine, batched);
                        assert_eq!(machine.vdp().registers().registers()[16], Some(size));
                        picture(&machine, size, h40, interlaced);
                        for _ in 0..2000 {
                            machine.step_instruction().unwrap();
                        }
                        let state = machine.encode_state().unwrap();
                        assert_eq!(&state[..8], b"ZMDRAW01");
                        assert_eq!(StateReader::new(&state[8..]).read_u32().unwrap(), 1);
                        let mut restored = Emulator::new(bytes, config).unwrap();
                        restored.decode_state(&state).unwrap();
                        machine.decode_state(&state).unwrap();
                        assert!(machine == restored);
                        for _ in 0..3 {
                            advance_field(&mut machine, batched);
                            advance_field(&mut restored, batched);
                            assert!(machine == restored);
                            assert_eq!(machine.framebuffer(), restored.framebuffer());
                        }
                        picture(&machine, size, h40, interlaced);
                        assert_eq!(
                            machine.encode_state().unwrap(),
                            restored.encode_state().unwrap()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn original_guest_publishes_r16_sparse_and_single_row_video_and_restores() {
    scheduled_plane_layout_guests();
}
