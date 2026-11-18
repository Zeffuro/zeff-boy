use super::*;

fn normal_rom(h40: bool, v30: bool) -> Vec<u8> {
    let mut rom = Rom {
        bytes: vec![0; 0x4000],
        pc: 0x200,
    };
    rom.bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, value) in [
        (1, 4),
        (15, 2),
        (12, if h40 { 0x8b } else { 0x0a }),
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
    rom.command(32, 1);
    for row in 0..8 {
        let value = if row % 2 == 0 { 0x1111 } else { 0x2222 };
        rom.write(0xc00000, value);
        rom.write(0xc00000, value);
    }
    for _ in 0..16 {
        rom.write(0xc00000, 0x3333);
    }
    rom.command(0x8000, 1);
    rom.word(0x303c);
    rom.word(2047);
    let fill = rom.pc;
    rom.write(0xc00000, 0x8001);
    let branch = rom.pc;
    rom.word(0x51c8);
    rom.word((fill as i32 - (branch + 2) as i32) as i16 as u16);
    rom.command(0xc000, 1);
    for word in [129, 0, 0x8002, 136] {
        rom.write(0xc00000, word);
    }
    rom.command(0, 5);
    rom.write(0xc00000, 0x401);
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

fn normal_machine(region: Region, h40: bool, v30: bool) -> Emulator {
    Emulator::new(
        normal_rom(h40, v30),
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap()
}

fn normal_picture(machine: &Emulator, width: usize, height: usize) {
    assert_eq!(machine.framebuffer_dimensions(), (width, height));
    for y in 0..height {
        let color = if (y / 2) % 2 == 0 {
            &[0, 0, 255, 255]
        } else {
            &[255, 0, 0, 255]
        };
        assert_eq!(pixel(machine, 0, y), color, "row {y}");
        assert_eq!(pixel(machine, width - 1, y), color, "last pixel row {y}");
        if (2..18).contains(&y) {
            assert_eq!(pixel(machine, 8, y), &[0, 255, 0, 255]);
        }
    }
}

pub fn normal_interlace_witness() -> u64 {
    normal_counter_witness();
    normal_transition_witness();
    let mut digest = 0xcbf29ce484222325u64;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for v30 in [false, true] {
                if v30 && region != Region::Pal {
                    continue;
                }
                let mut actual = normal_machine(region, h40, v30);
                ready(&mut actual);
                let (width, height) = (if h40 { 320 } else { 256 }, if v30 { 480 } else { 448 });
                normal_picture(&actual, width, height);
                let frontend = actual.encode_frontend_state().unwrap();
                let mut cold = normal_machine(region, h40, v30);
                cold.decode_frontend_state(&frontend).unwrap();
                assert_eq!(cold.encode_frontend_state().unwrap(), frontend);
                field(&mut actual);
                field(&mut cold);
                assert_eq!(actual.framebuffer(), cold.framebuffer());
                assert_eq!(actual.drain_audio_samples(), cold.drain_audio_samples());
                actual.decode_frontend_state(&frontend).unwrap();
                cold.decode_frontend_state(&frontend).unwrap();
                field(&mut actual);
                field(&mut cold);
                assert_eq!(
                    actual.encode_frontend_state().unwrap(),
                    cold.encode_frontend_state().unwrap()
                );
                field(&mut actual);
                normal_picture(&actual, width, height);
                for _ in 0..57 {
                    actual.step_instruction().unwrap();
                }
                let state = actual.encode_state().unwrap();
                let mut restored = normal_machine(region, h40, v30);
                restored.decode_state(&state).unwrap();
                actual.decode_state(&state).unwrap();
                assert!(actual == restored);
                field(&mut actual);
                field(&mut restored);
                assert!(actual == restored);
                normal_picture(&actual, width, height);
                assert_eq!(
                    actual.encode_state().unwrap(),
                    restored.encode_state().unwrap()
                );
                assert_eq!(actual.drain_audio_samples(), restored.drain_audio_samples());
                for byte in actual.framebuffer() {
                    digest = (digest ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
                }
                let before = actual.clone();
                let mut unsupported = state;
                unsupported[8..12].copy_from_slice(&0u32.to_le_bytes());
                assert!(actual.decode_state(&unsupported).is_err());
                assert!(actual == before);
            }
        }
    }
    digest
}

fn normal_transition_witness() {
    for region in [Region::NtscUs, Region::Pal] {
        for h40 in [false, true] {
            let mut machine = normal_machine(region, h40, false);
            ready(&mut machine);
            for mode in [6, 2] {
                let start = machine.timing_snapshot().now().get() / 3420 * 3420;
                assert!(machine.timing_snapshot().now().get() - start < 200);
                let requested = (if h40 { 0x81 } else { 0 }) | mode;
                let deadline = machine.timing_snapshot().now().get() + 200;
                loop {
                    if machine.queue_vdp_register(12, requested).is_ok() {
                        break;
                    }
                    machine.step_instruction().unwrap();
                    assert!(machine.timing_snapshot().now().get() < deadline);
                }
                let pending = machine.encode_frontend_state().unwrap();
                let mut pending_copy = machine.clone();
                pending_copy.decode_frontend_state(&pending).unwrap();
                assert_eq!(pending_copy.encode_frontend_state().unwrap(), pending);
                let outgoing = start + 223 * 3420 + 2700;
                while machine.timing_snapshot().now().get() < outgoing {
                    assert_eq!(machine.step_instruction(), pending_copy.step_instruction());
                }
                assert_eq!(
                    machine.encode_frontend_state().unwrap(),
                    pending_copy.encode_frontend_state().unwrap()
                );
                let saved = machine.encode_frontend_state().unwrap();
                let mut restored = normal_machine(region, h40, false);
                restored.decode_frontend_state(&saved).unwrap();
                for _ in 0..2 {
                    field(&mut machine);
                    field(&mut restored);
                    assert_eq!(
                        machine.encode_frontend_state().unwrap(),
                        restored.encode_frontend_state().unwrap()
                    );
                }
            }
        }
    }
}

fn normal_counter_witness() {
    use zeff_emu_common::time::MasterTicks;
    use zeff_md_core::Vdp;
    for (region, tall, cases) in [
        (
            Region::NtscUs,
            false,
            [(234, 0xea), (235, 0xe5), (262, 0xff)],
        ),
        (
            Region::NtscJapan,
            false,
            [(234, 0xea), (235, 0xe5), (262, 0xff)],
        ),
        (Region::Pal, false, [(255, 0xfe), (256, 1), (258, 0xc9)]),
        (Region::Pal, true, [(255, 0xfe), (256, 1), (266, 0xd1)]),
    ] {
        for h40 in [false, true] {
            let mut vdp = Vdp::bootstrap_mode5();
            for (index, value) in [
                (1, if tall { 0x0c } else { 4 }),
                (12, if h40 { 0x8b } else { 0x0a }),
            ] {
                vdp.advance_to(MasterTicks::new(vdp.now().get() + 1), region)
                    .unwrap();
                let write = vdp.queue_register(index, value, region).unwrap();
                vdp.advance_to(write.publish_at, region).unwrap();
            }
            let clock = if h40 { 2640 } else { 2660 };
            for (line, external) in cases {
                vdp.advance_to(MasterTicks::new((line - 1) * 3420 + clock), region)
                    .unwrap();
                assert_eq!(vdp.read_hv(region).unwrap() >> 8, external);
            }
            let lines = if region == Region::Pal { 313 } else { 263 };
            let wrap = (lines - 1) * 3420 + clock;
            vdp.advance_to(MasterTicks::new(wrap - 1), region).unwrap();
            assert_eq!(vdp.read_hv(region).unwrap() >> 8, 0xff);
            vdp.advance_to(MasterTicks::new(wrap), region).unwrap();
            assert_eq!(vdp.read_hv(region).unwrap() >> 8, 0);
        }
    }
}

#[test]
fn normal_interlace_cpu_render_state_shared_witness() {
    assert_ne!(normal_interlace_witness(), 0);
}
