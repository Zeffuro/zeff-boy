use super::*;

mod admission;
mod continuation;
mod lifecycle;

const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

fn images(program: &[u16]) -> (Vec<u8>, Vec<u8>) {
    let boot = [
        0x33fc, 0x5345, 0x00a1, 0x4000, 0x33fc, 0x4741, 0x00a1, 0x4002, 0x13fc, 1, 0x00a1, 0x4101,
        0x4ef9, 1, 0x0100, 0x4e71,
    ];
    let mut rom = image(&boot);
    write_words(&mut rom, CODE as usize, program);
    let firmware = rom[..2048].to_vec();
    (rom, firmware)
}

fn machine(program: &[u16], region: Region, concurrent_sound: bool) -> Emulator {
    let (rom, firmware) = images(program);
    Emulator::new_with_tmss(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
        firmware,
    )
    .unwrap()
}

fn enter_cartridge(machine: &mut Emulator) {
    for _ in 0..4 {
        machine.step_instruction().unwrap();
    }
    let tmss = machine.bus.tmss.as_ref().unwrap();
    assert!(tmss.unlocked() && !tmss.boot_mapped());
    assert_eq!(machine.registers().pc, CODE);
    assert_eq!(
        machine.rom_offset_for_cpu_address(CODE),
        Some(CODE as usize)
    );
}

fn restore(machine: &mut Emulator, states: &[Vec<u8>; 3], codec: usize) {
    match codec {
        0 => machine.decode_state(&states[codec]).unwrap(),
        1 => machine.decode_frontend_state(&states[codec]).unwrap(),
        2 => machine.decode_debugger_checkpoint(&states[codec]).unwrap(),
        _ => unreachable!(),
    }
}

