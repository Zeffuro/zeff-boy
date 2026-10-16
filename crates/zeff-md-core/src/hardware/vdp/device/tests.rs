use super::*;
use crate::hardware::vdp::{VdpWrite, VdpWriteError};

#[path = "dma_irq_tests.rs"]
mod dma_irq_tests;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn pending(epoch: u64, at: u64) -> Vdp {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(epoch));
    for (index, value) in [(0, 0), (1, 0x44), (12, 0), (10, 0xff), (15, 2)] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    registers.advance_to(tick(at)).unwrap();
    let mut interrupts = VdpInterrupts::at_counters_zero(tick(epoch), 0xff, false, false);
    while interrupts
        .advance_to(tick(at), Region::NtscUs, &registers)
        .unwrap()
        .is_some()
    {}
    let mut device = Vdp::from_stable_display_state(interrupts, registers, Region::NtscUs).unwrap();
    // Bypass admission to qualify pending-work guards independently of preflight.
    device
        .state
        .fifo
        .push(VdpWrite {
            command: ControlCommand {
                address: 0,
                code: 1,
            },
            data: 0x1234,
        })
        .unwrap();
    device
}

pub fn future_margin_with(
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
) {
    let mut device = pending(0, 758991);
    advance(&mut device, tick(759239), Region::NtscUs).unwrap();
    assert_eq!(device.fifo().len(), 1);
    assert_eq!(&device.memory().vram()[..2], &[0; 2]);
    for (at, bytes, remaining) in [
        (759240, [0, 0], 1),
        (759289, [0, 0], 1),
        (759290, [0, 0x34], 1),
        (769510, [0x12, 0x34], 0),
        (896090, [0x12, 0x34], 0),
    ] {
        advance(&mut device, tick(at), Region::NtscUs).unwrap();
        assert_eq!(device.now(), tick(at));
        assert_eq!(&device.memory().vram()[..2], &bytes);
        assert_eq!(device.fifo().len(), remaining);
    }
}

pub fn overflow_horizon_with(
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
) {
    let mut device = pending(u64::MAX - 195, u64::MAX - 1);
    advance(&mut device, tick(u64::MAX), Region::NtscUs).unwrap();
    assert_eq!(device.now(), tick(u64::MAX));
    assert_eq!(device.fifo().len(), 1);
    assert_eq!(&device.memory().vram()[..2], &[0; 2]);
}

#[test]
fn future_margin() {
    future_margin_with(Vdp::advance_to);
}
#[test]
fn overflow_horizon() {
    overflow_horizon_with(Vdp::advance_to);
}

#[test]
fn unused_targets_retire_without_memory_writes_and_retain_fifo_data() {
    for target in [2, 6, 7, 9, 0xa, 0xb, 0xd, 0xe, 0xf] {
        for code in [target, target | COMMAND_CD4] {
            let mut device = Vdp::bootstrap_mode5();
            device.advance_to(tick(1), Region::NtscUs).unwrap();
            device
                .state
                .registers
                .queue_register(AUTO_INCREMENT_REGISTER, 2)
                .unwrap();
            device.advance_to(tick(21), Region::NtscUs).unwrap();
            device.state.command = ControlCommand {
                address: 0x1fffd,
                code,
            };
            let memory = device.memory().clone();
            for data in [0x1111, 0x2222, 0x3333, 0x4444] {
                device.write_data(data, Region::NtscUs).unwrap();
            }
            assert_eq!(device.command().address, 5);
            assert_eq!(device.fifo().front().unwrap().command.address, 0x1fffd);
            let saved = device.clone();
            assert_eq!(
                device.write_data(0xffff, Region::NtscUs),
                Err(VdpPortError::Data(VdpWriteError::Full))
            );
            assert_eq!(device, saved);
            assert_eq!(device.data_write_wait(Region::NtscUs), Ok(Some(tick(30))));
            let mut split = device.clone();
            device.advance_to(tick(90), Region::NtscUs).unwrap();
            for (at, remaining) in [(30, 3), (50, 2), (70, 1), (90, 0)] {
                split.advance_to(tick(at), Region::NtscUs).unwrap();
                assert_eq!(split.fifo().len(), remaining);
                assert_eq!(split.memory(), &memory);
            }
            assert_eq!(device, split);
            assert_eq!(device.fifo().next_slot_data(), Some(0x1111));
            assert_eq!(
                device.sprite_cache().unwrap().entry_words(0).unwrap(),
                [None, None]
            );
            device.write_control(0, Region::NtscUs).unwrap();
            device.write_control(0x20, Region::NtscUs).unwrap();
            device.advance_to(tick(110), Region::NtscUs).unwrap();
            assert_eq!(device.read_data(Region::NtscUs), Ok(0x1111));
        }
    }
}

#[test]
fn idle_advancement_matches_prepared_transactions() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for fill_waiting in [false, true] {
            let mut device = Vdp::bootstrap_mode5();
            device.state.dma = fill_waiting.then_some(dma::Phase::FillWaiting);
            if !fill_waiting {
                device.write_control(0x8010, region).unwrap();
            }
            let mut prepared = device.clone();
            for at in [0, 1, 19, 3419, 3420, 759240, 896040, 1067040, 0] {
                let at = tick(at);
                let expected = prepared
                    .state
                    .prepare_to_from(at, region, false, &prepared.memory, None)
                    .map(|(state, writes, _)| prepared.publish(state, writes));
                assert_eq!(device.advance_to(at, region), expected);
                assert_eq!(device, prepared);
            }
        }
    }
}
