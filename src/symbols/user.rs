use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use zeff_emu_common::system::System;

use super::identity::RomIdentity;
use super::{
    AddressSpaceId, Confidence, CpuLocation, DebugSegment, ExecMode, ImageId, LoadInstance,
    LoadInstanceId, Provenance, ProvenanceKind, RegionId, SegmentId, StorageLocation, StorageRange,
    SymbolId, SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
};

const FORMAT: &str = "zeff-debug-symbols";
const USER_VERSION: u32 = 1;
const ZDBG_VERSION: u32 = 2;
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
struct UserSymbolFile {
    format: String,
    version: u32,
    system: String,
    rom_sha256: String,
    symbols: Vec<UserSymbolEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    segments: Vec<SegmentEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    load_instances: Vec<LoadInstanceEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SegmentEntry {
    id: u32,
    name: String,
    rom_offset: u64,
    size: u64,
    linked_cpu_address: Option<u64>,
    exec_mode: ExecMode,
}

#[derive(Debug, Serialize, Deserialize)]
struct LoadInstanceEntry {
    id: u32,
    segment_id: u32,
    cpu_address: u64,
    generation: u64,
    created_cycle: u64,
    #[serde(default = "default_active")]
    active: bool,
}

pub(super) struct LoadedZdbgSymbols {
    pub(super) symbols: Vec<SymbolRecord>,
    pub(super) segments: Vec<DebugSegment>,
    pub(super) load_instances: Vec<LoadInstance>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserSymbolEntry {
    name: String,
    cpu_address: Option<u64>,
    rom_offset: Option<u64>,
    bank: Option<u32>,
    exec_mode: ExecMode,
    value: Option<u64>,
    size: Option<u64>,
    kind: SymbolKind,
    scope: SymbolScope,
    comment: Option<String>,
}

pub(super) fn load_user_symbols(
    path: &Path,
    identity: RomIdentity,
) -> anyhow::Result<Vec<SymbolRecord>> {
    let file = load_file(path, identity)?;
    anyhow::ensure!(
        file.version == USER_VERSION,
        "unsupported user symbol version"
    );
    entries_to_symbols(file.symbols, path, ProvenanceKind::User)
}

pub(super) fn load_zdbg_symbols(
    path: &Path,
    identity: RomIdentity,
) -> anyhow::Result<LoadedZdbgSymbols> {
    let file = load_file(path, identity)?;
    anyhow::ensure!(
        (USER_VERSION..=ZDBG_VERSION).contains(&file.version),
        "unsupported debug symbol version"
    );
    let symbols = entries_to_symbols(file.symbols, path, ProvenanceKind::DebugFormat)?;
    let (segments, load_instances) = load_segments(file.segments, file.load_instances)?;
    Ok(LoadedZdbgSymbols {
        symbols,
        segments,
        load_instances,
    })
}

fn load_file(path: &Path, identity: RomIdentity) -> anyhow::Result<UserSymbolFile> {
    let metadata = std::fs::metadata(path)?;
    anyhow::ensure!(
        metadata.len() <= MAX_BYTES,
        "user symbol file is larger than 16 MiB"
    );
    let data = std::fs::read(path)?;
    let file: UserSymbolFile = serde_json::from_slice(&data).context("invalid user symbol JSON")?;
    anyhow::ensure!(file.format == FORMAT, "unsupported user symbol format");
    anyhow::ensure!(
        file.system == system_code(identity.system),
        "system mismatch"
    );
    anyhow::ensure!(
        file.rom_sha256 == encode_hash(identity.sha256),
        "ROM hash mismatch"
    );

    Ok(file)
}

fn entries_to_symbols(
    symbols: Vec<UserSymbolEntry>,
    path: &Path,
    provenance: ProvenanceKind,
) -> anyhow::Result<Vec<SymbolRecord>> {
    symbols
        .into_iter()
        .map(|entry| {
            validate_name(&entry.name)?;
            anyhow::ensure!(
                entry.cpu_address.is_some() || entry.rom_offset.is_some() || entry.value.is_some(),
                "{} has no location or value",
                entry.name
            );
            Ok(SymbolRecord {
                id: SymbolId(0),
                name: entry.name,
                location: SymbolLocation {
                    cpu: entry.cpu_address.map(|address| CpuLocation {
                        space: AddressSpaceId(0),
                        address,
                    }),
                    storage: entry.rom_offset.map(|offset| StorageLocation {
                        image: ImageId(0),
                        region: RegionId(0),
                        offset,
                    }),
                    bank: entry.bank,
                    exec_mode: entry.exec_mode,
                },
                value: entry.value,
                size: entry.size,
                kind: entry.kind,
                scope: entry.scope,
                provenance: Provenance {
                    kind: provenance,
                    source: Some(path.display().to_string()),
                },
                confidence: Confidence::Exact,
                comment: entry.comment,
            })
        })
        .collect()
}

fn load_segments(
    segments: Vec<SegmentEntry>,
    instances: Vec<LoadInstanceEntry>,
) -> anyhow::Result<(Vec<DebugSegment>, Vec<LoadInstance>)> {
    let mut segment_ids = HashSet::new();
    let segments = segments
        .into_iter()
        .map(|segment| {
            validate_name(&segment.name)?;
            anyhow::ensure!(
                segment_ids.insert(segment.id),
                "duplicate segment id {}",
                segment.id
            );
            anyhow::ensure!(
                segment.size != 0,
                "{} has an empty storage range",
                segment.name
            );
            segment
                .rom_offset
                .checked_add(segment.size - 1)
                .context("segment storage range overflows")?;
            Ok(DebugSegment {
                id: SegmentId(segment.id),
                name: segment.name,
                storage: StorageRange {
                    start: StorageLocation {
                        image: ImageId(0),
                        region: RegionId(0),
                        offset: segment.rom_offset,
                    },
                    size: segment.size,
                },
                linked_cpu: segment.linked_cpu_address.map(|address| CpuLocation {
                    space: AddressSpaceId(0),
                    address,
                }),
                exec_mode: segment.exec_mode,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let sizes = segments
        .iter()
        .map(|segment| (segment.id, segment.storage.size))
        .collect::<std::collections::HashMap<_, _>>();
    let mut instance_ids = HashSet::new();
    let instances = instances
        .into_iter()
        .map(|instance| {
            anyhow::ensure!(
                instance_ids.insert(instance.id),
                "duplicate load instance id {}",
                instance.id
            );
            let segment = SegmentId(instance.segment_id);
            let size = sizes
                .get(&segment)
                .context("load instance references an unknown segment")?;
            instance
                .cpu_address
                .checked_add(size - 1)
                .context("load instance CPU range overflows")?;
            Ok(LoadInstance {
                id: LoadInstanceId(instance.id),
                segment,
                runtime_base: CpuLocation {
                    space: AddressSpaceId(0),
                    address: instance.cpu_address,
                },
                generation: instance.generation,
                created_cycle: instance.created_cycle,
                active: instance.active,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok((segments, instances))
}

const fn default_active() -> bool {
    true
}

pub(super) fn save_user_symbols(
    path: &Path,
    identity: RomIdentity,
    symbols: &[SymbolRecord],
) -> anyhow::Result<()> {
    let file = UserSymbolFile {
        format: FORMAT.to_owned(),
        version: USER_VERSION,
        system: system_code(identity.system).to_owned(),
        rom_sha256: encode_hash(identity.sha256),
        symbols: symbols
            .iter()
            .map(|symbol| UserSymbolEntry {
                name: symbol.name.clone(),
                cpu_address: symbol.location.cpu.map(|location| location.address),
                rom_offset: symbol.location.storage.map(|location| location.offset),
                bank: symbol.location.bank,
                exec_mode: symbol.location.exec_mode,
                value: symbol.value,
                size: symbol.size,
                kind: symbol.kind,
                scope: symbol.scope,
                comment: symbol.comment.clone(),
            })
            .collect(),
        segments: Vec::new(),
        load_instances: Vec::new(),
    };
    let data = serde_json::to_vec_pretty(&file)?;
    crate::platform::write_save_data(path, &data)
}

pub(super) fn validate_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!name.trim().is_empty(), "symbol name is empty");
    anyhow::ensure!(name.len() <= 256, "symbol name is too long");
    anyhow::ensure!(
        !name.chars().any(char::is_control),
        "symbol name contains a control character"
    );
    Ok(())
}

fn system_code(system: System) -> &'static str {
    match system {
        System::Gb => "gb",
        System::Gba => "gba",
        System::Nes => "nes",
        System::Coleco => "coleco",
        System::Pce => "pce",
        System::Ws => "ws",
        System::Sms => "sms",
        System::Gg => "gg",
        System::Sg => "sg",
    }
}

fn encode_hash(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "zeff-user-symbols-{}-{}.json",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn identity(hash: u8) -> RomIdentity {
        RomIdentity {
            system: System::Gb,
            sha256: [hash; 32],
        }
    }

    fn symbol() -> SymbolRecord {
        SymbolRecord {
            id: SymbolId(0),
            name: "UpdatePlayer".to_owned(),
            location: SymbolLocation {
                cpu: Some(CpuLocation {
                    space: AddressSpaceId(0),
                    address: 0x4560,
                }),
                storage: Some(StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset: 0x8560,
                }),
                bank: Some(2),
                exec_mode: ExecMode::Sm83,
            },
            value: None,
            size: Some(12),
            kind: SymbolKind::Function,
            scope: SymbolScope::Global,
            provenance: Provenance {
                kind: ProvenanceKind::User,
                source: None,
            },
            confidence: Confidence::Exact,
            comment: Some("Movement update".to_owned()),
        }
    }

    #[test]
    fn round_trips_user_symbols() {
        let path = temp_path();
        save_user_symbols(&path, identity(1), &[symbol()]).unwrap();
        let loaded = load_user_symbols(&path, identity(1)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "UpdatePlayer");
        assert_eq!(loaded[0].location.storage.unwrap().offset, 0x8560);
        assert_eq!(loaded[0].comment.as_deref(), Some("Movement update"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_symbols_for_another_rom() {
        let path = temp_path();
        save_user_symbols(&path, identity(1), &[symbol()]).unwrap();
        assert!(load_user_symbols(&path, identity(2)).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
