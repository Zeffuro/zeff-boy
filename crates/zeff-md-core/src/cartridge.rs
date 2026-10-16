use crate::constants::{MAX_CARTRIDGE_SIZE, SHORT_FRAME_BYTES, WORK_RAM_START};
use crate::emulator::Region;
use std::sync::Arc;
use zeff_m68k::Registers;
use zeff_m68k::bus::BUS_ADDRESS_MASK;

mod header;
pub(crate) mod ram;
pub use header::CartridgeHeader;
pub(crate) use ram::SaveMemory;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    UnsupportedRegion(Region),
    CartridgeSize(usize),
    ResetProgramCounter(u32),
    ResetStackPointer(u32),
    UnsupportedSaveHardware,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cartridge {
    pub bytes: Arc<[u8]>,
    pub initial: Registers,
    pub header: Option<Arc<CartridgeHeader>>,
    pub save: Option<SaveMemory>,
}

impl Cartridge {
    pub fn new(bytes: Vec<u8>) -> Result<Self, LoadError> {
        if !(8..=MAX_CARTRIDGE_SIZE).contains(&bytes.len()) {
            return Err(LoadError::CartridgeSize(bytes.len()));
        }
        let sp = u32::from_be_bytes(bytes[..4].try_into().unwrap());
        let pc = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
        let address = (pc & BUS_ADDRESS_MASK) as usize;
        if pc & 1 != 0 || address + 3 >= bytes.len() {
            return Err(LoadError::ResetProgramCounter(pc));
        }
        if sp & 1 != 0
            || [sp.wrapping_sub(2), sp.wrapping_sub(SHORT_FRAME_BYTES)]
                .into_iter()
                .any(|address| address & BUS_ADDRESS_MASK < WORK_RAM_START)
        {
            return Err(LoadError::ResetStackPointer(sp));
        }
        let mut initial = Registers::new();
        initial.ssp = sp;
        initial.pc = pc;
        let header = CartridgeHeader::parse(&bytes).map(Arc::new);
        let save = SaveMemory::from_rom(&bytes)?;
        Ok(Self {
            header,
            save,
            bytes: bytes.into(),
            initial,
        })
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        if let Some(save) = &self.save
            && save.contains(address)
        {
            return save.read(address);
        }
        Some(
            self.bytes
                .get(address as usize)
                .copied()
                .unwrap_or(crate::constants::UNPOPULATED_ROM_BYTE),
        )
    }
}
