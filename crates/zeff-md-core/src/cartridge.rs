use crate::constants::MAX_CARTRIDGE_SIZE;
use crate::emulator::Region;
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::{Arc, OnceLock};
use zeff_m68k::Registers;
use zeff_m68k::bus::BUS_ADDRESS_MASK;

mod debug_save;
pub(crate) mod download;
mod eeprom;
pub(crate) mod extended_ssf;
mod header;
mod rollback;
mod sampled;
pub(crate) use sampled::SampledByte;
#[cfg(test)]
pub(crate) mod fixed_register_fixture;
pub(crate) mod linear_rom;
pub(crate) mod parallel_adapter;
pub(crate) mod protection;
pub(crate) mod ram;
pub(crate) mod realtec;
pub(crate) mod registers;
pub(crate) mod rom_overlay;
pub(crate) mod save_latch;
pub(crate) mod sf001;
pub(crate) mod sf002;
mod sf002_access;
pub(crate) mod sf004;
pub(crate) mod ssf2;
pub(crate) mod unprotected_rom;
pub use header::CartridgeHeader;
pub(crate) use ram::SaveMemory;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    UnsupportedRegion(Region),
    CartridgeSize(usize),
    ResetProgramCounter(u32),
    ResetStackPointer(u32),
    UnsupportedSaveHardware,
    TimingProfile(String),
}

#[derive(Clone)]
pub(crate) struct Cartridge {
    pub bytes: Arc<[u8]>,
    pub svp: Option<Box<crate::svp::InstructionModel>>,
    pub initial: Registers,
    pub header: Option<Arc<CartridgeHeader>>,
    pub save: Option<SaveMemory>,
    pub save_latch: Option<save_latch::SaveLatch>,
    pub download: Option<download::DownloadRam>,
    pub linear_rom: Option<linear_rom::LinearRomOnly>,
    pub unprotected_rom: Option<unprotected_rom::UnprotectedRom>,
    pub parallel_adapter: Option<parallel_adapter::ParallelAdapter>,
    pub registers: Option<registers::CartridgeRegisters>,
    pub protection: Option<protection::FixedProtection>,
    pub sf001: Option<sf001::Sf001>,
    pub sf002: Option<sf002::Sf002>,
    pub sf004: Option<sf004::Sf004>,
    pub extended_ssf: Option<extended_ssf::ExtendedSsf>,
    pub realtec: Option<realtec::Realtec>,
    pub ssf2: Option<ssf2::Ssf2>,
    pub lock_on: Option<registers::LockOn>,
    pub lock_on_save: Option<registers::LockOnSave>,
    pub(crate) rom_overlay: rom_overlay::RomOverlay,
    digest_memo: Arc<RomDigestMemo>,
}

impl Cartridge {
    pub(crate) fn rollback_plain_factory(&self) -> bool {
        Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self.svp.is_none()
            && self.save.is_none()
            && self.save_latch.is_none()
            && self.download.is_none()
            && self.linear_rom.is_none()
            && self.unprotected_rom.is_none()
            && self.parallel_adapter.is_none()
            && self.registers.is_none()
            && self.protection.is_none()
            && self.sf001.is_none()
            && self.sf002.is_none()
            && self.sf004.is_none()
            && self.extended_ssf.is_none()
            && self.realtec.is_none()
            && self.ssf2.is_none()
            && self.lock_on.is_none()
            && self.lock_on_save.is_none()
            && self.rom_overlay.is_empty()
    }
}

struct RomDigestMemo {
    seal: Arc<[u8]>,
    factory_save: Option<ram::OddSaveLayout>,
    factory_timed_save: bool,
    factory_lock_on: Option<registers::LockOn>,
    factory_registers: Option<registers::CartridgeRegisters>,
    factory_protection: Option<protection::FixedProtection>,
    digest: OnceLock<[u8; 32]>,
}

