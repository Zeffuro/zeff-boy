use crate::hardware::constants::{
    BACKUP_START, BIOS_END, BIOS_START, EWRAM_END, EWRAM_START, GAMEPAK0_END, GAMEPAK0_START,
    GAMEPAK1_END, GAMEPAK1_START, GAMEPAK2_END, GAMEPAK2_START, IO_END, IO_START, IWRAM_END,
    IWRAM_START, OAM_END, OAM_START, PALETTE_RAM_END, PALETTE_RAM_START, SRAM_TIMING_END, VRAM_END,
    VRAM_START,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessType {
    NonSequential,
    Sequential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusRegion {
    Bios,
    Ewram,
    Iwram,
    Io,
    PaletteRam,
    Vram,
    Oam,
    GamePak0,
    GamePak1,
    GamePak2,
    Sram,
    Unused,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DataAccessOrigin {
    Cpu,
    #[cfg(test)]
    Dma {
        channel: u8,
    },
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimerIoRegister {
    CounterReload,
    Control,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimerIoAccessKind {
    Read,
    Write,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimerIoAccessWidth {
    Byte,
    Halfword,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TimerIoCompletionEvent {
    pub origin: DataAccessOrigin,
    pub completion_cycle: u32,
    pub address: u32,
    pub timer: u8,
    pub register: TimerIoRegister,
    pub kind: TimerIoAccessKind,
    pub width: TimerIoAccessWidth,
    pub value: u16,
}

#[cfg(test)]
impl TimerIoCompletionEvent {
    pub(crate) fn new(
        origin: DataAccessOrigin,
        completion_cycle: u32,
        address: u32,
        kind: TimerIoAccessKind,
        width: TimerIoAccessWidth,
        value: u16,
    ) -> Option<Self> {
        let aligned = address & !u32::from(matches!(width, TimerIoAccessWidth::Halfword));
        let offset = aligned.checked_sub(IO_START + 0x100)?;
        if offset > 0x0F {
            return None;
        }

        Some(Self {
            origin,
            completion_cycle,
            address: aligned,
            timer: (offset / 4) as u8,
            register: if offset & 2 == 0 {
                TimerIoRegister::CounterReload
            } else {
                TimerIoRegister::Control
            },
            kind,
            width,
            value,
        })
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DataAccessCursor {
    timeline_origin_cycle: u32,
    elapsed_cycles: u32,
    access_count: u32,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CpuInstructionTimeline {
    pub fetch_cycles: u32,
    pub total_cycles: u32,
    pub data_access_cycles: u32,
    pub data_access_count: u32,
    pub replaced_legacy_data_cycles: u32,
    pub incremental_non_data_cycles: u32,
    pub required_cycles: u32,
}

#[cfg(test)]
impl CpuInstructionTimeline {
    pub(crate) fn completion_events_are_bounded(self, events: &[TimerIoCompletionEvent]) -> bool {
        self.completion_events_fit(events, self.total_cycles)
    }

    pub(crate) fn completion_events_fit_required_timeline(
        self,
        events: &[TimerIoCompletionEvent],
    ) -> bool {
        self.completion_events_fit(events, self.required_cycles)
    }

    fn completion_events_fit(self, events: &[TimerIoCompletionEvent], end_cycle: u32) -> bool {
        if self.fetch_cycles > end_cycle {
            return false;
        }

        let mut previous_cycle = self.fetch_cycles;
        events.iter().all(|event| {
            let bounded = event.origin == DataAccessOrigin::Cpu
                && event.completion_cycle >= previous_cycle
                && event.completion_cycle <= end_cycle;
            previous_cycle = event.completion_cycle;
            bounded
        })
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DataAccessCompletion {
    pub first_completion_cycle: u32,
    pub second_halfword_completion_cycle: Option<u32>,
    pub completion_cycle: u32,
}

#[cfg(test)]
impl DataAccessCursor {
    pub(crate) fn reset(&mut self, timeline_origin_cycle: u32) {
        self.timeline_origin_cycle = timeline_origin_cycle;
        self.elapsed_cycles = 0;
        self.access_count = 0;
    }

    #[cfg(test)]
    pub(crate) fn elapsed_cycles(self) -> u32 {
        self.elapsed_cycles
    }

    pub(crate) fn access_count(self) -> u32 {
        self.access_count
    }

    pub(crate) fn advance(
        &mut self,
        addr: u32,
        width_bytes: u8,
        access: AccessType,
        waitcnt: u16,
    ) -> DataAccessCompletion {
        self.access_count = self.access_count.saturating_add(1);
        let aligned = match width_bytes {
            2 => addr & !1,
            4 => addr & !3,
            _ => addr,
        };
        if width_bytes == WORD_BYTES && word_access_uses_halfwords(aligned) {
            let first_cycles = access_cycles_with_waitcnt(aligned, 2, access, waitcnt);
            let first_completion_cycle = self
                .timeline_origin_cycle
                .saturating_add(self.elapsed_cycles)
                .saturating_add(first_cycles);
            let second_cycles = access_cycles_with_waitcnt(
                aligned.wrapping_add(HALFWORD_BYTES),
                2,
                AccessType::Sequential,
                waitcnt,
            );
            self.elapsed_cycles = self
                .elapsed_cycles
                .saturating_add(first_cycles)
                .saturating_add(second_cycles);
            DataAccessCompletion {
                first_completion_cycle,
                second_halfword_completion_cycle: Some(
                    self.timeline_origin_cycle
                        .saturating_add(self.elapsed_cycles),
                ),
                completion_cycle: self
                    .timeline_origin_cycle
                    .saturating_add(self.elapsed_cycles),
            }
        } else {
            self.elapsed_cycles = self
                .elapsed_cycles
                .saturating_add(access_cycles_with_waitcnt(
                    aligned,
                    width_bytes,
                    access,
                    waitcnt,
                ));
            DataAccessCompletion {
                first_completion_cycle: self
                    .timeline_origin_cycle
                    .saturating_add(self.elapsed_cycles),
                second_halfword_completion_cycle: None,
                completion_cycle: self
                    .timeline_origin_cycle
                    .saturating_add(self.elapsed_cycles),
            }
        }
    }
}

const HALFWORD_BYTES: u32 = 2;
const WORD_BYTES: u8 = 4;
const WAITCNT_MASK_2BIT: u16 = 0x03;
const WAITCNT_MASK_1BIT: u16 = 0x01;
const WAITCNT_SRAM_SHIFT: u16 = 0;
const WAITCNT_GAMEPAK0_FIRST_SHIFT: u16 = 2;
const WAITCNT_GAMEPAK0_SECOND_SHIFT: u16 = 4;
const WAITCNT_GAMEPAK1_FIRST_SHIFT: u16 = 5;
const WAITCNT_GAMEPAK1_SECOND_SHIFT: u16 = 7;
const WAITCNT_GAMEPAK2_FIRST_SHIFT: u16 = 8;
const WAITCNT_GAMEPAK2_SECOND_SHIFT: u16 = 10;
const GAMEPAK0_SECOND_CYCLES: [u32; 2] = [3, 2];
const GAMEPAK1_SECOND_CYCLES: [u32; 2] = [5, 2];
const GAMEPAK2_SECOND_CYCLES: [u32; 2] = [9, 2];
const ACCESS_CYCLE_TABLE: [u32; 4] = [5, 4, 3, 9];

pub fn region_for_addr(addr: u32) -> BusRegion {
    match addr {
        BIOS_START..=BIOS_END => BusRegion::Bios,
        EWRAM_START..=EWRAM_END => BusRegion::Ewram,
        IWRAM_START..=IWRAM_END => BusRegion::Iwram,
        IO_START..=IO_END => BusRegion::Io,
        PALETTE_RAM_START..=PALETTE_RAM_END => BusRegion::PaletteRam,
        VRAM_START..=VRAM_END => BusRegion::Vram,
        OAM_START..=OAM_END => BusRegion::Oam,
        GAMEPAK0_START..=GAMEPAK0_END => BusRegion::GamePak0,
        GAMEPAK1_START..=GAMEPAK1_END => BusRegion::GamePak1,
        GAMEPAK2_START..=GAMEPAK2_END => BusRegion::GamePak2,
        BACKUP_START..=SRAM_TIMING_END => BusRegion::Sram,
        _ => BusRegion::Unused,
    }
}

pub fn access_cycles(addr: u32, width_bytes: u8, access: AccessType) -> u32 {
    access_cycles_with_waitcnt(addr, width_bytes, access, 0)
}

pub fn access_cycles_with_waitcnt(
    addr: u32,
    width_bytes: u8,
    access: AccessType,
    waitcnt: u16,
) -> u32 {
    let region = region_for_addr(addr);
    let base = match (region, access) {
        (BusRegion::Bios, _) => 1,
        (BusRegion::Ewram, _) => 3,
        (BusRegion::Iwram, _) => 1,
        (BusRegion::Io, _) => 1,
        (BusRegion::PaletteRam | BusRegion::Vram | BusRegion::Oam, _) => 1,
        (BusRegion::GamePak0, AccessType::NonSequential) => {
            gamepak_first_access_cycles(waitcnt, WAITCNT_GAMEPAK0_FIRST_SHIFT)
        }
        (BusRegion::GamePak0, AccessType::Sequential) => gamepak_second_access_cycles(
            waitcnt,
            WAITCNT_GAMEPAK0_SECOND_SHIFT,
            GAMEPAK0_SECOND_CYCLES,
        ),
        (BusRegion::GamePak1, AccessType::NonSequential) => {
            gamepak_first_access_cycles(waitcnt, WAITCNT_GAMEPAK1_FIRST_SHIFT)
        }
        (BusRegion::GamePak1, AccessType::Sequential) => gamepak_second_access_cycles(
            waitcnt,
            WAITCNT_GAMEPAK1_SECOND_SHIFT,
            GAMEPAK1_SECOND_CYCLES,
        ),
        (BusRegion::GamePak2, AccessType::NonSequential) => {
            gamepak_first_access_cycles(waitcnt, WAITCNT_GAMEPAK2_FIRST_SHIFT)
        }
        (BusRegion::GamePak2, AccessType::Sequential) => gamepak_second_access_cycles(
            waitcnt,
            WAITCNT_GAMEPAK2_SECOND_SHIFT,
            GAMEPAK2_SECOND_CYCLES,
        ),
        (BusRegion::Sram, _) => sram_access_cycles(waitcnt),
        (BusRegion::Unused, _) => 1,
    };

    if width_bytes >= WORD_BYTES
        && matches!(
            region,
            BusRegion::GamePak0 | BusRegion::GamePak1 | BusRegion::GamePak2
        )
    {
        let second_halfword = if addr & 0x01FF_FFFF <= 0x01FF_FFFD {
            match region {
                BusRegion::GamePak0 => gamepak_second_access_cycles(
                    waitcnt,
                    WAITCNT_GAMEPAK0_SECOND_SHIFT,
                    GAMEPAK0_SECOND_CYCLES,
                ),
                BusRegion::GamePak1 => gamepak_second_access_cycles(
                    waitcnt,
                    WAITCNT_GAMEPAK1_SECOND_SHIFT,
                    GAMEPAK1_SECOND_CYCLES,
                ),
                BusRegion::GamePak2 => gamepak_second_access_cycles(
                    waitcnt,
                    WAITCNT_GAMEPAK2_SECOND_SHIFT,
                    GAMEPAK2_SECOND_CYCLES,
                ),
                _ => unreachable!(),
            }
        } else {
            sequential_cycles_with_waitcnt(addr + HALFWORD_BYTES, waitcnt)
        };
        base + second_halfword
    } else {
        base
    }
}

pub fn instruction_fetch_cycles(addr: u32, width_bytes: u8, sequential: bool) -> u32 {
    instruction_fetch_cycles_with_waitcnt(addr, width_bytes, sequential, 0)
}

pub fn instruction_fetch_cycles_with_waitcnt(
    addr: u32,
    width_bytes: u8,
    sequential: bool,
    waitcnt: u16,
) -> u32 {
    access_cycles_with_waitcnt(
        addr,
        width_bytes,
        if sequential {
            AccessType::Sequential
        } else {
            AccessType::NonSequential
        },
        waitcnt,
    )
}

fn sequential_cycles_with_waitcnt(addr: u32, waitcnt: u16) -> u32 {
    access_cycles_with_waitcnt(addr, HALFWORD_BYTES as u8, AccessType::Sequential, waitcnt)
}

fn gamepak_first_access_cycles(waitcnt: u16, shift: u16) -> u32 {
    ACCESS_CYCLE_TABLE[((waitcnt >> shift) & WAITCNT_MASK_2BIT) as usize]
}

fn gamepak_second_access_cycles(waitcnt: u16, shift: u16, cycles: [u32; 2]) -> u32 {
    cycles[((waitcnt >> shift) & WAITCNT_MASK_1BIT) as usize]
}

fn sram_access_cycles(waitcnt: u16) -> u32 {
    ACCESS_CYCLE_TABLE[((waitcnt >> WAITCNT_SRAM_SHIFT) & WAITCNT_MASK_2BIT) as usize]
}

#[cfg(test)]
fn word_access_uses_halfwords(addr: u32) -> bool {
    matches!(
        region_for_addr(addr),
        BusRegion::Ewram
            | BusRegion::PaletteRam
            | BusRegion::Vram
            | BusRegion::GamePak0
            | BusRegion::GamePak1
            | BusRegion::GamePak2
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_regions() {
        assert_eq!(region_for_addr(0x0200_0000), BusRegion::Ewram);
        assert_eq!(region_for_addr(0x0300_0000), BusRegion::Iwram);
        assert_eq!(region_for_addr(0x0800_0000), BusRegion::GamePak0);
        assert_eq!(region_for_addr(0x0E00_0000), BusRegion::Sram);
    }

    #[test]
    fn gamepak_sequential_fetch_is_faster_than_nonsequential() {
        assert!(
            instruction_fetch_cycles(0x0800_0000, 2, true)
                < instruction_fetch_cycles(0x0800_0000, 2, false)
        );
    }

    #[test]
    fn waitcnt_controls_gamepak0_waitstates() {
        let waitcnt = 0b10 << 2 | 1 << 4;
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0800_0000, 2, false, waitcnt),
            3
        );
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0800_0002, 2, true, waitcnt),
            2
        );
    }

    #[test]
    fn waitcnt_controls_gamepak_mirror_waitstates() {
        let waitcnt = (0b01 << 5) | (1 << 7) | (0b11 << 8);
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0A00_0000, 2, false, waitcnt),
            4
        );
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0A00_0002, 2, true, waitcnt),
            2
        );
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0C00_0000, 2, false, waitcnt),
            9
        );
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0C00_0002, 2, true, waitcnt),
            9
        );
    }

    #[test]
    fn word_gamepak_access_adds_sequential_halfword() {
        let waitcnt = (0b10 << 2) | (1 << 4);
        assert_eq!(
            access_cycles_with_waitcnt(0x0800_0000, 4, AccessType::NonSequential, waitcnt),
            5
        );
    }

    #[test]
    fn word_gamepak_access_preserves_waitstate_window_crossing() {
        assert_eq!(
            access_cycles_with_waitcnt(
                0x09FF_FFFE,
                4,
                AccessType::NonSequential,
                1 << WAITCNT_GAMEPAK1_SECOND_SHIFT,
            ),
            7
        );
    }

    #[test]
    fn data_cursor_uses_region_bus_width_for_word_accesses() {
        let mut cursor = DataAccessCursor::default();
        cursor.reset(0);
        let ewram = cursor.advance(0x0200_0000, 4, AccessType::NonSequential, 0);
        let io = cursor.advance(0x0400_0100, 4, AccessType::NonSequential, 0);
        let palette = cursor.advance(0x0500_0000, 4, AccessType::NonSequential, 0);
        let vram = cursor.advance(0x0600_0000, 4, AccessType::NonSequential, 0);
        let oam = cursor.advance(0x0700_0000, 4, AccessType::NonSequential, 0);

        assert_eq!(ewram.first_completion_cycle, 3);
        assert_eq!(ewram.second_halfword_completion_cycle, Some(6));
        assert_eq!(io.first_completion_cycle, 7);
        assert_eq!(io.second_halfword_completion_cycle, None);
        assert_eq!(palette.first_completion_cycle, 8);
        assert_eq!(palette.second_halfword_completion_cycle, Some(9));
        assert_eq!(vram.first_completion_cycle, 10);
        assert_eq!(vram.second_halfword_completion_cycle, Some(11));
        assert_eq!(oam.first_completion_cycle, 12);
        assert_eq!(oam.second_halfword_completion_cycle, None);
        assert_eq!(cursor.elapsed_cycles(), 12);
    }

    #[test]
    fn data_cursor_keeps_io_words_on_single_cycle_transactions() {
        let mut cursor = DataAccessCursor::default();
        cursor.reset(8);

        let first = cursor.advance(0x0400_00FC, 4, AccessType::NonSequential, 0);
        let second = cursor.advance(0x0400_0100, 4, AccessType::Sequential, 0);

        assert_eq!(first.first_completion_cycle, 9);
        assert_eq!(first.second_halfword_completion_cycle, None);
        assert_eq!(second.first_completion_cycle, 10);
        assert_eq!(second.second_halfword_completion_cycle, None);
        assert_eq!(cursor.elapsed_cycles(), 2);
    }

    #[test]
    fn data_cursor_applies_waitcnt_to_each_gamepak_halfword() {
        let waitcnt = (0b10 << 2) | (1 << 4);
        let mut cursor = DataAccessCursor::default();
        cursor.reset(0);
        let first = cursor.advance(0x0800_0000, 4, AccessType::NonSequential, waitcnt);
        let second = cursor.advance(0x0800_0004, 4, AccessType::Sequential, waitcnt);

        assert_eq!(first.first_completion_cycle, 3);
        assert_eq!(first.second_halfword_completion_cycle, Some(5));
        assert_eq!(second.first_completion_cycle, 7);
        assert_eq!(second.second_halfword_completion_cycle, Some(9));
        assert_eq!(cursor.elapsed_cycles(), 9);
    }

    #[test]
    fn data_cursor_offsets_completions_from_instruction_fetch() {
        let mut cursor = DataAccessCursor::default();
        cursor.reset(8);

        let access = cursor.advance(0x0400_0100, 2, AccessType::NonSequential, 0);

        assert_eq!(access.first_completion_cycle, 9);
        assert_eq!(access.completion_cycle, 9);
        assert_eq!(cursor.elapsed_cycles(), 1);
    }

    #[test]
    fn instruction_timeline_rejects_late_or_reordered_completions() {
        let timeline = CpuInstructionTimeline {
            fetch_cycles: 8,
            total_cycles: 10,
            data_access_cycles: 2,
            data_access_count: 2,
            replaced_legacy_data_cycles: 2,
            incremental_non_data_cycles: 0,
            required_cycles: 10,
        };
        let event = |completion_cycle| TimerIoCompletionEvent {
            origin: DataAccessOrigin::Cpu,
            completion_cycle,
            address: 0x0400_0100,
            timer: 0,
            register: TimerIoRegister::CounterReload,
            kind: TimerIoAccessKind::Read,
            width: TimerIoAccessWidth::Halfword,
            value: 0,
        };

        assert!(timeline.completion_events_are_bounded(&[event(9), event(10)]));
        assert!(!timeline.completion_events_are_bounded(&[event(10), event(9)]));
        assert!(!timeline.completion_events_are_bounded(&[event(11)]));
    }

    #[test]
    fn timer_io_events_keep_register_lane_and_origin() {
        assert_eq!(
            TimerIoCompletionEvent::new(
                DataAccessOrigin::Dma { channel: 2 },
                17,
                0x0400_0107,
                TimerIoAccessKind::Write,
                TimerIoAccessWidth::Byte,
                0x5A,
            ),
            Some(TimerIoCompletionEvent {
                origin: DataAccessOrigin::Dma { channel: 2 },
                completion_cycle: 17,
                address: 0x0400_0107,
                timer: 1,
                register: TimerIoRegister::Control,
                kind: TimerIoAccessKind::Write,
                width: TimerIoAccessWidth::Byte,
                value: 0x5A,
            })
        );
        assert!(
            TimerIoCompletionEvent::new(
                DataAccessOrigin::Cpu,
                1,
                0x0400_0110,
                TimerIoAccessKind::Read,
                TimerIoAccessWidth::Halfword,
                0,
            )
            .is_none()
        );
    }
}
