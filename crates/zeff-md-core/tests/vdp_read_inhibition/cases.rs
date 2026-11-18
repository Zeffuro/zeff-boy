use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, ControlWrite, Region, Vdp, VdpControl, VdpDmaError, VdpPortError, VdpReadError,
    VdpWriteError,
};

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn register(vdp: &mut Vdp, index: u8, value: u8, region: Region) {
    vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    let write = vdp.queue_register(index, value, region).unwrap();
    vdp.advance_to(write.publish_at, region).unwrap();
}

fn first(vdp: &mut Vdp, address: u32, code: u8, region: Region) {
    vdp.write_control(
        (address as u16 & 0x3fff) | u16::from(code & 3) << 14,
        region,
    )
    .unwrap();
}

fn second(address: u32, code: u8) -> u16 {
    ((address >> 14) as u16 & 7) | u16::from(code & 0x3c) << 2
}

fn command(vdp: &mut Vdp, address: u32, code: u8, region: Region) {
    first(vdp, address, code, region);
    assert_eq!(
        vdp.write_control(second(address, code), region),
        Ok(ControlWrite::Command(ControlCommand { address, code }))
    );
}

fn fixture(region: Region, horizontal: u8) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    register(&mut vdp, 12, horizontal, region);
    register(&mut vdp, 15, 2, region);
    register(&mut vdp, 1, 4, region);
    for (address, data) in [(0, 0x369c), (0x8000, 0xa135), (0x8002, 0xcdef)] {
        command(&mut vdp, address, 1, region);
        vdp.write_data(data, region).unwrap();
    }
    vdp.advance_to(tick(vdp.now().get() + 3420), region)
        .unwrap();
    register(&mut vdp, 15, 1, region);
    command(&mut vdp, 0, 0, region);
    let end = vdp.data_read_wait(region).unwrap().unwrap();
    vdp.advance_to(end, region).unwrap();
    assert!(vdp.read_cache().is_ready());
    assert_eq!(vdp.command().address, 1);
    vdp
}

pub fn inhibited_commands_latch_without_prefetch_and_reject_data_accesses() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for horizontal in [0, 0x81] {
            let seed = fixture(region, horizontal);
            for code in [0x14, 0x18, 0x1c] {
                let mut vdp = seed.clone();
                let memory = vdp.memory().clone();
                let fifo = vdp.fifo().clone();
                let now = vdp.now();
                let buffered = vdp.read_cache().buffered_word();
                command(&mut vdp, 0x8000, code, region);
                assert_eq!(vdp.now(), now);
                assert_eq!(vdp.control().pending_word(), None);
                assert_eq!(vdp.registers().auto_increment(), Some(1));
                assert!(vdp.registers().pending().is_none());
                assert!(!vdp.read_cache().is_ready());
                assert_eq!(vdp.read_cache().buffered_word(), buffered);
                assert_eq!(vdp.memory(), &memory);
                assert_eq!(vdp.fifo(), &fifo);
                let before = vdp.clone();
                assert_eq!(
                    vdp.data_read_wait(region),
                    Err(VdpPortError::Read(VdpReadError::UnsupportedCommand(
                        vdp.command()
                    )))
                );
                assert_eq!(vdp, before);
                assert_eq!(
                    vdp.read_data(region),
                    Err(VdpPortError::Read(VdpReadError::Empty))
                );
                assert_eq!(vdp, before);
                if code != 0x14 {
                    assert_eq!(
                        vdp.write_data(0xbeef, region),
                        Err(VdpPortError::Data(VdpWriteError::UnsupportedCommand(
                            vdp.command()
                        )))
                    );
                    assert_eq!(vdp, before);
                }
                vdp.advance_to(tick(now.get() + 3420), region).unwrap();
                assert_eq!(
                    vdp.command(),
                    ControlCommand {
                        address: 0x8000,
                        code
                    }
                );
                assert!(!vdp.read_cache().is_ready());
                assert_eq!(vdp.read_cache().buffered_word(), buffered);
                assert_eq!(vdp.memory(), &memory);
                assert_eq!(vdp.fifo(), &fifo);
                command(&mut vdp, 0x8000, 0, region);
                let end = vdp.data_read_wait(region).unwrap().unwrap();
                vdp.advance_to(end, region).unwrap();
                assert_eq!(vdp.read_cache().buffered_word(), Some(0xa135));
                assert_eq!(vdp.command().address, 0x8001);
            }
        }
    }
}

