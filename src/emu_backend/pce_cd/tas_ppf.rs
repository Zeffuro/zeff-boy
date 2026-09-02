use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::{
    LoadedPceCd, PCE_CD_CUE_BYTES_LIMIT, PceCdLoadError, parse_cue_bytes, pce_cd_mod_config,
};

const TAS_PPF_SOURCE_DOMAIN: &[u8] = b"zeff-tas-pce-cd-ppf-source:v1\0";

#[derive(Clone, Debug)]
pub(crate) struct PceCdTasPpfStack {
    patches: Vec<(String, Vec<u8>)>,
    source_media_sha256: [u8; 32],
    source_media_len: usize,
}

impl PceCdTasPpfStack {
    pub(crate) fn discover(cue_path: &Path) -> Result<Option<Self>, PceCdLoadError> {
        let base = super::load_direct_cue_with_mods(cue_path, false)?;
        let (directory, mods, _) = pce_cd_mod_config(
            crc32fast::hash(&base.source_disc_sha256),
            base.content_crc32,
        );
        let enabled = mods
            .iter()
            .filter(|entry| entry.enabled)
            .collect::<Vec<_>>();
        if enabled.is_empty() {
            return Ok(None);
        }
        let patches = enabled
            .into_iter()
            .map(|entry| {
                let filename = Path::new(&entry.filename);
                validate_filename(filename)?;
                Ok((
                    entry.filename.clone(),
                    read_patch(&directory.join(filename))?,
                ))
            })
            .collect::<Result<Vec<_>, PceCdLoadError>>()?;
        Ok(Some(Self::from_patches(base, patches)?))
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        cue_path: &Path,
        patches: Vec<(String, Vec<u8>)>,
    ) -> Result<Self, PceCdLoadError> {
        Self::from_patches(super::load_direct_cue_with_mods(cue_path, false)?, patches)
    }

    fn from_patches(
        base: LoadedPceCd,
        patches: Vec<(String, Vec<u8>)>,
    ) -> Result<Self, PceCdLoadError> {
        let mut source_len = base.raw_source_media_len;
        let mut hasher = Sha256::new();
        hasher.update(TAS_PPF_SOURCE_DOMAIN);
        hasher.update(base.raw_source_media_sha256);
        hasher.update((base.raw_source_media_len as u64).to_le_bytes());
        hasher.update((patches.len() as u64).to_le_bytes());
        for (filename, bytes) in &patches {
            let path = Path::new(filename);
            validate_filename(path)?;
            if bytes.len() > super::super::pce_cd_overlay::PATCH_BYTES_LIMIT {
                return Err(PceCdLoadError::Disc(
                    "PC Engine CD TAS PPF overlay is outside bounded limits".to_owned(),
                ));
            }
            source_len = source_len
                .checked_add(bytes.len())
                .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
            hasher.update((filename.len() as u64).to_le_bytes());
            hasher.update(filename.as_bytes());
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(Sha256::digest(bytes));
        }
        Ok(Self {
            patches,
            source_media_sha256: hasher.finalize().into(),
            source_media_len: source_len,
        })
    }

    pub(crate) fn source_media_identity(&self) -> ([u8; 32], usize) {
        (self.source_media_sha256, self.source_media_len)
    }

    pub(crate) fn load(&self, cue_path: &Path) -> Result<LoadedPceCd, PceCdLoadError> {
        let cue_metadata = std::fs::metadata(cue_path)
            .map_err(|_| PceCdLoadError::CueUnreadable(cue_path.to_path_buf()))?;
        if cue_metadata.len() > PCE_CD_CUE_BYTES_LIMIT as u64 {
            return Err(PceCdLoadError::CueTooLarge(cue_metadata.len()));
        }
        let cue_bytes = std::fs::read(cue_path)
            .map_err(|_| PceCdLoadError::CueUnreadable(cue_path.to_path_buf()))?;
        let sheet = parse_cue_bytes(&cue_bytes)?;
        let mut loaded =
            super::super::pce_cd_file::load_direct_cue_file_backed(cue_path, &cue_bytes, &sheet)?;
        let disc = super::super::pce_cd_file::try_load_direct_cue_ppf_overlay_bytes(
            cue_path,
            &sheet,
            &self.patches,
        )?
        .ok_or_else(|| {
            PceCdLoadError::Disc("PC Engine CD TAS PPF overlay is unsupported".to_owned())
        })?;
        loaded.disc = disc;
        Ok(loaded)
    }
}

fn validate_filename(path: &Path) -> Result<(), PceCdLoadError> {
    if path.file_name() != Some(path.as_os_str())
        || !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ppf"))
    {
        return Err(PceCdLoadError::Disc(
            "PC Engine CD TAS requires an ordered PPF-only overlay stack".to_owned(),
        ));
    }
    Ok(())
}

fn read_patch(path: &Path) -> Result<Vec<u8>, PceCdLoadError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| PceCdLoadError::BinUnreadable(path.to_path_buf()))?;
    if !metadata.file_type().is_file()
        || metadata.len() > super::super::pce_cd_overlay::PATCH_BYTES_LIMIT as u64
    {
        return Err(PceCdLoadError::Disc(
            "PC Engine CD TAS PPF overlay is outside bounded limits".to_owned(),
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|_| PceCdLoadError::DataTooLarge(metadata.len()))?;
    let mut bytes = vec![0; len];
    let mut file =
        File::open(path).map_err(|_| PceCdLoadError::BinUnreadable(path.to_path_buf()))?;
    file.read_exact(&mut bytes)
        .map_err(|_| PceCdLoadError::BinUnreadable(path.to_path_buf()))?;
    Ok(bytes)
}
