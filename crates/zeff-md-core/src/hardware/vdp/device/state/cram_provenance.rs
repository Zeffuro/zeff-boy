use super::*;
use crate::{VdpReadError, VdpRenderError};

pub(super) fn cold_fill_retirement_and_reads() {
    for increment in [1, 2] {
        let mut vdp = cold_fill(increment, false);
        let capture_source = cold_fill(increment, true);
        let mut captured = capture_source.clone();
        let capture_at = tick(captured.now().get() + 1000);
        assert_eq!(
            captured.capture_line_at(capture_at, REGION),
            Err(crate::VdpLineError::Render(VdpRenderError::UnknownCram(0)))
        );
        assert_eq!(captured, capture_source);
        let mut copy = restored(&vdp, REGION);
        for _ in 0..2 {
            let at = next_slot(&vdp, REGION);
            vdp.advance_to(at, REGION).unwrap();
            copy.advance_to(at, REGION).unwrap();
            assert_eq!(vdp, copy);
            copy = restored(&copy, REGION);
        }
        assert!(vdp.state.dma.is_none());
        let unknown = if increment == 1 { (1 << 63) | 1 } else { 3 };
        assert_eq!(vdp.memory.cram_known_mask(), !unknown);
        assert_eq!(vdp.memory.known_cram(), None);
        assert_eq!(vdp.memory.cram_word(0), None);
        assert_eq!(vdp.memory.cram_word(2), Some(0));
        assert_eq!(
            vdp.memory.render_cram::<true>(0),
            Err(VdpRenderError::UnknownCram(0))
        );
        assert_eq!(vdp.memory.render_cram::<true>(2), Ok(0));
        let before = vdp.clone();
        assert_eq!(
            vdp.prepare_raster_line(0, REGION),
            Err(crate::VdpLineError::Render(VdpRenderError::UnknownCram(0)))
        );
        assert_eq!(vdp, before);
        let mut known_color = vdp.clone();
        known_color
            .memory
            .write_word(
                ControlCommand {
                    address: 0,
                    code: CRAM_WRITE_CODE,
                },
                0,
            )
            .unwrap();
        assert!(known_color.prepare_raster_line(0, REGION).is_ok());
        assert_eq!(
            vdp.read_cpu_status(REGION),
            known_color.read_cpu_status(REGION)
        );
        assert_eq!(vdp.state, known_color.state);

        let read = ControlCommand {
            address: 0,
            code: CRAM_READ_CODE,
        };
        assert_eq!(
            vdp.memory.read_word_with_history(read, Some(0xffff)),
            Err(VdpReadError::UnknownCram(0))
        );
        copy.write_control(0, REGION).unwrap();
        let before = copy.clone();
        assert_eq!(
            copy.write_control(u16::from(CRAM_READ_CODE) << 2, REGION),
            Err(VdpPortError::Schedule(VdpScheduleError::Read(
                VdpReadError::UnknownCram(0)
            )))
        );
        assert_eq!(copy, before);

        let write = VdpWrite {
            command: ControlCommand {
                address: 0,
                code: CRAM_WRITE_CODE,
            },
            data: 0x0246,
        };
        let known = crate::hardware::vdp::memory::MemoryTransfer::Known(write, None);
        let unknown = crate::hardware::vdp::memory::MemoryTransfer::UnknownCram(write.command);
        assert_eq!(
            vdp.memory
                .read_after_transfers(read, Some(0), [unknown, known].into_iter()),
            Ok(0x0246)
        );
        assert_eq!(
            vdp.memory
                .read_after_transfers(read, Some(0), [known, unknown].into_iter()),
            Err(VdpReadError::UnknownCram(0))
        );
        let undo = vdp.memory.publish_retirement(known);
        assert_eq!(vdp.memory.cram_word(0), Some(0x0246));
        vdp.memory.undo_transfer(undo);
        assert_eq!(vdp.memory.cram_word(0), None);
        command(&mut vdp, 0, CRAM_WRITE_CODE, REGION);
        vdp.write_data(0x0246, REGION).unwrap();
        let at = next_slot(&vdp, REGION);
        vdp.advance_to(at, REGION).unwrap();
        assert_eq!(vdp.memory.cram_word(0), Some(0x0246));
    }
}

fn cold_fill(increment: u8, display: bool) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    for (index, value) in [
        (
            MODE_REGISTER,
            MODE5_ENABLE | DMA_ENABLE | if display { DISPLAY_ENABLE } else { 0 },
        ),
        (AUTO_INCREMENT_REGISTER, increment),
        (DMA_LENGTH_LOW_REGISTER, 2),
        (DMA_SOURCE_REGISTER, DMA_FILL_MODE),
    ] {
        register(&mut vdp, index, value, REGION);
    }
    if display {
        vdp = Vdp::from_stable_display_state(*vdp.interrupts(), *vdp.registers(), REGION).unwrap();
        for address in [0, 2] {
            vdp.state
                .sprites
                .complete_write(
                    &mut vdp.memory,
                    &vdp.state.registers,
                    VdpWrite {
                        command: ControlCommand {
                            address,
                            code: VRAM_WRITE_CODE,
                        },
                        data: 0,
                    },
                )
                .unwrap();
        }
    }
    assert_eq!(vdp.state.fifo.next_slot_data(), None);
    command(&mut vdp, 126, 0x23, REGION);
    vdp.write_data(0xabcd, REGION).unwrap();
    let first = next_slot(&vdp, REGION);
    vdp.advance_to(first, REGION).unwrap();
    assert_eq!(vdp.memory.cram_word(63), Some(0xabcd & CRAM_DATA_MASK));
    assert_eq!(vdp.state.dma, Some(dma::Phase::FillUnknownCram));
    vdp
}
