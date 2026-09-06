use super::{Bus, Cpu, FetchedInstruction, InstructionSet, decode};
use crate::hardware::cartridge::BackupKind;
use crate::hardware::constants::{
    EWRAM_END, EWRAM_SIZE, EWRAM_START, IWRAM_END, IWRAM_SIZE, IWRAM_START,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PrefetchedInstruction {
    pub(super) pc: u32,
    pub(super) raw: u32,
}

impl PrefetchedInstruction {
    #[inline]
    pub(super) fn decode(
        self,
        instruction_set: InstructionSet,
        fetch_cycles: u32,
    ) -> FetchedInstruction {
        FetchedInstruction {
            pc: self.pc,
            raw: self.raw,
            instruction_set,
            width_bytes: instruction_set.width_bytes(),
            fetch_cycles,
            decoded: decode::decode_stub(self.raw, instruction_set),
        }
    }
}

impl From<FetchedInstruction> for PrefetchedInstruction {
    fn from(fetched: FetchedInstruction) -> Self {
        Self {
            pc: fetched.pc,
            raw: fetched.raw,
        }
    }
}

pub(super) const EMPTY_PREFETCHED_INSTRUCTION: PrefetchedInstruction =
    PrefetchedInstruction { pc: 0, raw: 0 };

#[derive(Clone, Copy)]
enum FetchMemory {
    Ewram,
    Iwram,
    GamePak,
}

#[derive(Clone, Copy)]
pub(super) struct FrameFetchWindow {
    memory: FetchMemory,
    start_address: u32,
    last_fetch_address: u32,
    instruction_set: InstructionSet,
    width: u8,
}

impl FrameFetchWindow {
    pub(super) fn new(
        bus: &Bus,
        pc: u32,
        instruction_set: InstructionSet,
        _cycles: u32,
    ) -> Option<Self> {
        if bus.cartridge.has_rtc() || bus.cartridge.backup_kind() == BackupKind::Eeprom {
            return None;
        }
        let (memory, start_address, end) = match pc {
            EWRAM_START..=EWRAM_END => (FetchMemory::Ewram, EWRAM_START, EWRAM_END + 1),
            IWRAM_START..=IWRAM_END => (FetchMemory::Iwram, IWRAM_START, IWRAM_END + 1),
            0x0800_0000..=0x0DFF_FFFF => {
                let base = pc & !0x01FF_FFFF;
                (
                    FetchMemory::GamePak,
                    base,
                    base + bus.cartridge.rom().len().min(0x0200_0000) as u32,
                )
            }
            _ => return None,
        };
        let width = instruction_set.width_bytes();
        let last_fetch_address = end.checked_sub(u32::from(width))?;
        if pc.checked_add(2 * u32::from(width))? > last_fetch_address {
            return None;
        }
        Some(Self {
            memory,
            start_address,
            last_fetch_address,
            instruction_set,
            width,
        })
    }

    pub(super) fn contains(self, address: u32) -> bool {
        (self.start_address..=self.last_fetch_address).contains(&address)
    }

    fn fetch(self, bus: &Bus, pc: u32) -> PrefetchedInstruction {
        let (memory, offset) = match self.memory {
            FetchMemory::Ewram => (bus.ewram.as_slice(), pc as usize & (EWRAM_SIZE - 1)),
            FetchMemory::Iwram => (bus.iwram.as_slice(), pc as usize & (IWRAM_SIZE - 1)),
            FetchMemory::GamePak => (bus.cartridge.rom(), (pc - self.start_address) as usize),
        };
        let bytes = &memory[offset..offset + usize::from(self.width)];
        let raw = match self.instruction_set {
            InstructionSet::Arm => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            InstructionSet::Thumb => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        };
        PrefetchedInstruction { pc, raw }
    }
}

impl Cpu {
    pub(super) fn fetch_frame_direct(
        &mut self,
        bus: &Bus,
        window: FrameFetchWindow,
        lookahead: u32,
    ) -> PrefetchedInstruction {
        #[cfg(feature = "profiling")]
        self.profile_instruction_fetch(bus, lookahead, window.instruction_set, window.width, true);
        let next = window.fetch(bus, lookahead);
        #[cfg(test)]
        match window.memory {
            FetchMemory::Ewram => self.ram_block_fetches[0] += 1,
            FetchMemory::Iwram => self.ram_block_fetches[1] += 1,
            FetchMemory::GamePak => self.gamepak_block_fetches += 1,
        }
        #[cfg(feature = "profiling")]
        {
            let counter = match window.memory {
                FetchMemory::Ewram => &mut self.profiling.ram_block_fetches[0],
                FetchMemory::Iwram => &mut self.profiling.ram_block_fetches[1],
                FetchMemory::GamePak => &mut self.profiling.gamepak_block_fetches,
            };
            *counter = counter.wrapping_add(1);
        }
        next
    }
}
