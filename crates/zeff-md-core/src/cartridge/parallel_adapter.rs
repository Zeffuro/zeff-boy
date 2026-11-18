use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{fmt, sync::Arc};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, FunctionCode};

const RAM_BYTES: usize = 0x10000;
const STATE_TAG: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Model {
    First,
    Second,
}

const IDENTITIES: [(usize, [u8; 32], Model); 3] = [
    (
        0x20000,
        [
            0x79, 0x71, 0x1f, 0xa5, 0x5e, 0x8a, 0x4e, 0xa6, 0x62, 0x01, 0xcf, 0xa1, 0x0e, 0x01,
            0xe4, 0xe2, 0x22, 0x38, 0x08, 0x21, 0xbd, 0x10, 0xb0, 0x3c, 0x66, 0x0a, 0x12, 0x87,
            0xce, 0x5c, 0x64, 0xc3,
        ],
        Model::First,
    ),
    (
        0x10000,
        [
            0xbf, 0x12, 0x28, 0x67, 0x89, 0x4f, 0x95, 0x78, 0x96, 0x79, 0xdf, 0x49, 0x23, 0xc8,
            0x07, 0xc8, 0x86, 0xd6, 0x09, 0x5e, 0x34, 0xa0, 0x30, 0xd2, 0x49, 0x3e, 0x19, 0xc9,
            0x03, 0x0d, 0x16, 0x4b,
        ],
        Model::First,
    ),
    (
        0x10000,
        [
            0x52, 0xe6, 0x00, 0x77, 0x49, 0x7e, 0xcd, 0x79, 0x2a, 0x0c, 0xe0, 0x88, 0x83, 0x12,
            0x07, 0xca, 0x75, 0x22, 0x54, 0xbf, 0xfc, 0x14, 0x99, 0xed, 0x09, 0x49, 0xa0, 0x2a,
            0xb0, 0x83, 0x75, 0xbe,
        ],
        Model::Second,
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WriteRejection {
    GuardedAddress,
    LowerLane,
    UnalignedAddress,
}

#[derive(Clone)]
pub(crate) struct ParallelAdapter {
    model: Model,
    ram: Arc<[u8]>,
}

impl PartialEq for ParallelAdapter {
    fn eq(&self, other: &Self) -> bool {
        self.model == other.model && (Arc::ptr_eq(&self.ram, &other.ram) || self.ram == other.ram)
    }
}

impl Eq for ParallelAdapter {}

impl fmt::Debug for ParallelAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParallelAdapter")
            .field("model", &self.model)
            .field("ram_bytes", &self.ram.len())
            .finish()
    }
}

impl ParallelAdapter {
    pub fn detect(rom: &[u8]) -> Option<Model> {
        if !Self::identity_geometry(rom) {
            return None;
        }
        Self::matches_identity(rom.len(), Sha256::digest(rom).into())
    }

    pub(super) fn identity_geometry(rom: &[u8]) -> bool {
        IDENTITIES.iter().any(|(length, _, _)| *length == rom.len())
            && matches!(rom.get(..8), Some([0, 0x42 | 0x60, 5, 0xdc, 0, 0, 2, 0]))
    }

    pub(crate) fn cold_prefetch_valid(&self, pc: u32, source_bytes: usize) -> bool {
        let address = pc & BUS_ADDRESS_MASK;
        pc & 1 == 0
            && (0..4).all(|offset| {
                let byte = address + offset;
                (byte as usize) < source_bytes && !self.guarded(byte) && !self.contains(byte)
            })
    }

    pub fn matches_identity(length: usize, digest: [u8; 32]) -> Option<Model> {
        IDENTITIES.iter().find_map(|(bytes, expected, model)| {
            (*bytes == length && *expected == digest).then_some(*model)
        })
    }