impl PartialEq for Cartridge {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
            && self.svp == other.svp
            && self.initial == other.initial
            && self.header == other.header
            && self.save == other.save
            && self.save_latch == other.save_latch
            && self.download == other.download
            && self.linear_rom == other.linear_rom
            && self.unprotected_rom == other.unprotected_rom
            && self.parallel_adapter == other.parallel_adapter
            && self.registers == other.registers
            && self.protection == other.protection
            && self.sf001 == other.sf001
            && self.sf002 == other.sf002
            && self.sf004 == other.sf004
            && self.extended_ssf == other.extended_ssf
            && self.realtec == other.realtec
            && self.ssf2 == other.ssf2
            && self.lock_on == other.lock_on
            && self.lock_on_save == other.lock_on_save
            && self.rom_overlay == other.rom_overlay
    }
}

impl Eq for Cartridge {}

impl fmt::Debug for Cartridge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut value = formatter.debug_struct("Cartridge");
        value
            .field("bytes", &self.bytes)
            .field("initial", &self.initial)
            .field("header", &self.header)
            .field("save", &self.save)
            .field("parallel_adapter", &self.parallel_adapter)
            .field("registers", &self.registers)
            .field("protection", &self.protection)
            .field("sf001", &self.sf001)
            .field("sf002", &self.sf002)
            .field("sf004", &self.sf004)
            .field("extended_ssf", &self.extended_ssf)
            .field("ssf2", &self.ssf2)
            .field("lock_on", &self.lock_on)
            .field("lock_on_save", &self.lock_on_save);
        if let Some(owner) = &self.linear_rom {
            value.field("linear_rom", owner);
        }
        if let Some(owner) = &self.svp {
            value.field("svp", owner);
        }
        if let Some(owner) = &self.save_latch {
            value.field("save_latch", owner);
        }
        if let Some(owner) = &self.download {
            value.field("download", owner);
        }
        if let Some(owner) = &self.unprotected_rom {
            value.field("unprotected_rom", owner);
        }
        if let Some(owner) = &self.realtec {
            value.field("realtec", owner);
        }
        value.finish()
    }
}

#[cfg(test)]
pub(crate) fn assert_digest_memo_sharing(bytes: &[u8]) {
    let source = Cartridge::new(bytes.to_vec()).unwrap();
    let expected: [u8; 32] = Sha256::digest(bytes).into();
    assert!(source.digest_memo.digest.get().is_none());
    let mut detached = source.clone();
    assert!(Arc::ptr_eq(&source.digest_memo, &detached.digest_memo));
    assert!(Arc::ptr_eq(&source.bytes, &source.digest_memo.seal));
    *Arc::make_mut(&mut detached.bytes).last_mut().unwrap() ^= 1;
    assert!(!Arc::ptr_eq(&detached.bytes, &source.digest_memo.seal));
    let detached_expected: [u8; 32] = Sha256::digest(detached.bytes.as_ref()).into();
    assert_eq!(detached.rom_digest(), detached_expected);
    assert_ne!(detached_expected, expected);
    assert!(source.digest_memo.digest.get().is_none());
    assert!(detached.digest_memo.digest.get().is_none());
    let snapshot = source.clone();
    assert!(Arc::ptr_eq(&source.digest_memo, &snapshot.digest_memo));
    assert!(snapshot.digest_memo.digest.get().is_none());
    assert_eq!(snapshot.rom_digest(), expected);
    assert_eq!(source.digest_memo.digest.get(), Some(&expected));
    assert_eq!(snapshot.digest_memo.digest.get(), Some(&expected));
    assert_eq!(source.rom_digest(), expected);
}

