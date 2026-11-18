use std::mem::size_of;
use std::path::PathBuf;

use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_md_core::Emulator;
use zeff_md_core::emulator::rollback::{MdRetainedAllocations, MdRollbackSession};

use super::{MAX_MD_TAS_OWNED_PAYLOAD, MdTasLoad};
use crate::emu_backend::md::MdBackend;
use crate::emu_core_trait::EmulatorCore;

pub(crate) struct MdTasSeed {
    load: MdTasLoad,
    runtime: Box<Emulator>,
    source: PathBuf,
    rom: PathBuf,
}

impl MdTasSeed {
    pub(in crate::emu_backend) fn from_normal_load(
        md: &MdBackend,
        project_raw: &[u8],
    ) -> Result<Self> {
        let load = md.tas_load()?;
        load.validate_fresh(md)?;
        load.validate_source(md)?;
        ensure!(
            md.encode_state_bytes()?.as_slice() == project_raw,
            "MD project RAW differs from authenticated fresh seed"
        );
        let seed = Self {
            load: load.clone(),
            runtime: Box::new(md.core().clone()),
            source: md.source_path().to_owned(),
            rom: md.rom_path().to_owned(),
        };
        let mut retained = MdRetainedAllocations::new();
        seed.visit_retained(&mut retained)?;
        ensure!(
            retained.finish()?.requested_payload_bytes()
                + size_of::<Self>() as u64
                + seed.metadata_bytes() as u64
                <= MAX_MD_TAS_OWNED_PAYLOAD,
            "MD authenticated seed exceeds owned payload bound"
        );
        Ok(seed)
    }

    pub(in crate::emu_backend) fn validate_for(&self, load: &MdTasLoad) -> Result<()> {
        ensure!(
            &self.load == load,
            "MD TAS seed source, member or profile differs"
        );
        self.load.validate_paths(&self.source, &self.rom)?;
        ensure!(
            self.runtime.pacing_field_count() == 0
                && self.runtime.pending_audio_frames() == 0
                && <[u8; 32]>::from(Sha256::digest(self.runtime.encode_state()?))
                    == self.load.fresh_raw
                && self.load.config().boundary_hashes(&self.runtime)? == self.load.fresh_hashes,
            "MD seed fresh runtime proof changed"
        );
        Ok(())
    }

    pub(in crate::emu_backend) fn install(
        &self,
        session: &mut MdRollbackSession,
        load: &MdTasLoad,
    ) -> Result<()> {
        self.validate_for(load)?;
        session.install_tas_seed(
            self.runtime.clone(),
            self.load.config(),
            &self.load.fresh_hashes,
        )
    }

    pub(in crate::emu_backend) fn visit_retained<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        into.visit_emulator(&self.runtime)
    }

    pub(in crate::emu_backend) fn metadata_bytes(&self) -> usize {
        self.source.capacity() + self.rom.capacity() + self.load.retained_source_bytes()
    }
}

impl MdBackend {
    pub(crate) fn capture_authenticated_tas_seed(&self, project_raw: &[u8]) -> Result<MdTasSeed> {
        MdTasSeed::from_normal_load(self, project_raw)
    }
}