fn states(machine: &Emulator) -> [Vec<u8>; 3] {
    [
        machine.encode_state().unwrap(),
        machine.encode_frontend_state().unwrap(),
        machine.encode_debugger_checkpoint().unwrap(),
    ]
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn tmss_factory_overlay_preserves_boot_source_and_rejects_invalid_lists() {
    for region in REGIONS {
        for concurrent in [false, true] {
            admission::check(region, concurrent);
            let mut m = machine(&[0x7001, 0x0640, 3, 0x60f8, 0x4e71], region, concurrent);
            assert!(m.supports_rom_cheats());
            let before = states(&m);
            let firmware = m.bus.tmss.clone();
            let source = m.bus.cartridge.bytes.clone();
            let hash = m.bus.cartridge.rom_digest();
            let queued = m.cpu.queued_instruction_inputs();
            assert_eq!(m.rom_offset_for_cpu_address(CODE), None);
            assert_eq!(m.peek(DATA), Some(0));
            m.install_rom_patches(&patches("018000:A55A + 010100:7007"))
                .unwrap();
            assert_eq!(m.peek(DATA), Some(0));
            crate::hardware::vdp::VdpDmaSource::from_cartridge(&m.bus.cartridge, &m.bus.ram)
                .with_tmss(m.bus.tmss.as_ref())
                .assert_word(DATA, 0x00ff);
            assert_eq!(m.bus.tmss, firmware);
            assert!(Arc::ptr_eq(&source, &m.bus.cartridge.bytes));
            assert_eq!(m.bus.cartridge.rom_digest(), hash);
            assert_eq!(m.cpu.queued_instruction_inputs(), queued);
            assert_eq!(states(&m), before);
            assert!(queued.unwrap().iter().all(|input| {
                input
                    .origin
                    .iter()
                    .all(|origin| origin.storage_offset().is_none())
            }));
            let accepted = m.clone();
            for text in ["018000:55 + 020000:01", "018000:55 + 000007:01"] {
                assert!(m.install_rom_patches(&patches(text)).is_err());
                assert_eq!(m, accepted);
                assert_eq!(states(&m), before);
            }
            let mut detached = m.clone();
            Arc::make_mut(&mut detached.bus.cartridge.bytes)[DATA as usize] ^= 1;
            assert!(!detached.supports_rom_cheats());
            let detached_before = detached.clone();
            assert!(detached.install_rom_patches(&[patch(DATA, 0xee)]).is_err());
            assert_eq!(detached, detached_before);
            m.clear_rom_patches();
            assert_eq!(m.bus.tmss, firmware);
            assert_eq!(states(&m), before);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn tmss_guest_byte_word_reads_and_compares_use_immutable_cart_source() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut m = machine(
                &[
                    0x1039, 1, 0x8000, 0x1239, 1, 0x8001, 0x3439, 1, 0x8000, 0x60fe, 0x4e71,
                ],
                region,
                concurrent,
            );
            m.install_rom_patches(&patches("018000:A55A")).unwrap();
            enter_cartridge(&mut m);
            for _ in 0..3 {
                m.step_instruction().unwrap();
            }
            assert_eq!(m.registers().d[0] & 0xff, 0xa5);
            assert_eq!(m.registers().d[1] & 0xff, 0x5a);
            assert_eq!(m.registers().d[2] & 0xffff, 0xa55a);
            m.install_rom_patches(&[
                CheatPatch::WideRomWriteIfEquals {
                    address: DATA,
                    value: CheatValue::Constant(0x5a),
                    compare: CheatValue::Constant(0x12),
                },
                CheatPatch::WideRomWriteIfEquals {
                    address: DATA,
                    value: CheatValue::Constant(0xee),
                    compare: CheatValue::Constant(0xa5),
                },
            ])
            .unwrap();
            assert_eq!(m.peek(DATA), Some(0x5a));
            assert_eq!(m.bus.cartridge.bytes[DATA as usize], 0x12);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn tmss_guest_mapping_queued_sources_codecs_and_reset_keep_current_overlay() {
    let program = [0x7001, 0x0640, 3, 0x60f8, 0x4e71];
    for region in REGIONS {
        for concurrent in [false, true] {
            continuation::check(region, concurrent);
            lifecycle::transitions(region, concurrent);
            let mut source = machine(&program, region, concurrent);
            source
                .install_rom_patches(&patches("010100:7007 + 018000:A55A"))
                .unwrap();
            enter_cartridge(&mut source);
            assert_eq!(source.queued_words(), Some([0x7007, 0x0640]));
            let inputs = source.cpu.queued_instruction_inputs().unwrap();
            assert_eq!(
                inputs[0].origin.map(|origin| origin.storage_offset()),
                [Some(u64::from(CODE)), Some(u64::from(CODE + 1))]
            );
            let saved = states(&source);
            assert_eq!(&saved[0][..12], b"ZMDRAW01\x01\0\0\0");
            assert_eq!(&saved[1][..12], b"ZMDFRONT\x01\0\0\0");
            assert_eq!(&saved[2][..12], b"ZMDDBG01\x02\0\0\0");
            for (set, opcode, data) in [
                ("010100:7007 + 018000:A55A", 7, 0xa5),
                ("010100:7009 + 018000:5AA5", 9, 0x5a),
                ("", 1, 0x12),
            ] {
                for codec in 0..3 {
                    let mut receiver = source.clone();
                    receiver.install_rom_patches(&patches(set)).unwrap();
                    assert_eq!(receiver.cpu.queued_instruction_inputs(), Some(inputs));
                    restore(&mut receiver, &saved, codec);
                    assert_eq!(receiver.queued_words(), Some([0x7007, 0x0640]));
                    assert_eq!(receiver.peek(DATA), Some(data));
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 7);
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 10);
                    receiver.step_instruction().unwrap();
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], opcode);
                    receiver.reset();
                    assert!(receiver.bus.tmss.as_ref().unwrap().boot_mapped());
                    assert_eq!(receiver.registers().pc, 0x200);
                    assert_eq!(receiver.rom_offset_for_cpu_address(CODE), None);
                    enter_cartridge(&mut receiver);
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], opcode);
                    assert_eq!(receiver.peek(DATA), Some(data));
                }
            }
            let (rom, mut firmware) = images(&program);
            firmware[0x7fe] ^= 1;
            let mut wrong = Emulator::new_with_tmss(
                rom,
                LoadConfig {
                    region,
                    concurrent_sound: concurrent,
                },
                firmware,
            )
            .unwrap();
            let before = wrong.clone();
            for (codec, bytes) in saved.iter().enumerate() {
                let result = match codec {
                    0 => wrong.decode_state(bytes),
                    1 => wrong.decode_frontend_state(bytes),
                    _ => wrong.decode_debugger_checkpoint(bytes),
                };
                assert!(result.is_err());
                assert_eq!(wrong, before);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn tmss_standard_sound_keeps_prepared_byte_and_future_reads_use_receiver_set() {
    for region in REGIONS {
        let mut source = machine(&[0x60fe, 0x4e71], region, true);
        enter_cartridge(&mut source);
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 3;
        source.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
        source.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
        sound_tests::pending_rom_read(&mut source);
        let pending = source.sound_schedule.pending.clone().unwrap();
        assert_eq!(pending.cpu.regs().a, 0xa5);
        let deadline = source.sound_deadline().unwrap().get();
        let saved = states(&source);
        for current in [Some(0xa5), Some(0x5a), None] {
            for codec in 0..3 {
                let mut receiver = source.clone();
                receiver
                    .install_rom_patches(
                        &current
                            .map(|value| vec![patch(DATA, value)])
                            .unwrap_or_default(),
                    )
                    .unwrap();
                restore(&mut receiver, &saved, codec);
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                let remaining = deadline - receiver.bus.now.get();
                sound_tests::advance_sound(&mut receiver, remaining - 1);
                assert_ne!(receiver.sound_cpu.regs().a, 0xa5);
                sound_tests::advance_sound(&mut receiver, 1);
                assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
                sound_tests::pending_rom_read(&mut receiver);
                let remaining = receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
                sound_tests::advance_sound(&mut receiver, remaining);
                assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
                assert_eq!(receiver.sound_fault(), None);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn tmss_actual_guest_dma_and_sampled_fifo_keep_source_ordering() {
    for region in REGIONS {
        for concurrent in [false, true] {
            lifecycle::complete_output(region, concurrent);
            lifecycle::trace_changes(region, concurrent);
            let mut program = sound_tests::dma_fifo_program();
            program.splice(
                program.len() - 2..program.len() - 2,
                [0x33fc, 0x0080, 0x00c0, 4],
            );
            let mut m = machine(&program, region, concurrent);
            m.install_rom_patches(&patches("018000:A55A + 018002:B66B"))
                .unwrap();
            enter_cartridge(&mut m);
            for _ in 0..256 {
                m.step_instruction().unwrap();
                if m.vdp().memory().vram()[0x400..0x404] == [0xa5, 0x5a, 0xb6, 0x6b] {
                    break;
                }
            }
            assert_eq!(
                &m.vdp().memory().vram()[0x400..0x404],
                &[0xa5, 0x5a, 0xb6, 0x6b]
            );
            assert_eq!(m.sound_fault(), None);
            let mut fifo = machine(&sound_tests::dma_fifo_program(), region, concurrent);
            enter_cartridge(&mut fifo);
            sound_tests::assert_dma_fifo_retains_sampled_word(fifo, region);
        }
    }
}
