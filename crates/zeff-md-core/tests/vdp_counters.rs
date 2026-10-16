mod vdp_hv {
    use zeff_emu_common::time::MasterTicks;
    use zeff_md_core::{
        Region, Vdp, VdpInterruptError, VdpInterrupts, VdpRegisterError, VdpRegisterPipeline,
        VdpScheduleError, VdpTimingError,
    };

    const REGION: Region = Region::NtscUs;

    fn tick(value: u64) -> MasterTicks {
        MasterTicks::new(value)
    }

    fn timing(error: VdpTimingError) -> VdpScheduleError {
        VdpInterruptError::Timing(error).into()
    }

    fn unchanged(
        device: &Vdp,
        region: Region,
        expected: Result<u16, VdpScheduleError>,
        read: &mut impl FnMut(&Vdp, Region) -> Result<u16, VdpScheduleError>,
    ) {
        let saved = device.clone();
        for _ in 0..2 {
            assert_eq!(read(device, region), expected);
            assert_eq!(*device, saved);
        }
    }

    fn publish(device: &mut Vdp, index: u8, value: u8) {
        device
            .advance_to(tick(device.now().get() + 1), REGION)
            .unwrap();
        let write = device.queue_register(index, value, REGION).unwrap();
        device.advance_to(write.publish_at, REGION).unwrap();
    }

    pub fn horizontal_and_vertical_edges_with(
        mut read: impl FnMut(&Vdp, Region) -> Result<u16, VdpScheduleError>,
    ) {
        for region in [REGION, Region::NtscJapan] {
            let mut device = Vdp::bootstrap_mode5();
            for (at, word) in [
                (0, 0x0000),
                (19, 0x0000),
                (20, 0x0001),
                (2659, 0x0084),
                (2660, 0x0185),
                (2950, 0x0193),
                (2959, 0x0193),
                (2960, 0x01e9),
                (3419, 0x01ff),
                (3420, 0x0100),
                (234 * 3420 + 2659, 0xea84),
                (234 * 3420 + 2660, 0xe585),
                (261 * 3420 + 2659, 0xff84),
                (261 * 3420 + 2660, 0x0085),
                (262 * 3420, 0x0000),
            ] {
                device.advance_to(tick(at), region).unwrap();
                unchanged(&device, region, Ok(word), &mut read);
            }
        }
    }

    pub fn latch_publication_hold_clear_and_clone_with(
        mut read: impl FnMut(&Vdp, Region) -> Result<u16, VdpScheduleError>,
    ) {
        let mut device = Vdp::bootstrap_mode5();
        device.advance_to(tick(101), REGION).unwrap();
        let write = device.queue_register(0, 2, REGION).unwrap();
        assert_eq!(write.publish_at, tick(120));
        device.advance_to(tick(119), REGION).unwrap();
        unchanged(&device, REGION, Ok(5), &mut read);
        device.advance_to(tick(120), REGION).unwrap();
        unchanged(&device, REGION, Ok(6), &mut read);
        let mut cloned = device.clone();
        for at in [2960, 3420, 234 * 3420 + 2660] {
            device.advance_to(tick(at), REGION).unwrap();
            cloned.advance_to(tick(at), REGION).unwrap();
            assert_eq!(device, cloned);
            unchanged(&device, REGION, Ok(6), &mut read);
            unchanged(&cloned, REGION, Ok(6), &mut read);
        }
        publish(&mut device, 0, 2);
        unchanged(&device, REGION, Ok(6), &mut read);
        publish(&mut device, 0, 0);
        unchanged(&device, REGION, Ok(0xe587), &mut read);
        publish(&mut device, 0, 2);
        unchanged(&device, REGION, Ok(0xe588), &mut read);
        device.advance_to(tick(262 * 3420), REGION).unwrap();
        unchanged(&device, REGION, Ok(0xe588), &mut read);
        unchanged(&Vdp::bootstrap_mode5(), REGION, Ok(0), &mut read);
    }

    pub fn side_effect_free_with(
        mut read: impl FnMut(&Vdp, Region) -> Result<u16, VdpScheduleError>,
    ) {
        let mut device = Vdp::bootstrap_mode5();
        device.advance_to(tick(1), REGION).unwrap();
        device.write_control(0x4000, REGION).unwrap();
        device.write_control(0, REGION).unwrap();
        device.write_data(0xbeef, REGION).unwrap();
        device.write_control(0, REGION).unwrap();
        device.write_control(0, REGION).unwrap();
        device.advance_to(tick(71), REGION).unwrap();
        assert!(device.read_cache().is_ready());
        unchanged(&device, REGION, Ok(3), &mut read);
        device.write_control(0x4567, REGION).unwrap();
        assert_eq!(device.control().pending_word(), Some(0x4567));
        unchanged(&device, REGION, Ok(3), &mut read);
        device.advance_to(tick(224 * 3420 + 11), REGION).unwrap();
        assert!(device.interrupts().vertical_pending());
        unchanged(&device, REGION, Ok(0xe000), &mut read);
        assert_eq!(device.control().pending_word(), Some(0x4567));
        assert!(device.interrupts().vertical_pending());
    }

    pub fn region_and_mode_guards_with(
        mut read: impl FnMut(&Vdp, Region) -> Result<u16, VdpScheduleError>,
    ) {
        let mut device = Vdp::bootstrap_mode5();
        device.advance_to(tick(101), REGION).unwrap();
        unchanged(&device, REGION, Ok(5), &mut read);
        unchanged(&device, Region::NtscJapan, Ok(5), &mut read);
        unchanged(&device, Region::Pal, Ok(5), &mut read);
        for (index, value) in [(1, 0x44), (12, 0x81)] {
            let mut pending = device.clone();
            pending.queue_register(index, value, REGION).unwrap();
            unchanged(
                &pending,
                REGION,
                Err(timing(VdpTimingError::PendingModeChange)),
                &mut read,
            );
            let mut latched = device.clone();
            publish(&mut latched, 0, 2);
            latched.advance_to(tick(121), REGION).unwrap();
            let write = latched.queue_register(index, value, REGION).unwrap();
            let mut reconstructed =
                Vdp::from_known_state(*latched.interrupts(), *latched.registers(), REGION).unwrap();
            unchanged(
                &reconstructed,
                REGION,
                Err(timing(VdpTimingError::PendingModeChange)),
                &mut read,
            );
            reconstructed.advance_to(write.publish_at, REGION).unwrap();
            unchanged(&reconstructed, REGION, Ok(6), &mut read);
        }
        publish(&mut device, 1, 0x84);
        unchanged(&device, REGION, Ok(6), &mut read);
        let saved = device.clone();
        for (index, value) in [(1, 0x0c), (12, 0x80), (12, 2)] {
            assert_eq!(
                device.queue_register(index, value, REGION),
                Err(VdpRegisterError::UnsupportedModeChange { index, value }.into()),
            );
            assert_eq!(device, saved);
        }
        assert_eq!(
            Vdp::from_known_state(
                VdpInterrupts::at_counters_zero(tick(0), 0, false, false),
                VdpRegisterPipeline::at_h32_edge(tick(0)),
                REGION,
            ),
            Err(timing(VdpTimingError::UnknownRegister(1))),
        );
        let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
        for (index, value) in [(0, 0), (1, 0x44), (10, 0), (12, 2)] {
            registers
                .advance_to(tick(registers.now().get() + 1))
                .unwrap();
            let write = registers.queue_register(index, value).unwrap();
            registers.advance_to(write.publish_at).unwrap();
        }
        assert_eq!(
            Vdp::from_known_state(
                VdpInterrupts::at_counters_zero(tick(80), 0, false, false),
                registers,
                REGION,
            ),
            Err(timing(VdpTimingError::UnsupportedMode {
                index: 12,
                value: 2
            })),
        );
    }

    pub fn known_epoch_and_reversal_with(
        mut read: impl FnMut(&Vdp, Region) -> Result<u16, VdpScheduleError>,
    ) {
        let mut registers = *Vdp::bootstrap_mode5().registers();
        registers.advance_to(tick(80)).unwrap();
        let mut device = Vdp::from_known_state(
            VdpInterrupts::at_counters_zero(tick(80), 7, false, false),
            registers,
            REGION,
        )
        .unwrap();
        unchanged(&device, REGION, Ok(0), &mut read);
        device.advance_to(tick(3040), REGION).unwrap();
        unchanged(&device, REGION, Ok(0x01e9), &mut read);
        let saved = device.clone();
        assert_eq!(
            device.advance_to(tick(3039), REGION),
            Err(VdpInterruptError::TimeReversal {
                current: tick(3040),
                requested: tick(3039)
            }
            .into()),
        );
        assert_eq!(device, saved);
        unchanged(&device, REGION, Ok(0x01e9), &mut read);
    }

    #[test]
    fn horizontal_and_vertical_edges() {
        horizontal_and_vertical_edges_with(Vdp::read_hv);
    }

    #[test]
    fn latch_publication_hold_clear_and_clone() {
        latch_publication_hold_clear_and_clone_with(Vdp::read_hv);
    }

    #[test]
    fn side_effect_free() {
        side_effect_free_with(Vdp::read_hv);
    }

    #[test]
    fn region_and_mode_guards() {
        region_and_mode_guards_with(Vdp::read_hv);
    }

    #[test]
    fn known_epoch_and_reversal() {
        known_epoch_and_reversal_with(Vdp::read_hv);
    }
}

