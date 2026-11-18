use super::*;
mod target;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use zeff_md_core::TimingModel;

fn normal_machine(region: Region, timing: TimingModel) -> Emulator {
    let mut sound = vec![
        0xf3, 0x31, 0xfe, 0x1f, 0x01, 0x34, 0x12, 0x11, 0x78, 0x56, 0x21, 0xbc, 0x9a, 0xaf, 0x3e,
        0x5a, 0xed, 0x47, 0xed, 0x5e,
    ];
    sound.extend([0xdd; 16]);
    sound.extend([0x21, 0x57, 0x13, 0xfd, 0x21, 0x68, 0x24, 0x76]);
    assert_eq!(sound.len(), 44);
    let mut words = vec![0x33fc, 0x8104, 0x00c0, 4];
    let write = |words: &mut Vec<u16>, address: u32, value: u8| {
        words.extend([
            0x13fc,
            u16::from(value),
            (address >> 16) as u16,
            address as u16,
        ]);
    };
    for value in [0x85, 0x12, 0x90] {
        write(&mut words, 0xc00011, value);
    }
    write(&mut words, 0xff0040, 0);
    write(&mut words, 0xa11100, 1);
    write(&mut words, 0xa11200, 1);
    words.extend([0x0839, 0, 0x00a1, 0x1100, 0x66f6]);
    for (offset, value) in sound.into_iter().enumerate() {
        write(&mut words, 0xa00000 + offset as u32, value);
    }
    write(&mut words, 0xa11200, 0);
    words.extend([0x0839, 0, 0x00a1, 0x1100, 0x67f6]);
    write(&mut words, 0xa11200, 1);
    write(&mut words, 0xa11100, 0);
    words.extend([0x5239, 0x00ff, 0x0040, 0x60f8]);
    let mut rom = vec![0; 0x2000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = std::path::Path::new("sound-register-view.md");
    let loaded = load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            apply_mods: false,
            md_region: Some(region),
            md_timing: timing,
            md_load_save_ram: false,
            md_host_persistence: false,
            sample_rate: Some(48_000),
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    let EmuBackend::Md(md) = loaded else {
        panic!("expected normal Mega Drive loader");
    };
    md.core().clone()
}

fn pcm(emu: &Emulator) -> Vec<u32> {
    emu.clone()
        .drain_audio_samples()
        .iter()
        .map(|sample| sample.to_bits())
        .collect()
}

fn state(emu: &Emulator) -> [Vec<u8>; 3] {
    [
        emu.encode_state().unwrap(),
        emu.encode_frontend_state().unwrap(),
        emu.encode_debugger_checkpoint().unwrap(),
    ]
}

fn section(cpu: &CpuDebugSnapshot) -> &DebugSection {
    cpu.sections
        .iter()
        .find(|section| section.heading == "SOUND Z80 (read-only published state)")
        .unwrap()
}

fn inspect_without_progress(emu: &Emulator, timing: TimingModel) {
    let mut control = emu.clone();
    let encoded = state(emu);
    let old_pcm = pcm(emu);
    let tick = emu.timing_snapshot();
    let main = emu.registers();
    let queue = emu.queued_words();
    let sound = emu.sound_cpu().clone();
    let mut req = request();
    req.show_memory_viewer = false;
    assert!(collect_md_snapshot(emu, &req, None).cpu_debug.is_none());
    req.want_debug_info = true;
    for _ in 0..8 {
        let cpu = collect_md_snapshot(emu, &req, None).cpu_debug.unwrap();
        assert_no_execution_data(&cpu);
        assert_eq!(cpu.pc, main.pc);
        assert_eq!(cpu.cycles, emu.cpu_clock().0);
        assert_eq!(cpu.register_lines, snapshot(&control).register_lines);
        let view = section(&cpu);
        let registers = sound.regs();
        assert_eq!(
            view.lines[2],
            format!(
                "I:{:02X}  R:{:02X}  Cycles:{}",
                registers.i,
                registers.r,
                sound.cycles()
            )
        );
        assert_eq!(
            view.lines[4],
            format!(
                "Prefix pending:{}  Instruction pending:{}",
                u8::from(emu.sound_prefix_pending()),
                u8::from(emu.sound_instruction_pending())
            )
        );
        let deadline = emu.sound_deadline().map_or_else(
            || "Unknown".into(),
            |deadline| format!("{} MCLK", deadline.get()),
        );
        assert_eq!(view.lines[5], format!("Segment deadline:{deadline}"));
        if timing == TimingModel::Precise {
            assert_eq!(emu.sound_deadline(), None);
            assert_eq!(view.lines[5], "Segment deadline:Unknown");
        }
        assert_eq!(emu.timing_snapshot(), tick);
        assert_eq!(emu.registers(), main);
        assert_eq!(emu.queued_words(), queue);
        assert_eq!(emu.sound_cpu(), &sound);
        assert_eq!(emu, &control);
        assert_eq!(pcm(emu), old_pcm);
        assert_eq!(state(emu), encoded);
    }
    let mut inspected = emu.clone();
    for _ in 0..64 {
        assert_eq!(
            inspected.step_instruction().unwrap(),
            control.step_instruction().unwrap()
        );
    }
    let fields = inspected.frame_count();
    for _ in 0..2 {
        inspected.step_field().unwrap();
        control.step_field().unwrap();
        assert_eq!(inspected, control);
        assert_eq!(inspected.sound_cpu(), control.sound_cpu());
        assert_eq!(inspected.queued_words(), control.queued_words());
        assert_eq!(state(&inspected), state(&control));
        assert_eq!(inspected.framebuffer(), control.framebuffer());
    }
    assert!(inspected.frame_count() > fields);
    let actual = pcm(&inspected);
    assert!(actual.iter().any(|sample| f32::from_bits(*sample) != 0.0));
    assert_eq!(actual, pcm(&control));
    assert_eq!(state(emu), encoded);
    assert_eq!(emu.timing_snapshot(), tick);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_published_register_view_uses_normal_concurrent_loader_and_preserves_prefix_future() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut emu = normal_machine(region, timing);
            let mut prefix = false;
            let mut deadline = false;
            let mut halted = false;
            for _ in 0..20_000 {
                emu.step_instruction().unwrap();
                let registers = emu.sound_cpu().regs();
                if !prefix && emu.sound_prefix_pending() && (21..=36).contains(&registers.pc) {
                    assert!(emu.sound_instruction_pending());
                    assert_eq!(registers.ix, 0);
                    assert_eq!(registers.iy, 0);
                    let cpu = snapshot(&emu);
                    assert_eq!(section(&cpu).lines[0], "AF:5A44  BC:1234  DE:5678  HL:9ABC");
                    assert_eq!(
                        section(&cpu).lines[1],
                        format!("IX:0000  IY:0000  SP:1FFE  PC:{:04X}", registers.pc)
                    );
                    inspect_without_progress(&emu, timing);
                    prefix = true;
                }
                if !deadline
                    && timing == TimingModel::Standard
                    && prefix
                    && emu.sound_deadline().is_some()
                {
                    assert!(
                        emu.sound_deadline().unwrap().get() > emu.timing_snapshot().now().get()
                    );
                    inspect_without_progress(&emu, timing);
                    deadline = true;
                }
                if emu.sound_cpu().is_halted() {
                    halted = true;
                    break;
                }
            }
            assert!(prefix, "no real prefix publication {region:?}/{timing:?}");
            assert!(halted, "no real SOUND HALT {region:?}/{timing:?}");
            assert!(timing == TimingModel::Precise || deadline);
            assert!(!emu.sound_reset_asserted());
            assert!(!emu.sound_bus_requested());
            assert!(!emu.sound_bus_granted());
            let cpu = snapshot(&emu);
            let view = section(&cpu);
            assert_eq!(view.lines[0], "AF:5A44  BC:1234  DE:5678  HL:9ABC");
            assert_eq!(view.lines[1], "IX:1357  IY:2468  SP:1FFE  PC:002C");
            assert_eq!(emu.sound_cpu().regs().i, 0x5a);
            assert_eq!(
                view.lines[3],
                "Published state:Halted  IM:Im2  IFF1:0  IFF2:0  EI pending:0"
            );
            assert_eq!(cpu.pc, emu.registers().pc);
            assert!(cpu.pc > 0x200);
            inspect_without_progress(&emu, timing);
        }
    }
}
