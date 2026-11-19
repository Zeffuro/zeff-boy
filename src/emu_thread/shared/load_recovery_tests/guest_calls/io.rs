use super::*;
use crate::debug::CpuDebugSnapshot;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_md_core::{ControllerKind, ControllerPort, Emulator, TimingModel};

const LOOP: u32 = 0x268;

fn fixture(header: u8, timing: TimingModel, pad: ControllerKind, high: bool) -> EmuBackend {
    let mut words = Vec::new();
    for (address, value) in [
        (0xc00011_u32, 0x80),
        (0xc00011, 0x10),
        (0xc00011, 0x90),
        (0xa10009, 0x40),
        (0xa10003, if high { 0x40 } else { 0 }),
        (0xa1000b, 1),
        (0xa10005, 0),
        (0xa1000d, 0x7f),
        (0xa10007, 0x55),
    ] {
        words.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
    }
    for value in [
        if high { 0x8104 } else { 0x8134 },
        if high { 0x8715 } else { 0x872a },
        if high { 0x8f02 } else { 0x8f04 },
    ] {
        words.extend([0x33fc, value, 0x00c0, 4]);
    }
    words.extend([0x33fc, if high { 0 } else { 0x100 }, 0x00a1, 0x1100]);
    assert_eq!(0x200 + words.len() as u32 * 2, LOOP);
    words.extend([0x60fe, 0x4e71]);
    guest_call_backend_with(header, timing, pad, &[(0x200, &words)], false)
}

fn machine(backend: &EmuBackend) -> &Emulator {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.core()
}

fn machine_mut(backend: &mut EmuBackend) -> &mut Emulator {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.test_normal_core_mut().unwrap()
}

fn overlay() -> Vec<CheatPatch> {
    vec![CheatPatch::WideRomWrite {
        address: 0x1800,
        value: CheatValue::Constant(0xa5),
    }]
}

fn input(frames: usize, inspect: bool) -> FrameInput {
    let mut request = guest_call_input(frames);
    request.snapshot.want_debug_info = inspect;
    request.snapshot.memory_view_start = 0x1800;
    request.joypad.buttons = 0x40;
    request.joypad.dpad = 4;
    request.joypad.dpad_p2 = 8;
    request
}