mod vdp_address_carry {
    use zeff_emu_common::time::MasterTicks;
    use zeff_md_core::{
        ControlCommand, Region, Vdp, VdpMemory, VdpReadCache, VdpWriteError, VdpWriteFifo,
    };

    const REGION: Region = Region::NtscUs;
    fn tick(value: u64) -> MasterTicks {
        MasterTicks::new(value)
    }

    fn register(device: &mut Vdp, index: u8, value: u8) {
        device
            .advance_to(tick(device.now().get() + 1), REGION)
            .unwrap();
        let write = device.queue_register(index, value, REGION).unwrap();
        device.advance_to(write.publish_at, REGION).unwrap();
    }

    fn command(device: &mut Vdp, address: u32, code: u8) {
        device
            .write_control(
                (address as u16 & 0x3fff) | (u16::from(code & 3) << 14),
                REGION,
            )
            .unwrap();
        device
            .write_control(
                ((address >> 14) as u16 & 7) | (u16::from(code & 0x3c) << 2),
                REGION,
            )
            .unwrap();
    }

    pub fn fifo_and_read_wrap() {
        for (address, increment, expected) in [
            (0x3fff, 1, 0x4000),
            (0x7fff, 1, 0x8000),
            (0xbfff, 1, 0xc000),
            (0xffff, 1, 0x10000),
            (0x1ffff, 1, 0),
            (0x1ff80, 255, 0x7f),
            (0x1ffff, 0, 0x1ffff),
        ] {
            let mut memory = VdpMemory::default();
            let mut fifo = VdpWriteFifo::default();
            let mut command = ControlCommand { address, code: 1 };
            let entry = fifo
                .push_data(&mut command, Some(increment), 0xabcd)
                .unwrap();
            assert_eq!(entry.command.address, address);
            assert_eq!(
                command,
                ControlCommand {
                    address: expected,
                    code: 1
                }
            );
            fifo.complete_write(&mut memory).unwrap();
            assert_eq!(command.address, expected);
            let physical = address as u16 & !1;
            let bytes = if address & 1 == 0 {
                [0xab, 0xcd]
            } else {
                [0xcd, 0xab]
            };
            assert_eq!(
                &memory.vram()[usize::from(physical)..usize::from(physical) + 2],
                &bytes
            );
            let mut cache = VdpReadCache::default();
            let mut read = ControlCommand { address, code: 0 };
            cache
                .complete_read(&memory, &fifo, &mut read, Some(increment))
                .unwrap();
            assert_eq!(read.address, expected);
            cache.read().unwrap();
            assert_eq!(read.address, expected);
        }
        let mut memory = VdpMemory::default();
        let saved = memory.clone();
        assert_eq!(
            memory.write_word(
                ControlCommand {
                    address: 0x20000,
                    code: 1
                },
                0xabcd
            ),
            Err(VdpWriteError::UnsupportedCommand(ControlCommand {
                address: 0x20000,
                code: 1
            }))
        );
        assert_eq!(memory, saved);
    }

