use super::*;

const REGION: Region = Region::NtscUs;
const TIE: u64 = V28_VBLANK_SET_LINE * H32_LINE_TICKS + H32_DOT_TICKS;

fn fixture(kind: u8, length: u8, enable: u8) -> Vdp {
    let mut registers = VdpRegisterPipeline::at_h32_edge(MasterTicks::ZERO);
    for index in 0..REGISTER_COUNT {
        let value = match index {
            MODE_REGISTER => MODE5_ENABLE | enable,
            HINT_ENABLE_REGISTER => HINT_ENABLE_MASK,
            AUTO_INCREMENT_REGISTER => 1,
            DMA_LENGTH_LOW_REGISTER => length,
            DMA_SOURCE_REGISTER => {
                if kind == 0 {
                    DMA_FILL_MODE
                } else {
                    DMA_COPY_MODE
                }
            }
            _ => 0,
        };
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    let at = tick(
        TIE - if kind == 2 {
            H32_MEMORY_SLOT_TICKS + 1
        } else {
            1
        },
    );
    registers.advance_to(at).unwrap();
    let mut interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
    while interrupts
        .advance_in_mode(at, REGION, &registers, false)
        .unwrap()
        .is_some()
    {}
    let mut device = Vdp::from_known_state(interrupts, registers, REGION).unwrap();
    device
        .memory
        .write_word(
            ControlCommand {
                address: 0,
                code: VRAM_WRITE_CODE,
            },
            0x1234,
        )
        .unwrap();
    device
        .resume_dma_from_known_phase(
            ControlCommand {
                address: 0x2000,
                code: if kind == 0 { 0x21 } else { 0x30 },
            },
            if kind == 0 { Some(0xabcd) } else { None },
            REGION,
        )
        .unwrap();
    if kind == 2 {
        device.advance_to(tick(TIE - 1), REGION).unwrap();
    }
    device
}

pub fn operation(device: &mut Vdp, interrupt_first: bool) -> Result<(), VdpScheduleError> {
    let mut state = device.state.clone();
    let mut writes = [None; WRITE_BATCH_CAPACITY];
    if interrupt_first {
        state.advance_to(tick(TIE), REGION)?;
    }
    writes[0] = state.service_dma(&device.memory, &writes)?;
    if !interrupt_first {
        state.advance_to(tick(TIE), REGION)?;
    }
    device.publish(state, writes);
    Ok(())
}

pub fn commute_with(mut run: impl FnMut(&mut Vdp, bool) -> Result<(), VdpScheduleError>) {
    for kind in 0..3 {
        for length in [1, 2] {
            for enable in [0, VINT_ENABLE_MASK] {
                let mut first = fixture(kind, length, enable);
                let mut last = first.clone();
                let pointer = first.memory().vram().as_ptr();
                run(&mut first, true).unwrap();
                run(&mut last, false).unwrap();
                assert_eq!(first, last);
                assert!(first.interrupts().vertical_pending());
                assert!(first.interrupts().horizontal_pending());
                assert_eq!(first.now(), tick(TIE));
                assert_eq!(first.registers().now(), first.now());
                assert_eq!(first.memory().vram().as_ptr(), pointer);
                assert_eq!(
                    first.memory().vram()[0x2001],
                    match kind {
                        0 => 0xab,
                        1 => 0,
                        _ => 0x34,
                    }
                );
                assert_eq!(
                    first.registers().registers()[usize::from(DMA_LENGTH_LOW_REGISTER)],
                    Some(length - u8::from(kind != 1))
                );
            }
        }
    }
}

#[test]
fn commute() {
    commute_with(operation);
}

pub fn non_dma_guards_with(
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
) {
    for read in [false, true] {
        let mut device = fixture(0, 2, VINT_ENABLE_MASK);
        device.state.dma = None;
        if read {
            device.state.command = ControlCommand {
                address: 0,
                code: VRAM_READ_CODE,
            };
            device.state.read_phase = Some(false);
        } else {
            device
                .state
                .fifo
                .push(VdpWrite {
                    command: ControlCommand {
                        address: 0x2000,
                        code: VRAM_WRITE_CODE,
                    },
                    data: 0xabcd,
                })
                .unwrap();
        }
        let pointer = device.memory().vram().as_ptr();
        advance(&mut device, tick(TIE), REGION).unwrap();
        assert!(device.interrupts().vertical_pending());
        if read {
            assert!(!device.read_cache().is_ready());
        } else {
            assert_eq!(device.fifo().len(), 1);
            assert_eq!(device.memory().vram()[0x2001], 0xcd);
        }
        advance(&mut device, tick(TIE + 20), REGION).unwrap();
        if read {
            assert!(device.read_cache().is_ready());
        } else {
            assert!(device.fifo().is_empty());
            assert_eq!(&device.memory().vram()[0x2000..0x2002], &[0xab, 0xcd]);
        }
        assert_eq!(device.memory().vram().as_ptr(), pointer);
    }
}

#[test]
fn non_dma_guards() {
    non_dma_guards_with(Vdp::advance_to);
}
