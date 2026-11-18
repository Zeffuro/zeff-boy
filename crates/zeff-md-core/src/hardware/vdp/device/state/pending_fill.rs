use super::*;

#[path = "zero_color_fill.rs"]
mod zero_color_fill;

pub(super) fn waiting_control_contract() {
    zero_color_fill::retirement_contract();
    for region in [Region::NtscUs, Region::Pal] {
        let mut base = Vdp::bootstrap_mode5();
        for (index, value) in [
            (1, 0x14),
            (15, 2),
            (19, 0x29),
            (20, 0),
            (21, 0),
            (22, 0),
            (23, 0x80),
        ] {
            register(&mut base, index, value, region);
        }
        command(&mut base, 0xe000, 0x21, region);
        assert_eq!(base.state.dma, Some(dma::Phase::FillWaiting));
        for (index, value) in [
            (0, 4),
            (1, 0x34),
            (2, 0x20),
            (10, 0xff),
            (12, 8),
            (15, 3),
            (19, 2),
            (20, 1),
            (19, 0xff),
            (20, 0),
            (21, 0x16),
            (22, 0x80),
            (23, 0x9c),
        ] {
            let mut vdp = base.clone();
            let before_memory = vdp.memory.clone();
            let source = VdpDmaRegisters::from_registers(vdp.registers())
                .unwrap()
                .source();
            vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
            let old = vdp.registers().registers()[index as usize];
            let word = 0x8000 | (index as u16) << 8 | value as u16;
            vdp.write_control(word, region).unwrap();
            assert_eq!(vdp.command().address, 0xc000 | u32::from(word & 0x3fff));
            assert_eq!(vdp.command().code, 0x32);
            let pending = vdp.registers().pending().unwrap();
            let mut restored_pending = restored(&vdp, region);
            for invalid in [0x9500, 0x9780, 0x4000] {
                let before = vdp.clone();
                assert!(vdp.write_control(invalid, region).is_err());
                assert_eq!(vdp, before);
            }
            let before = vdp.clone();
            assert!(vdp.data_write_wait(region).is_err());
            assert_eq!(vdp, before);
            assert!(vdp.write_data(0xabcd, region).is_err());
            assert_eq!(vdp, before);
            vdp.advance_to(tick(pending.publish_at.get() - 1), region)
                .unwrap();
            assert_eq!(vdp.registers().registers()[index as usize], old);
            vdp.advance_to(pending.publish_at, region).unwrap();
            restored_pending
                .advance_to(pending.publish_at, region)
                .unwrap();
            assert_eq!(vdp, restored_pending);
            assert_eq!(vdp.memory, before_memory);
            assert_eq!(vdp.registers().registers()[index as usize], Some(value));
            let expected_source = match index {
                21 => (source & 0xff00) | u16::from(value),
                22 => (source & 0xff) | (u16::from(value) << 8),
                _ => source,
            };
            assert_eq!(
                VdpDmaRegisters::from_registers(vdp.registers())
                    .unwrap()
                    .source(),
                expected_source
            );
            restored(&vdp, region);
            let before = vdp.clone();
            assert!(vdp.write_data(0xabcd, region).is_err());
            assert_eq!(vdp, before);
        }
        for word in [0x8104, 0x8154, 0x8c02, 0x8c81, 0x9700, 0x97c0] {
            let mut rejected = base.clone();
            let before = rejected.clone();
            assert!(rejected.write_control(word, region).is_err());
            assert_eq!(rejected, before);
        }
        let mut vdp = base.clone();
        vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
        let queued = vdp.queue_register(19, 2, region).unwrap();
        assert_eq!(vdp.command().code, 0x21);
        restored(&vdp, region);
        vdp.advance_to(queued.publish_at, region).unwrap();
        vdp.write_control(0x4000, region).unwrap();
        assert_eq!(vdp.command().code, 0x31);
        let mut half = restored(&vdp, region);
        let before = vdp.clone();
        assert!(vdp.data_write_wait(region).is_err());
        assert_eq!(vdp, before);
        assert!(vdp.write_data(0xabcd, region).is_err());
        assert_eq!(vdp, before);
        assert!(vdp.write_control(0x10, region).is_err());
        assert_eq!(vdp, before);
        vdp.write_control(0x9380, region).unwrap();
        half.write_control(0x9380, region).unwrap();
        assert_eq!(vdp, half);
        assert_eq!(vdp.registers().registers()[19], Some(2));
        assert!(vdp.registers().pending().is_none());
        assert_eq!(
            vdp.command(),
            ControlCommand {
                address: 0,
                code: 0x21
            }
        );
        assert_ne!(vdp.read_cpu_status(region).unwrap() & 2, 0);
        let mut copy = restored(&vdp, region);
        vdp.write_data(0xabcd, region).unwrap();
        copy.write_data(0xabcd, region).unwrap();
        let end = tick(vdp.now().get() + H32_LINE_TICKS);
        vdp.advance_to(end, region).unwrap();
        copy.advance_to(end, region).unwrap();
        assert_eq!(vdp, copy);
        assert!(vdp.state.dma.is_none());
        assert_eq!(&vdp.memory.vram()[..6], &[0xab, 0xcd, 0, 0xab, 0, 0xab]);
        for (second, code) in [(2, 0x01), (0x42, 0x11)] {
            let mut cancelled = base.clone();
            let counters = cancelled.registers().registers()[19..24].to_vec();
            cancelled.write_control(0x5800, region).unwrap();
            let mut half = restored(&cancelled, region);
            cancelled.write_control(second, region).unwrap();
            half.write_control(second, region).unwrap();
            assert_eq!(cancelled, half);
            assert_eq!(
                cancelled.command(),
                ControlCommand {
                    address: 0x9800,
                    code
                }
            );
            assert!(cancelled.state.dma.is_none());
            assert_eq!(cancelled.memory, base.memory);
            assert_eq!(cancelled.read_cpu_status(region).unwrap() & 2, 0);
            let mut copy = restored(&cancelled, region);
            cancelled.write_data(0xabcd, region).unwrap();
            copy.write_data(0xabcd, region).unwrap();
            let end = tick(cancelled.now().get() + H32_LINE_TICKS);
            cancelled.advance_to(end, region).unwrap();
            copy.advance_to(end, region).unwrap();
            assert_eq!(cancelled, copy);
            assert_eq!(
                &cancelled.memory.vram()[0x9800..0x9806],
                &[0xab, 0xcd, 0, 0, 0, 0]
            );
            assert_eq!(cancelled.registers().registers()[19..24], counters);
            command(&mut cancelled, 0xe000, 0x21, region);
            assert_eq!(cancelled.state.dma, Some(dma::Phase::FillWaiting));
            cancelled.write_data(0x1234, region).unwrap();
            let mut fresh_fill = restored(&cancelled, region);
            let end = tick(cancelled.now().get() + H32_LINE_TICKS);
            cancelled.advance_to(end, region).unwrap();
            fresh_fill.advance_to(end, region).unwrap();
            assert_eq!(cancelled, fresh_fill);
            assert!(cancelled.state.dma.is_none());
            assert_eq!(
                &cancelled.memory.vram()[0xe000..0xe004],
                &[0x12, 0x34, 0, 0x12]
            );
        }
        let mut status = base.clone();
        status.write_control(0x4004, region).unwrap();
        assert_ne!(status.read_cpu_status(region).unwrap() & 2, 0);
        assert!(status.control().pending_word().is_none());
        restored(&status, region);
        let before = status.clone();
        assert!(status.write_control(0x9700, region).is_err());
        assert_eq!(status, before);
        let mut invalid = base.clone();
        invalid
            .advance_to(tick(invalid.now().get() + 1), region)
            .unwrap();
        invalid.state.command.code = 0x32;
        invalid.state.registers.queue_register(23, 0).unwrap();
        assert!(
            invalid
                .write_state_in_region(&mut StateWriter::new(), region)
                .is_err()
        );
    }
}