    pub fn full_fill_with(
        mut advance: impl FnMut(
            &mut Vdp,
            MasterTicks,
            Region,
        ) -> Result<(), zeff_md_core::VdpScheduleError>,
    ) {
        for length in [0xffffu16, 0] {
            let mut device = Vdp::bootstrap_mode5();
            for (index, value) in [
                (1, 0x14),
                (12, 0x81),
                (15, 1),
                (19, length as u8),
                (20, (length >> 8) as u8),
                (21, 0),
                (22, 0),
                (23, 0x80),
            ] {
                register(&mut device, index, value);
            }
            command(&mut device, 0, 0x21);
            device.write_data(0xabcd, REGION).unwrap();
            let mut partitioned = device.clone();
            advance(&mut device, tick(2 * 896040), REGION).unwrap();
            for at in [3420, 766088, 896040, 2 * 896040] {
                advance(&mut partitioned, tick(at), REGION).unwrap();
            }
            assert_eq!(device, partitioned);
            assert_eq!(
                device.command(),
                ControlCommand {
                    address: if length == 0 { 0x10001 } else { 0x10000 },
                    code: 1
                }
            );
            for (address, &value) in device.memory().vram().iter().enumerate() {
                assert_eq!(
                    value,
                    if length != 0 && address == 1 {
                        0xcd
                    } else {
                        0xab
                    },
                    "{address:04x}"
                );
            }
            assert_eq!(device.registers().registers()[19], Some(0));
            assert_eq!(device.registers().registers()[20], Some(0));
            assert_eq!(device.registers().registers()[21], Some(length as u8));
            assert_eq!(
                device.registers().registers()[22],
                Some((length >> 8) as u8)
            );
            assert_eq!(device.read_status(REGION).unwrap() & 2, 0);
        }
    }

