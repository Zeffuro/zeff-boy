mod vdp_line {
    #[path = "cases.rs"]
    mod cases;
    #[path = "support.rs"]
    pub(crate) mod support;

    #[test]
    fn asymmetric_canonical_row() {
        cases::asymmetric_canonical_row();
    }

    #[test]
    fn owned_snapshot_and_replay() {
        cases::owned_snapshot_and_replay();
    }

    #[test]
    fn publication_and_cached_layout() {
        cases::publication_and_cached_layout();
    }

    #[test]
    fn complete_sprite_list_limits() {
        cases::complete_sprite_list_limits();
    }

    #[test]
    fn pending_inputs_and_render_guards() {
        cases::pending_inputs_and_render_guards();
    }

    #[test]
    fn logical_row_and_schedule_guards() {
        cases::logical_row_and_schedule_guards();
    }
}

mod vdp_logical_line {
    #[path = "cases.rs"]
    mod cases;
    #[path = "guards.rs"]
    mod guards;
    #[path = "support.rs"]
    mod support;

    #[test]
    fn all_logical_rows() {
        cases::all_logical_rows();
    }

    #[test]
    fn static_rows_and_timed_guards() {
        cases::static_rows_and_timed_guards();
    }

    #[test]
    fn coordinates_and_sprite_guards() {
        guards::coordinates_and_sprite_guards();
    }

    #[test]
    fn pending_and_render_guards() {
        guards::pending_and_render_guards();
    }

    #[test]
    fn configuration_guards() {
        guards::configuration_guards();
    }
}

mod vdp_stable_display {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn active_fifo_and_captured_targets() {
        cases::active_fifo_and_captured_targets();
    }

    #[test]
    fn active_reads_and_automatic_fetch() {
        cases::active_reads_and_automatic_fetch();
    }

    #[test]
    fn nonactive_fifo_and_reads() {
        cases::nonactive_fifo_and_reads();
    }

    #[test]
    fn margins_and_atomic_preflight() {
        cases::margins_and_atomic_preflight();
    }

    #[test]
    fn constructor_and_legacy_guards() {
        cases::constructor_and_legacy_guards();
    }

    #[test]
    fn publication_history_and_interrupts() {
        cases::publication_history_and_interrupts();
    }
}

mod vdp_h40_machine {
    use zeff_emu_common::time::Reset;
    use zeff_md_core::{Emulator, LoadConfig, MachineFault, MachineStep};

    fn write(program: &mut Vec<u16>, port: u16, value: u16) {
        program.extend([0x33fc, value, 0x00c0, port]);
    }

    fn command(program: &mut Vec<u16>, address: u16, code: u16) {
        write(program, 4, (address & 0x3fff) | ((code & 3) << 14));
        write(program, 4, (address >> 14) | ((code & 0x3c) << 2));
    }