fn assert_panel(cpu: &CpuDebugSnapshot, version: u32, high: bool, irq: u8) {
    assert_eq!(
        cpu.io_registers
            .iter()
            .map(|row| (
                row.name,
                row.address,
                row.value,
                row.width,
                row.writable_mask
            ))
            .collect::<Vec<_>>(),
        [
            ("VERSION", 0xa10001, version, 1, 0),
            ("DATA1", 0xa10003, if high { 0x7e } else { 0x22 }, 1, 0),
            ("DATA2", 0xa10005, 0x7c, 1, 0),
            ("DATA EXT", 0xa10007, 0x55, 1, 0),
            ("CTRL1", 0xa10009, 0x40, 1, 0x7f),
            ("CTRL2", 0xa1000b, 1, 1, 0x7f),
            ("CTRL EXT", 0xa1000d, 0x7f, 1, 0x7f),
        ]
    );
    assert_eq!(
        cpu.io_registers[0]
            .bits
            .iter()
            .map(|bit| (bit.mask, bit.label))
            .collect::<Vec<_>>(),
        [(0x80, "Overseas"), (0x40, "PAL")]
    );
    assert_eq!(
        (
            cpu.io_registers[1].bits[6].mask,
            cpu.io_registers[1].bits[6].label
        ),
        (0x40, "TH")
    );
    assert_eq!(
        (
            cpu.io_registers[4].bits[6].mask,
            cpu.io_registers[4].bits[6].label
        ),
        (0x40, "TH out")
    );
    let vdp = cpu
        .sections
        .iter()
        .find(|section| section.heading == "VDP registers")
        .unwrap();
    assert_eq!(
        vdp.lines[0],
        if high {
            "Display enable:0  VBlank IRQ enable:0  DMA enable:0  Mode 5:1"
        } else {
            "Display enable:0  VBlank IRQ enable:1  DMA enable:1  Mode 5:1"
        }
    );
    assert!(
        vdp.lines
            .iter()
            .any(|line| line.ends_with(if high { "R07:15" } else { "R07:2A" }))
    );
    assert!(
        vdp.lines
            .iter()
            .any(|line| line.ends_with(if high { "R15:02" } else { "R15:04" }))
    );
    let bus = cpu
        .sections
        .iter()
        .find(|section| section.heading == "Bus and interrupts")
        .unwrap();
    assert_eq!(bus.lines[0], format!("MAIN IRQ:{irq}"));
    assert_eq!(
        bus.lines[1],
        if high {
            "SOUND request:0  granted:0  reset:1"
        } else {
            "SOUND request:1  granted:0  reset:1"
        }
    );
    assert_eq!(
        bus.lines[2],
        "SOUND bank:000  Prefix pending:0  Instruction pending:0"
    );
    assert_eq!(bus.lines[3], "SOUND deadline:--");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_io_backend_snapshots_decode_real_mmio_and_preserve_frontend_owned_state() {
    for (header, version) in [(b'U', 0xa0), (b'J', 0x20), (b'E', 0xe0)] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            for pad in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
                for high in [false, true] {
                    let mut subject = fixture(header, timing, pad, high);
                    let mut control = fixture(header, timing, pad, high);
                    for backend in [&mut subject, &mut control] {
                        let emu = machine_mut(backend);
                        emu.input_mut()
                            .set_controller_raw(ControllerPort::One, 0xbe);
                        emu.input_mut()
                            .set_controller_raw(ControllerPort::Two, 0xfd);
                        emu.install_rom_patches(&overlay()).unwrap();
                        for _ in 0..21 {
                            emu.step_instruction().unwrap();
                        }
                        assert_eq!(emu.registers().pc, LOOP);
                    }
                    let emu = machine(&subject);
                    let raw = emu.encode_state().unwrap();
                    let frontend = emu.encode_frontend_state().unwrap();
                    let owned = emu.encode_debugger_checkpoint().unwrap();
                    let queue = emu.queued_words();
                    assert_eq!(emu.rom_bytes()[0x1800], 0);
                    assert_eq!(emu.peek(0x1800), Some(0xa5));
                    let request = input(0, true);
                    for _ in 0..8 {
                        let view = crate::ui::collect_md_snapshot(emu, &request.snapshot, None);
                        assert_panel(view.cpu_debug.as_ref().unwrap(), version, high, 0);
                        assert_eq!(emu.encode_state().unwrap(), raw);
                        assert_eq!(emu.encode_frontend_state().unwrap(), frontend);
                        assert_eq!(emu.encode_debugger_checkpoint().unwrap(), owned);
                        assert_eq!(emu.queued_words(), queue);
                        assert_eq!(emu.peek(0x1800), Some(0xa5));
                    }
                    let emu = machine_mut(&mut subject);
                    emu.input_mut()
                        .set_controller_raw(ControllerPort::One, 0xff);
                    let released = crate::ui::collect_md_snapshot(emu, &request.snapshot, None);
                    assert_eq!(
                        released.cpu_debug.unwrap().io_registers[1].value,
                        if high { 0x7f } else { 0x33 }
                    );
                    assert_ne!(emu.encode_state().unwrap(), raw);
                    emu.decode_debugger_checkpoint(&owned).unwrap();
                    let restored = crate::ui::collect_md_snapshot(emu, &request.snapshot, None);
                    assert_panel(restored.cpu_debug.as_ref().unwrap(), version, high, 0);
                    assert_eq!(emu.encode_frontend_state().unwrap(), frontend);
                    assert_eq!(emu.encode_debugger_checkpoint().unwrap(), owned);
                    assert_eq!(emu.peek(0x1800), Some(0xa5));
                    for _ in 0..128 {
                        assert_eq!(
                            machine_mut(&mut subject).step_instruction().unwrap(),
                            machine_mut(&mut control).step_instruction().unwrap()
                        );
                    }
                    let actual = machine_mut(&mut subject).drain_audio_samples();
                    let expected = machine_mut(&mut control).drain_audio_samples();
                    assert!(actual.iter().any(|sample| *sample != 0.0));
                    assert!(
                        actual
                            .iter()
                            .map(|sample| sample.to_bits())
                            .eq(expected.iter().map(|sample| sample.to_bits()))
                    );
                    assert_eq!(
                        machine(&subject).framebuffer(),
                        machine(&control).framebuffer()
                    );
                    assert_eq!(
                        machine(&subject).encode_state().unwrap(),
                        machine(&control).encode_state().unwrap()
                    );
                    assert_eq!(
                        machine(&subject).encode_frontend_state().unwrap(),
                        machine(&control).encode_frontend_state().unwrap()
                    );
                    assert_eq!(
                        machine(&subject).encode_debugger_checkpoint().unwrap(),
                        machine(&control).encode_debugger_checkpoint().unwrap()
                    );
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_io_actual_workers_inspect_running_and_paused_without_changing_future_output() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for high in [false, true] {
            let mut subject = EmuThread::spawn(
                fixture(b'U', timing, ControllerKind::ThreeButton, high),
                false,
            );
            let mut control = EmuThread::spawn(
                fixture(b'U', timing, ControllerKind::ThreeButton, high),
                false,
            );
            for thread in [&subject, &control] {
                thread.send(EmuCommand::UpdateCheats(overlay()));
            }
            let actual = guest_call_deliver(&subject, input(2, true));
            let expected = guest_call_deliver(&control, input(2, false));
            assert_eq!((actual.advanced_frames, expected.advanced_frames), (2, 2));
            assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
            assert_panel(
                actual.ui_data.cpu_debug.as_ref().unwrap(),
                0xa0,
                high,
                if high { 0 } else { 6 },
            );
            assert_eq!(
                actual.ui_data.cpu_debug.as_ref().unwrap().cpu_state,
                "Running"
            );
            assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm(&actual), pcm(&expected));
            assert_eq!(capture(&subject), capture(&control));
            for (thread, inspect) in [(&subject, true), (&control, false)] {
                let mut stop = input(1, inspect);
                stop.debug_actions.add_breakpoint = Some(LOOP);
                paused(&guest_call_deliver(thread, stop));
            }
            let raw = capture(&subject);
            assert_eq!(raw, capture(&control));
            for _ in 0..8 {
                let inspected = guest_call_deliver(&subject, input(0, true));
                paused(&inspected);
                assert!(inspected.audio_samples.is_empty());
                let cpu = inspected.ui_data.cpu_debug.as_ref().unwrap();
                assert_eq!(cpu.cpu_state, "Suspended");
                assert_panel(cpu, 0xa0, high, if high { 0 } else { 6 });
                assert_eq!(cpu.recent_opcodes[0].storage_offset, Some(u64::from(LOOP)));
                assert!(
                    inspected
                        .ui_data
                        .memory_page
                        .unwrap()
                        .contains(&(0x1800, 0xa5))
                );
                assert_eq!(capture(&subject), raw);
            }
            for round in 0..2 {
                let request = |inspect| {
                    let mut run = input(1, inspect);
                    if round == 0 {
                        run.debug_actions.remove_breakpoints.push(LOOP);
                        run.debug_continue = true;
                    }
                    run
                };
                let actual = guest_call_deliver(&subject, request(true));
                let expected = guest_call_deliver(&control, request(false));
                assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
                assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
                assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(pcm(&actual), pcm(&expected));
                assert_eq!(capture(&subject), capture(&control));
                let actual = subject.shared_framebuffer().load_full().unwrap();
                let expected = control.shared_framebuffer().load_full().unwrap();
                assert!(!actual.is_empty());
                assert_eq!(actual.dimensions(), expected.dimensions());
                assert!(actual.as_slice() == expected.as_slice());
            }
            subject.shutdown();
            control.shutdown();
        }
    }
}

mod input_edits;
