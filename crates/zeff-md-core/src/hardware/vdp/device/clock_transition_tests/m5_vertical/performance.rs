use super::*;
use crate::hardware::vdp::timing::blank_mode::{BlankHistory, PortKind};
use crate::hardware::vdp::{VdpInterruptEdge, VdpTimingError};

fn literal(
    mut history: BlankHistory,
    region: Region,
    mut registers: VdpRegisterPipeline,
) -> Result<VdpInterruptEdge, VdpTimingError> {
    history = history.advance_projection(history.now, region, &mut registers)?;
    loop {
        let next = history.next_event(&registers)?;
        if history.port.is_some_and(|p| next > p.end) {
            return Err(VdpTimingError::ClockOverflow);
        }
        let before = history.core;
        history = history.advance_projection(next, region, &mut registers)?;
        let vertical = history.core.h && history.core.q && !(before.h && before.q);
        if history.core.v != before.v || vertical {
            return Ok(VdpInterruptEdge {
                at: next,
                clock: if vertical {
                    VdpInterruptClock::Vertical
                } else {
                    VdpInterruptClock::HorizontalCounter
                },
            });
        }
    }
}

pub(super) fn checks() {
    let region = Region::NtscUs;
    for wide in [false, true] {
        let mut vdp = seed(region);
        if wide {
            main_word(&mut vdp, region, 0x8c01);
        }
        let boundary = vdp.next_row_deadline(region).unwrap().unwrap();
        vdp.advance_to(tick(boundary.get() + if wide { 80 } else { 100 }), region)
            .unwrap();
        let original = vdp.state.interrupts.blank_history().unwrap();
        let registers = *vdp.registers();
        let mut folded = original;
        let mut fold_registers = registers;
        assert_eq!(
            folded.fold_interrupt_forecast(region, &mut fold_registers),
            Some(())
        );
        assert!(folded.now.get() > original.now.get() + 1000);
        let mut expected_registers = registers;
        let expected = original
            .advance_projection(folded.now, region, &mut expected_registers)
            .unwrap();
        assert_eq!(folded, expected);
        assert_eq!(fold_registers, expected_registers);
        let mut writer = StateWriter::new();
        folded.write_state(&mut writer);
        let mut expected_writer = StateWriter::new();
        expected.write_state(&mut expected_writer);
        assert_eq!(writer.into_bytes(), expected_writer.into_bytes());
        for history in [original, folded] {
            let regs = if history.now == original.now {
                registers
            } else {
                fold_registers
            };
            assert_eq!(
                history.next_interrupt(history.now, region, &regs),
                literal(history, region, regs)
            );
        }
        let edge = literal(original, region, registers).unwrap();
        let mut regs = registers;
        let dot = original.vertical.unwrap().dot();
        let mut overflow = original
            .advance_projection(tick(edge.at.get() + 4 * dot), region, &mut regs)
            .unwrap();
        overflow.core.wrap = true;
        overflow.core.field_id = u64::MAX;
        assert_eq!(
            literal(overflow, region, regs),
            Err(VdpTimingError::ClockOverflow)
        );
        assert_eq!(
            overflow.next_interrupt(overflow.now, region, &regs),
            Err(VdpTimingError::ClockOverflow)
        );
        for variant in 0..3 {
            let mut declined = original;
            let mut regs = registers;
            match variant {
                0 => {
                    regs.queue_control_register(10, 1).unwrap();
                }
                1 => {
                    declined
                        .begin_port(
                            declined.now,
                            tick(declined.now.get() + 28),
                            PortKind::Status,
                        )
                        .unwrap();
                }
                _ => {
                    declined.begin_data(declined.now, true).unwrap();
                }
            }
            let before = declined;
            let before_registers = regs;
            assert_eq!(declined.fold_interrupt_forecast(region, &mut regs), None);
            assert_eq!(declined, before);
            assert_eq!(regs, before_registers);
        }
    }
}