    pub fn image(h40: bool) -> Vec<u8> {
        let mut program = Vec::new();
        for (index, value) in [
            (12, if h40 { 0x81 } else { 0 }),
            (2, 0x30),
            (3, 0x2c),
            (4, 5),
            (5, 0x10),
            (7, 0),
            (11, 0),
            (13, 0x3f),
            (15, 2),
            (16, 1),
        ] {
            write(&mut program, 4, 0x8000 | (index << 8) | value);
        }
        command(&mut program, 2, 3);
        write(&mut program, 0, 0x000e);
        write(&mut program, 0, 0x00e0);
        for (address, data) in [(32, 0x1111), (64, 0x2222)] {
            command(&mut program, address, 1);
            for _ in 0..16 {
                write(&mut program, 0, data);
            }
        }
        for row in 0..28 {
            command(&mut program, 0xc000 + row * 64 * 2, 1);
            for column in 0..40 {
                write(
                    &mut program,
                    0,
                    if column == 0 || column & 1 != 0 { 1 } else { 2 },
                );
            }
        }
        write(&mut program, 4, 0x8144);
        program.extend([0x3039, 0x00c0, 8, 0x3239, 0x00c0, 4, 0x60f2]);
        let mut image = vec![0; 0x200 + program.len() * 2];
        image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        for (index, word) in program.into_iter().enumerate() {
            image[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
        }
        image
    }

    pub fn cpu_to_frame_with(
        mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
    ) {
        for h40 in [false, true] {
            for concurrent_sound in [false, true] {
                let mut machine = Emulator::new(
                    image(h40),
                    LoadConfig {
                        concurrent_sound,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(machine.framebuffer_dimensions(), (256, 224));
                assert!(!machine.frame_ready());
                for _ in 0..50_000 {
                    step(&mut machine).unwrap();
                    if machine.frame_count() == 2 {
                        break;
                    }
                }
                let width = if h40 { 320 } else { 256 };
                assert_eq!(machine.frame_count(), 2);
                assert!(machine.frame_ready());
                assert_eq!(machine.video_error(), None);
                assert_eq!(machine.framebuffer_dimensions(), (width, 224));
                assert_eq!(machine.framebuffer().len(), width * 224 * 4);
                let pixel =
                    |x, y| &machine.framebuffer()[(y * width + x) * 4..(y * width + x) * 4 + 4];
                assert_eq!(pixel(0, 0), &[255, 0, 0, 255]);
                assert_eq!(pixel(255, 223), &[255, 0, 0, 255]);
                if h40 {
                    assert_eq!(pixel(256, 223), &[0, 255, 0, 255]);
                    assert_eq!(pixel(319, 223), &[255, 0, 0, 255]);
                }
                for (index, rgba) in machine.framebuffer().as_chunks::<4>().0.iter().enumerate() {
                    let column = (index % width) / 8;
                    assert_eq!(
                        rgba,
                        if column == 0 || column & 1 != 0 {
                            &[255, 0, 0, 255]
                        } else {
                            &[0, 255, 0, 255]
                        }
                    );
                }
                assert_eq!(machine.registers().d[1] & 0x0100, 0);
                machine.clear_frame_ready();
                assert!(!machine.frame_ready());
                let mut clone = machine.clone();
                for _ in 0..100 {
                    assert_eq!(step(&mut machine), step(&mut clone));
                }
                assert_eq!(machine, clone);
            }
        }
    }

    pub fn reset_frame_with(
        mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
        mut restart: impl FnMut(
            &mut Emulator,
        )
            -> Result<(), zeff_m68k::timing::TimingFault<zeff_md_core::BusError>>,
        mut reset: impl FnMut(&mut Emulator),
    ) {
        for concurrent_sound in [false, true] {
            let mut machine = Emulator::new(
                image(true),
                LoadConfig {
                    concurrent_sound,
                    ..Default::default()
                },
            )
            .unwrap();
            for _ in 0..50_000 {
                step(&mut machine).unwrap();
                if machine.frame_count() == 2 {
                    break;
                }
            }
            assert_eq!(machine.frame_count(), 2);
            let framebuffer = machine.framebuffer().to_vec();
            let pointer = machine.framebuffer().as_ptr();
            restart(&mut machine).unwrap();
            assert_eq!(machine.framebuffer(), framebuffer);
            assert_eq!(machine.frame_count(), 2);
            assert_eq!(machine.framebuffer().as_ptr(), pointer);
            reset(&mut machine);
            assert_eq!(machine.framebuffer_dimensions(), (256, 224));
            assert_eq!(machine.frame_count(), 0);
            assert!(!machine.frame_ready());
            assert_eq!(machine.video_error(), None);
            assert_eq!(machine.framebuffer().as_ptr(), pointer);
            assert!(machine.framebuffer().iter().all(|&channel| channel == 0));
        }
    }

    #[test]
    fn cpu_to_frame() {
        cpu_to_frame_with(Emulator::step_instruction);
    }

    #[test]
    fn reset_frame() {
        reset_frame_with(
            Emulator::step_instruction,
            Emulator::reset_cpu,
            Reset::reset,
        );
    }
}
