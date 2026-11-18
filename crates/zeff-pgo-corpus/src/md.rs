pub(super) fn rom() -> Vec<u8> {
    let mut rom = vec![0; 32 * 1024];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x120..0x12c].copy_from_slice(b"PGO VDP TONE");
    rom[0x1a4..0x1a8].copy_from_slice(&0x7fff_u32.to_be_bytes());
    rom[0x1f0] = b'U';
    let mut code = vec![0x46fc, 0x2700];
    for register in [
        0x8004, 0x8104, 0x8230, 0x8300, 0x8407, 0x8578, 0x8700, 0x8b00, 0x8c00, 0x8d3f, 0x8f02,
        0x9000, 0x9100, 0x9200,
    ] {
        word(&mut code, 0xc00004, register);
    }
    for command in [0xc000, 0x0000] {
        word(&mut code, 0xc00004, command);
    }
    for color in [0, 0x0eee] {
        word(&mut code, 0xc00000, color);
    }
    for command in [0x4020, 0x0000] {
        word(&mut code, 0xc00004, command);
    }
    for _ in 0..16 {
        word(&mut code, 0xc00000, 0x1010);
    }
    for command in [0x4000, 0x0003] {
        word(&mut code, 0xc00004, command);
    }
    code.extend([0x303c, 1023]);
    word(&mut code, 0xc00000, 1);
    code.extend([0x51c8, 0xfff6]);
    word(&mut code, 0xc00004, 0x8144);
    for (address, value) in [
        (0xa11100, 1),
        (0xa11200, 0),
        (0xa11200, 1),
        (0xa04002, 0xb6),
        (0xa04003, 0xc0),
        (0xa04000, 0x2b),
        (0xa04001, 0x80),
        (0xa04000, 0x2a),
        (0xc00011, 0x85),
        (0xc00011, 0x06),
        (0xc00011, 0x90),
    ] {
        code.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
    }
    // The running counter drives RAM and DAC writes after video initialization.
    code.extend([
        0x7000, 0x5280, 0x23c0, 0x00ff, 0x0000, 0x13c0, 0x00a0, 0x4001, 0x60f0, 0x4e71,
    ]);
    for (index, instruction) in code.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&instruction.to_be_bytes());
    }
    let checksum = rom[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, bytes| {
            sum.wrapping_add(u16::from_be_bytes(*bytes))
        });
    rom[0x18e..0x190].copy_from_slice(&checksum.to_be_bytes());
    rom
}

fn word(code: &mut Vec<u16>, address: u32, value: u16) {
    code.extend([0x33fc, value, (address >> 16) as u16, address as u16]);
}

#[cfg(test)]
mod tests {
    use zeff_md_core::{Emulator, LoadConfig, Region};

    #[test]
    fn md_guest_runs_cpu_video_and_sustained_audio_with_exact_continuation() {
        let fixture = crate::fixtures()
            .into_iter()
            .find(|fixture| fixture.system == "MD")
            .unwrap();
        let config = LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        };
        let mut emu = Emulator::new(fixture.bytes.clone(), config).unwrap();
        emu.set_apu_sample_generation_enabled(true);
        emu.set_sample_rate(48_000);
        for _ in 0..90 {
            emu.step_field().unwrap();
            emu.drain_audio_samples();
        }
        let clock = emu.cpu_clock();
        let counter = emu.registers().d[0];
        assert!(counter > 1024);
        let published = u32::from_be_bytes(emu.work_ram()[..4].try_into().unwrap());
        assert!(
            counter - published <= 1,
            "guest counter was not published to RAM"
        );
        let (width, height) = emu.framebuffer_dimensions();
        let pixels = emu.framebuffer()[..width * height * 4].as_chunks::<4>().0;
        assert!(pixels.iter().any(|pixel| pixel != &pixels[0]));
        assert!(emu.video_error().is_none());
        let checkpoint = emu.encode_frontend_state().unwrap();
        let mut restored = Emulator::new(fixture.bytes, config).unwrap();
        restored.set_apu_sample_generation_enabled(true);
        restored.set_sample_rate(48_000);
        restored.decode_frontend_state(&checkpoint).unwrap();
        for _ in 0..30 {
            emu.step_field().unwrap();
            restored.step_field().unwrap();
            assert_eq!(emu.framebuffer(), restored.framebuffer());
            let audio = emu.drain_audio_samples();
            assert!(audio.len() > 1000 && audio.iter().any(|sample| *sample != 0.0));
            assert_eq!(
                audio.into_iter().map(f32::to_bits).collect::<Vec<_>>(),
                restored
                    .drain_audio_samples()
                    .into_iter()
                    .map(f32::to_bits)
                    .collect::<Vec<_>>()
            );
        }
        assert!(emu.cpu_clock() > clock && emu.registers().d[0] > counter);
        assert_eq!(
            emu.encode_state().unwrap(),
            restored.encode_state().unwrap()
        );
    }
}