impl Cartridge {
    pub fn new(bytes: Vec<u8>) -> Result<Self, LoadError> {
        let ssf2 = ssf2::Ssf2::detect(&bytes).then(ssf2::Ssf2::new);
        let extended_ssf = extended_ssf::ExtendedSsf::detect(&bytes)
            .then(|| extended_ssf::ExtendedSsf::new(bytes.len()));
        if !(8..=MAX_CARTRIDGE_SIZE).contains(&bytes.len())
            && ssf2.is_none()
            && extended_ssf.is_none()
        {
            return Err(LoadError::CartridgeSize(bytes.len()));
        }
        let realtec = realtec::Realtec::detect(&bytes).then(realtec::Realtec::new);
        let vectors = realtec
            .as_ref()
            .map_or(0, |board| board.source_offset(0).unwrap());
        let sp = u32::from_be_bytes(bytes[vectors..vectors + 4].try_into().unwrap());
        let pc = u32::from_be_bytes(bytes[vectors + 4..vectors + 8].try_into().unwrap());
        let address = (pc & BUS_ADDRESS_MASK) as usize;
        let mapped_pc = if realtec.is_some() {
            realtec::Realtec::cold_prefetch_valid(pc)
        } else if let Some(board) = &extended_ssf {
            (address..address + 4).all(|lane| board.source_offset(lane as u32).is_some())
        } else {
            address + 3 < bytes.len()
        };
        if pc & 1 != 0 || !mapped_pc {
            return Err(LoadError::ResetProgramCounter(pc));
        }
        let mut initial = Registers::new();
        initial.ssp = sp;
        initial.pc = pc;
        let header = CartridgeHeader::parse(&bytes).map(Arc::new);
        let unprotected_rom =
            unprotected_rom::UnprotectedRom::detect(&bytes, header.as_deref(), || {
                Sha256::digest(&bytes).into()
            });
        let save_latch = save_latch::SaveLatch::detect(&bytes, header.as_deref(), || {
            Sha256::digest(&bytes).into()
        });
        let download = download::DownloadRam::detect(&bytes, header.as_deref(), || {
            Sha256::digest(&bytes).into()
        });
        let sf004 = sf004::Sf004::detect(&bytes).then(sf004::Sf004::new);
        let linear_rom = linear_rom::LinearRomOnly::detect(&bytes);
        let parallel_adapter = parallel_adapter::ParallelAdapter::detect(&bytes)
            .map(parallel_adapter::ParallelAdapter::new);
        let save = if linear_rom.is_some()
            || sf004.is_some()
            || extended_ssf.is_some()
            || realtec.is_some()
        {
            None
        } else {
            SaveMemory::from_rom(&bytes)?
        };
        let registers =
            registers::CartridgeRegisters::detect_rom(&bytes, header.as_deref(), || {
                Sha256::digest(&bytes).into()
            });
        let protection = protection::FixedProtection::detect(bytes.len(), header.as_deref());
        let sf002 = sf002::Sf002::detect(&bytes).then(sf002::Sf002::new);
        let sf001 = sf001::Sf001::detect_revision(&bytes, header.as_deref()).map(sf001::Sf001::new);
        if sf001.is_some() && save.is_some()
            || ssf2.is_some()
                && (save.is_some()
                    || registers.is_some()
                    || protection.is_some()
                    || sf001.is_some())
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        if sf002.is_some()
            && (sf001.is_some()
                || ssf2.is_some()
                || registers.is_some()
                || protection.is_some()
                || !save.as_ref().is_some_and(SaveMemory::sf002_compatible))
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        let lock_on = registers::LockOn::detect(&bytes).filter(|_| {
            save.is_none()
                && registers.is_none()
                && protection.is_none()
                && sf001.is_none()
                && ssf2.is_none()
                && sf002.is_none()
        });
        if linear_rom.is_some()
            && (save.is_some()
                || registers.is_some()
                || protection.is_some()
                || sf001.is_some()
                || sf002.is_some()
                || ssf2.is_some()
                || lock_on.is_some())
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        if parallel_adapter.is_some()
            && (save.is_some()
                || registers.is_some()
                || protection.is_some()
                || sf001.is_some()
                || sf002.is_some()
                || ssf2.is_some()
                || lock_on.is_some()
                || linear_rom.is_some())
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        if sf004.is_some()
            && (save.is_some()
                || registers.is_some()
                || protection.is_some()
                || sf001.is_some()
                || sf002.is_some()
                || ssf2.is_some()
                || lock_on.is_some()
                || linear_rom.is_some()
                || parallel_adapter.is_some())
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        if extended_ssf.is_some()
            && (save.is_some()
                || registers.is_some()
                || protection.is_some()
                || sf001.is_some()
                || sf002.is_some()
                || sf004.is_some()
                || ssf2.is_some()
                || lock_on.is_some()
                || linear_rom.is_some()
                || parallel_adapter.is_some())
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        if realtec.is_some()
            && (save.is_some()
                || registers.is_some()
                || protection.is_some()
                || sf001.is_some()
                || sf002.is_some()
                || sf004.is_some()
                || extended_ssf.is_some()
                || ssf2.is_some()
                || lock_on.is_some()
                || linear_rom.is_some()
                || parallel_adapter.is_some())
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        let bytes: Arc<[u8]> = bytes.into();
        let digest_memo = Arc::new(RomDigestMemo {
            seal: Arc::clone(&bytes),
            factory_save: save
                .as_ref()
                .and_then(SaveMemory::special_rom_overlay_layout),
            factory_timed_save: save
                .as_ref()
                .is_some_and(|save| save.ordinary_timed_header(&bytes))
                && save_latch.is_none()
                && download.is_none()
                && linear_rom.is_none()
                && unprotected_rom.is_none()
                && parallel_adapter.is_none()
                && registers.is_none()
                && protection.is_none()
                && sf001.is_none()
                && sf002.is_none()
                && sf004.is_none()
                && extended_ssf.is_none()
                && realtec.is_none()
                && ssf2.is_none()
                && lock_on.is_none(),
            factory_lock_on: lock_on,
            factory_registers: registers.clone(),
            factory_protection: protection,
            digest: OnceLock::new(),
        });
        let cartridge = Self {
            svp: None,
            registers,
            protection,
            sf001,
            sf002,
            sf004,
            extended_ssf,
            realtec,
            ssf2,
            lock_on,
            lock_on_save: (lock_on == Some(registers::LockOn::TwoMiB))
                .then(registers::LockOnSave::new),
            header,
            save,
            save_latch,
            download,
            linear_rom,
            unprotected_rom,
            parallel_adapter,
            bytes,
            initial,
            rom_overlay: Default::default(),
            digest_memo,
        };
        if cartridge.unprotected_rom.is_some() && !cartridge.unprotected_rom_compatible() {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        if cartridge.save_latch.is_some() && !cartridge.save_latch_compatible() {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        if cartridge.download.is_some() && !cartridge.download_compatible() {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        if cartridge
            .save
            .as_ref()
            .is_some_and(SaveMemory::expansion_bytes)
            && cartridge.validate_expansion_save().is_err()
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        Ok(cartridge)
    }

    pub(crate) fn rom_digest(&self) -> [u8; 32] {
        if Arc::ptr_eq(&self.bytes, &self.digest_memo.seal) {
            *self
                .digest_memo
                .digest
                .get_or_init(|| Sha256::digest(self.digest_memo.seal.as_ref()).into())
        } else {
            Sha256::digest(self.bytes.as_ref()).into()
        }
    }

    pub(crate) fn parallel_adapter_domain(&self, address: u32) -> bool {
        self.parallel_adapter
            .as_ref()
            .is_some_and(|board| board.contains(address) || board.guarded(address))
    }

    pub(crate) fn bounded_split(&self) -> bool {
        self.save.as_ref().is_some_and(SaveMemory::bounded_split)
    }

    pub(crate) fn bounded_split_hole(&self, address: u32) -> bool {
        self.bounded_split() && (0x210000..0x300000).contains(&address)
    }
    #[inline]
    pub(crate) fn bounded_odd_guarded(&self, address: u32) -> bool {
        ((0x200000..0x400000).contains(&address) || (0xa13000..0xa13100).contains(&address))
            && self.save.as_ref().is_some_and(SaveMemory::bounded_odd)
    }

    #[inline]
    pub(crate) fn bounded_switched_guarded(&self, address: u32) -> bool {
        ((0x200000..0x400000).contains(&address) || (0xa13000..0xa13100).contains(&address))
            && self.save.as_ref().is_some_and(SaveMemory::partial_switched)
    }

    pub fn mode_sensitive_read(&self, address: u32) -> bool {
        self.svp.is_some() && (0x200000..=0x3fffff).contains(&address)
            || self.expansion_save_contains(address)
            || self.download.is_some() && download::memory(address)
            || self.save_latch.is_some() && save_latch::SaveLatch::save_address(address)
            || self.bounded_odd_guarded(address)
            || self.bounded_switched_guarded(address)
            || self
                .registers
                .as_ref()
                .is_some_and(|board| board.source_offset(address).is_some())
            || self.realtec.is_some() && address <= 0x3fffff
            || self.extended_ssf.is_some() && address <= 0x3fffff
            || self.parallel_adapter_domain(address)
            || self.sf004.is_some() && sf004::Sf004::domain(address)
            || self.bounded_split() && (0x200000..0x300000).contains(&address)
            || self.sf002.is_some() && (0x200000..=0x3c3fff).contains(&address)
            || self.ssf2.is_some() && ssf2::Ssf2::mode_sensitive(address)
            || self
                .lock_on
                .is_some_and(|mapping| mapping.mode_sensitive(address))
            || self
                .lock_on_save
                .as_ref()
                .is_some_and(|save| save.contains(address))
    }

    pub fn rom_source_offset(&self, address: u32) -> Option<usize> {
        if self.svp.is_some() {
            return (address < 0x200000 && (address as usize) < self.bytes.len())
                .then_some(address as usize);
        }
        if self.download.is_some() {
            return None;
        }
        if self.save_latch.is_some() && address < 0x400000 {
            return self.save_latch_rom_offset(address);
        }
        if self.bounded_switched_guarded(address) {
            return (self.save.as_ref()?.rom_visible() && (address as usize) < self.bytes.len())
                .then_some(address as usize);
        }
        if self.bounded_odd_guarded(address) {
            return None;
        }
        if let Some(offset) = self
            .registers
            .as_ref()
            .and_then(|board| board.source_offset(address))
        {
            return (offset < self.bytes.len()).then_some(offset);
        }
        if let Some(board) = &self.realtec {
            return board.source_offset(address);
        }
        if let Some(board) = &self.extended_ssf {
            return board.source_offset(address);
        }
        if let Some(board) = &self.sf004 {
            return board.source_offset(address);
        }
        if let Some(board) = &self.sf001 {
            return match board.decode(address) {
                sf001::Location::Rom(offset) => Some(offset as usize),
                _ => None,
            };
        }
        if self.bounded_split() && (0x200000..0x300000).contains(&address) {
            return None;
        }
        if self.parallel_adapter_domain(address) {
            return None;
        }
        if let Some(board) = &self.sf002 {
            return board.source_offset(address).ok().flatten();
        }
        if let Some(board) = &self.ssf2 {
            return board.source_offset(address);
        }
        if self
            .lock_on_save
            .as_ref()
            .is_some_and(|save| save.enabled && save.contains(address))
        {
            return None;
        }
        if (0x200000..=0x3fffff).contains(&address)
            && let Some(mapping) = self.lock_on
        {
            mapping.source_offset(address)
        } else {
            ((address as usize) < self.bytes.len()).then_some(address as usize)
        }
    }
}