    pub fn new(model: Model) -> Self {
        // Zero fill is a deterministic cold-start policy, not a physical RAM value.
        Self {
            model,
            ram: vec![0; RAM_BYTES].into(),
        }
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn power_on(&mut self) {
        *self = Self::new(self.model);
    }

    fn ram_start(&self) -> u32 {
        match self.model {
            Model::First => 0x420000,
            Model::Second => 0x600000,
        }
    }

    pub fn contains(&self, address: u32) -> bool {
        (self.ram_start()..self.ram_start() + RAM_BYTES as u32).contains(&address)
    }

    pub fn guarded(&self, address: u32) -> bool {
        match self.model {
            // Retain the previously guarded aperture until its reads are qualified.
            Model::First => {
                (0x010000..=0x010019).contains(&address) || (0x100000..=0x100019).contains(&address)
            }
            Model::Second => (0x100000..=0x10ffff).contains(&address),
        }
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        self.contains(address)
            .then(|| self.ram[(address - self.ram_start()) as usize])
    }

    pub fn check_write(&self, access: BusAccess, value: u16) -> Option<Result<(), WriteRejection>> {
        let address = access.bus_address;
        if self.guarded(address) {
            let neutral = self.model == Model::Second
                && address == 0x100038
                && access.logical_address & BUS_ADDRESS_MASK == address
                && access.kind == AccessKind::Data
                && access.lanes == ByteLanes::Both
                && matches!(
                    access.function_code,
                    FunctionCode::UserData | FunctionCode::SupervisorData
                )
                && value == 0;
            return Some(if neutral {
                Ok(())
            } else {
                Err(WriteRejection::GuardedAddress)
            });
        }
        if !self.contains(address) {
            return None;
        }
        if address & 1 != 0 {
            return Some(Err(WriteRejection::UnalignedAddress));
        }
        if access.lanes == ByteLanes::Lower {
            return Some(Err(WriteRejection::LowerLane));
        }
        Some(Ok(()))
    }

    pub fn write(&mut self, access: BusAccess, value: u16) -> Option<bool> {
        if self.check_write(access, value)?.is_err() {
            return Some(false);
        }
        if self.guarded(access.bus_address) {
            return Some(true);
        }
        let [high, low] = value.to_be_bytes();
        let word = match access.lanes {
            ByteLanes::Both => [high, low],
            // Both SRAM chips share the upper write strobe.
            ByteLanes::Upper => [high, high],
            ByteLanes::Lower => return Some(false),
        };
        let index = (access.bus_address - self.ram_start()) as usize;
        Arc::make_mut(&mut self.ram)[index..index + 2].copy_from_slice(&word);
        Some(true)
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(STATE_TAG);
        writer.write_vec(&self.ram);
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        ensure!(
            reader.read_u8()? == STATE_TAG,
            "MD parallel adapter state tag mismatch"
        );
        let ram = reader.read_slice(RAM_BYTES)?;
        ensure!(
            ram.len() == RAM_BYTES,
            "MD parallel adapter RAM length mismatch"
        );
        self.ram = Arc::from(ram);
        Ok(())
    }
}

impl super::Cartridge {
    pub(crate) fn validate_parallel_adapter(&self) -> Result<()> {
        ensure!(
            self.parallel_adapter.as_ref().map(|board| board.model())
                == crate::cartridge::parallel_adapter::ParallelAdapter::matches_identity(
                    self.bytes.len(),
                    self.rom_digest()
                ),
            "MD parallel adapter identity mismatch"
        );
        if self.parallel_adapter.is_some() {
            ensure!(
                self.save.is_none()
                    && self.registers.is_none()
                    && self.protection.is_none()
                    && self.sf001.is_none()
                    && self.sf002.is_none()
                    && self.sf004.is_none()
                    && self.ssf2.is_none()
                    && self.lock_on.is_none()
                    && self.lock_on_save.is_none()
                    && self.linear_rom.is_none(),
                "MD parallel adapter owner conflict"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "parallel_adapter/rom_fixture.rs"]
pub(crate) mod rom_fixture;