pub fn register_looking_second_words_keep_command_priority_with_dma_disabled() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut vdp = fixture(region, 0);
        let memory = vdp.memory().clone();
        for second_word in [0x937f, 0x93ff] {
            vdp.write_control(0x0010, region).unwrap();
            let command = ControlCommand {
                address: 0x1c010,
                code: 0x1c,
            };
            assert_eq!(
                vdp.write_control(second_word, region),
                Ok(ControlWrite::Command(ControlCommand {
                    code: ((second_word >> 2) & 0x3c) as u8,
                    ..command
                }))
            );
            assert_eq!(vdp.command(), command);
            assert_eq!(vdp.control().pending_word(), None);
            assert_eq!(vdp.registers().auto_increment(), Some(1));
            assert!(vdp.registers().pending().is_none());
            assert!(!vdp.read_cache().is_ready());
        }
        vdp.write_control(0x0000, region).unwrap();
        assert_eq!(
            vdp.write_control(0x8f02, region),
            Ok(ControlWrite::Command(ControlCommand {
                address: 0x8000,
                code: 0
            }))
        );
        assert_eq!(vdp.registers().auto_increment(), Some(1));
        assert!(vdp.registers().pending().is_none());
        let end = vdp.data_read_wait(region).unwrap().unwrap();
        vdp.advance_to(end, region).unwrap();
        assert_eq!(
            vdp.command(),
            ControlCommand {
                address: 0x8001,
                code: 0
            }
        );
        assert_eq!(vdp.read_cache().buffered_word(), Some(0xa135));
        assert_eq!(vdp.memory(), &memory);
    }
}

pub fn active_unsupported_reads_and_dma_dispatch_keep_their_boundaries() {
    for region in [Region::NtscUs, Region::Pal] {
        let seed = fixture(region, 0);
        let mut active = seed.clone();
        first(&mut active, 0x8000, 0x0c, region);
        let before = active.clone();
        assert_eq!(
            active.write_control(second(0x8000, 0x0c), region),
            Err(VdpPortError::Control(VdpWriteError::UnsupportedCommand(
                ControlCommand {
                    address: 0x8000,
                    code: 0x0c
                }
            )))
        );
        assert_eq!(active, before);
        let mut dma = seed;
        for (index, value) in [
            (1, 0x14),
            (15, 2),
            (19, 2),
            (20, 0),
            (21, 0),
            (22, 0),
            (23, 0xc0),
        ] {
            register(&mut dma, index, value, region);
        }
        command(&mut dma, 0x8000, 0x3c, region);
        let armed = dma.clone();
        assert_eq!(
            dma.data_read_wait(region),
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        );
        assert_eq!(
            dma.read_data(region),
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        );
        assert_eq!(
            dma.write_data(0, region),
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        );
        assert_eq!(
            dma.write_control(0x8f02, region),
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        );
        assert_eq!(dma, armed);
        dma.advance_to(tick(dma.now().get() + 3420), region)
            .unwrap();
        assert_eq!(
            dma.command(),
            ControlCommand {
                address: 0x8004,
                code: 0x1c
            }
        );
        assert_eq!(
            &dma.memory().vram()[0x8000..0x8004],
            &[0xa1, 0x9c, 0xcd, 0x36]
        );
        assert!(!dma.read_cache().is_ready());
        let mut parser = VdpControl::default();
        let mut retained = ControlCommand {
            address: 0,
            code: 0x20,
        };
        parser
            .complete_control_write(&mut retained, 0x0010, false)
            .unwrap();
        parser
            .complete_control_write(&mut retained, 0x937f, false)
            .unwrap();
        assert_eq!(
            retained,
            ControlCommand {
                address: 0x1c010,
                code: 0x3c
            }
        );
    }
}