    pub fn upper_alias_preserves_sat() {
        let mut device = Vdp::bootstrap_mode5();
        for (index, value) in [
            (1, 0x14),
            (15, 2),
            (5, 0),
            (19, 1),
            (20, 0),
            (21, 0),
            (22, 0),
            (23, 0x80),
        ] {
            register(&mut device, index, value);
        }
        command(&mut device, 0, 1);
        device.write_data(128, REGION).unwrap();
        device.write_data(0, REGION).unwrap();
        device.advance_to(tick(500), REGION).unwrap();
        assert_eq!(
            device.sprite_cache().unwrap().entry_words(0),
            Ok([Some(128), Some(0)])
        );
        command(&mut device, 0x10000, 0x21);
        device.write_data(0xabcd, REGION).unwrap();
        device.advance_to(tick(600), REGION).unwrap();
        assert_eq!(&device.memory().vram()[..4], &[0xab, 0xcd, 0, 0xab]);
        assert_eq!(
            device.sprite_cache().unwrap().entry_words(0),
            Ok([Some(128), Some(0)])
        );
    }

    #[test]
    fn wrap() {
        fifo_and_read_wrap();
    }
    #[test]
    fn fill() {
        full_fill_with(Vdp::advance_to);
    }
    #[test]
    fn cache_identity() {
        upper_alias_preserves_sat();
    }
}
