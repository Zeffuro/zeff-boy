use super::super::super::collect_md_snapshot;
use super::super::memory_tests::request;
use crate::debug::ApuDebugInfo;
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{Emulator, LoadConfig, Region};

fn fixture(region: Region) -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    let mut writes = vec![(0xa11100, 1), (0xa11200, 1)];
    for value in [
        0x85, 0x12, 0x92, 0xa6, 0x23, 0xb5, 0xc7, 0x34, 0xd8, 0xe7, 0xf3,
    ] {
        writes.push((0xc00011, value));
    }
    for (address, value) in [
        (0x24, 0x56),
        (0x25, 2),
        (0x26, 0x78),
        (0x27, 5),
        (0x2b, 0x80),
        (0x2a, 0x40),
    ] {
        writes.extend([(0xa04000, address), (0xa04001, value)]);
    }
    let vdp_registers = [0x8104, 0x8c00, 0x8700];
    let initial_writes = writes.len() + vdp_registers.len();
    writes.extend([
        (0xa04001, 0xa0),
        (0xc00011, 0x95),
        (0xa04001, 0x40),
        (0xc00011, 0x92),
    ]);
    let mut words = Vec::new();
    for value in vdp_registers {
        words.extend([0x33fc_u16, value, 0x00c0, 4]);
    }
    for (address, value) in writes {
        words.extend([0x13fc_u16, value, (address >> 16) as u16, address as u16]);
    }
    words.extend([0x60de, 0x4e71]);
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut emu = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap();
    for _ in 0..initial_writes {
        emu.step_instruction().unwrap();
    }
    emu
}

fn capture(emu: &Emulator, show: bool) -> Option<ApuDebugInfo> {
    let mut req = request();
    req.show_memory_viewer = false;
    req.show_apu_viewer = show;
    collect_md_snapshot(emu, &req, None).apu_debug
}

fn section<'a>(data: &'a ApuDebugInfo, heading: &str) -> &'a [String] {
    &data
        .extra_sections
        .iter()
        .find(|section| section.heading == heading)
        .unwrap()
        .lines
}

fn assert_exposed_fields(emu: &Emulator, data: &ApuDebugInfo) {
    let psg = emu.psg_snapshot();
    let fm = emu.opn2_snapshot();
    assert_eq!(
        data.master_lines[0],
        format!("sample_rate={}", emu.sample_rate())
    );
    assert!(data.master_lines[1].contains("Read-only"));
    assert!(data.master_lines[1].contains("history"));
    assert!(data.channels.is_empty() && data.master_waveform.is_empty());
    assert_eq!(data.extra_sections.len(), 2);
    assert_eq!(
        section(data, "PSG"),
        [
            format!(
                "periods={:03X?} attenuation={:X?}",
                psg.periods, psg.volumes
            ),
            format!("counters={:03X?} outputs={:?}", psg.counters, psg.outputs),
            format!(
                "latched_register={:X} noise_control={:X}",
                psg.latched_register, psg.noise_control
            ),
            format!(
                "noise_counter={:04X} noise_clock={} noise_lfsr={:04X}",
                psg.noise_counter, psg.noise_clock, psg.noise_lfsr
            ),
        ]
    );
    assert_eq!(
        section(data, "OPN2"),
        [
            format!(
                "address={:03X} status={:02X} mode={:02X}",
                fm.address, fm.status, fm.mode
            ),
            format!("timer_a={:03X} timer_b={:02X}", fm.timer_a, fm.timer_b),
            format!("dac={:02X} dac_enabled={}", fm.dac, fm.dac_enabled),
            format!("instantaneous_outputs={:?}", fm.output),
        ]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_audio_snapshot_obeys_request_and_exposes_current_chip_fields() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut emu = fixture(region);
        let original = emu.encode_state().unwrap();
        assert!(capture(&emu, false).is_none());
        let initial = capture(&emu, true).unwrap();
        assert_exposed_fields(&emu, &initial);
        assert_eq!(
            section(&initial, "PSG")[0],
            "periods=[125, 236, 347] attenuation=[2, 5, 8, 3]"
        );
        assert_eq!(
            section(&initial, "PSG")[2],
            "latched_register=7 noise_control=7"
        );
        assert_eq!(section(&initial, "OPN2")[1], "timer_a=15A timer_b=78");
        assert_eq!(section(&initial, "OPN2")[2], "dac=40 dac_enabled=true");
        for _ in 0..2 {
            emu.step_instruction().unwrap();
        }
        let changed = capture(&emu, true).unwrap();
        assert_exposed_fields(&emu, &changed);
        assert_eq!(section(&changed, "OPN2")[2], "dac=A0 dac_enabled=true");
        assert!(section(&changed, "PSG")[0].ends_with("attenuation=[5, 5, 8, 3]"));
        assert_eq!(section(&initial, "OPN2")[2], "dac=40 dac_enabled=true");
        emu.decode_state(&original).unwrap();
        let restored = capture(&emu, true).unwrap();
        assert_eq!(restored.master_lines, initial.master_lines);
        assert_eq!(section(&restored, "PSG"), section(&initial, "PSG"));
        assert_eq!(section(&restored, "OPN2"), section(&initial, "OPN2"));
        assert!(capture(&emu, false).is_none());
        assert_eq!(emu.encode_state().unwrap(), original);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_audio_inspection_preserves_whole_state_and_future_pcm_frames_after_restore() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut emu = fixture(region);
        let saved = emu.encode_state().unwrap();
        for restored in [false, true] {
            if restored {
                emu.decode_state(&saved).unwrap();
            }
            let mut control = emu.clone();
            let frame = emu.frame_count();
            let end = emu.timing_snapshot().now().get() + region.frame_ticks() * 2;
            while emu.timing_snapshot().now().get() < end {
                for show in [true, false, true] {
                    let before = emu.encode_state().unwrap();
                    if let Some(data) = capture(&emu, show) {
                        assert_exposed_fields(&emu, &data);
                    } else {
                        assert!(!show);
                    }
                    assert_eq!(emu.encode_state().unwrap(), before);
                    assert_eq!(emu, control);
                }
                for _ in 0..256 {
                    assert_eq!(
                        emu.step_instruction().unwrap(),
                        control.step_instruction().unwrap()
                    );
                }
            }
            assert!(emu.frame_count() > frame);
            assert_eq!(emu.video_error(), None);
            assert_eq!(emu.framebuffer_dimensions(), (256, 224));
            assert!(
                emu.framebuffer()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == [0, 0, 0, 255])
            );
            assert_eq!(emu, control);
            assert_eq!(emu.framebuffer(), control.framebuffer());
            let pcm = emu.drain_audio_samples();
            assert!(pcm.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm, control.drain_audio_samples());
            assert_eq!(emu.encode_state().unwrap(), control.encode_state().unwrap());
        }
    }
}
