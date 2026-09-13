use super::*;

const MAX_PACK_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PACK_ENTRIES: usize = super::super::gsf::MAX_NATIVE_PATCHES + 2;

pub(super) struct PreparedPack {
    entries: Vec<PreparedEntry>,
}

struct PreparedEntry {
    name: String,
    original: String,
    bytes: Vec<u8>,
}

impl PreparedPack {
    pub(super) fn read(
        archive: &Archive<'_>,
        path: &Path,
        song: &PlannedSong,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let mut pack = zip::ZipArchive::new(File::open(path)?)?;
        ensure!(
            pack.len() <= MAX_PACK_ENTRIES,
            "miniGSF pack has too many entries"
        );
        let mut entries = Vec::new();
        let mut originals = std::collections::BTreeSet::new();
        let (mut mini_count, mut manifest_count, mut library_count) = (0, 0, 0);
        let mut total_bytes = 0;
        for index in 0..pack.len() {
            check_cancel(cancel)?;
            let mut entry = pack.by_index(index)?;
            let original = entry.name().to_owned();
            ensure!(
                !original.is_empty()
                    && !original.contains(['/', '\\', ':'])
                    && original != "."
                    && original != "..",
                "invalid miniGSF pack entry"
            );
            ensure!(
                originals.insert(original.clone()),
                "duplicate miniGSF pack entry"
            );
            let name = if original.ends_with(".gsflib") {
                library_count += 1;
                format!("{}/{original}", song.engine)
            } else if original.ends_with(".minigsf") {
                mini_count += 1;
                format!("{}/{:04} - {original}", song.engine, song.ordinal)
            } else {
                ensure!(original == "manifest.json", "unexpected miniGSF pack entry");
                manifest_count += 1;
                format!("{}/manifests/{:04}.json", song.engine, song.ordinal)
            };
            ensure!(
                entry.size() <= MAX_PACK_BYTES - total_bytes,
                "miniGSF pack exceeds export limit"
            );
            let mut bytes = Vec::new();
            let mut buffer = [0; 64 * 1024];
            loop {
                check_cancel(cancel)?;
                let count = entry.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                total_bytes += count as u64;
                ensure!(
                    total_bytes <= MAX_PACK_BYTES,
                    "miniGSF pack exceeds export limit"
                );
                bytes.extend_from_slice(&buffer[..count]);
            }
            ensure!(
                bytes.len() as u64 == entry.size(),
                "miniGSF pack entry length changed"
            );
            archive.check_existing(
                &name,
                bytes.len() as u64,
                &zeff_firmware::sha256_hex(&bytes),
            )?;
            entries.push(PreparedEntry {
                name,
                original,
                bytes,
            });
        }
        ensure!(
            mini_count == 1 && manifest_count == 1 && library_count != 0,
            "miniGSF pack requires one song, its manifest and libraries"
        );
        Ok(Self { entries })
    }

    pub(super) fn append(
        self,
        archive: &mut Archive<'_>,
        cancel: &AtomicBool,
    ) -> Result<Vec<OutputFile>> {
        self.entries
            .into_iter()
            .map(|entry| {
                let mut file = archive.add_bytes(&entry.name, &entry.bytes, cancel)?;
                file.original_entry = Some(entry.original);
                Ok(file)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
