#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;

use crate::emu_backend::EmuBackend;

use super::identity::IdentityStatus;
#[cfg(not(target_arch = "wasm32"))]
use super::identity::RomIdentity;
#[cfg(not(target_arch = "wasm32"))]
use super::import::{ImportContext, TargetInfo, import_symbols};
use super::store::SymbolStore;
use super::{AddressSpaceId, CpuLocation, ImageId, RegionId, StorageLocation};

#[cfg(not(target_arch = "wasm32"))]
const MAX_SYMBOL_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct LoadedSymbolModule {
    pub(crate) path: PathBuf,
    pub(crate) format: String,
    pub(crate) identity: IdentityStatus,
    pub(crate) symbol_count: usize,
    pub(crate) diagnostics: Vec<String>,
}

#[derive(Default)]
pub(crate) struct SymbolSession {
    pub(crate) store: SymbolStore,
    pub(crate) modules: Vec<LoadedSymbolModule>,
    pub(crate) diagnostics: Vec<String>,
}

impl SymbolSession {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_for_backend(backend: &EmuBackend) -> Self {
        Self::load_sidecar(
            backend.system(),
            backend.rom_path(),
            backend.source_path(),
            backend.rom_hash(),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn load_for_backend(_backend: &EmuBackend) -> Self {
        Self::default()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_sidecar(
        system: zeff_emu_common::system::System,
        rom_path: &Path,
        source_path: &Path,
        rom_sha256: [u8; 32],
    ) -> Self {
        let mut session = Self::default();
        let Some(path) = discover_symbol_sidecar(source_path, rom_path) else {
            return session;
        };
        let result = (|| -> anyhow::Result<_> {
            let metadata = std::fs::metadata(&path)?;
            anyhow::ensure!(
                metadata.len() <= MAX_SYMBOL_FILE_BYTES,
                "symbol file is larger than 64 MiB"
            );
            let data = std::fs::read(&path)?;
            let mut module = import_symbols(
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("symbols.sym"),
                &data,
                &ImportContext {
                    target: TargetInfo { system },
                    image: ImageId(0),
                    rom_region: RegionId(0),
                    cpu_space: AddressSpaceId(0),
                    source_name: Some(path.display().to_string()),
                },
            )?;
            let count = module.symbols.len();
            session.store.extend(module.symbols.drain(..));
            Ok(LoadedSymbolModule {
                path: path.clone(),
                format: module.format,
                identity: IdentityStatus::compare(
                    None,
                    RomIdentity {
                        system,
                        sha256: rom_sha256,
                    },
                ),
                symbol_count: count,
                diagnostics: module.diagnostics,
            })
        })();
        match result {
            Ok(module) => {
                log::info!(
                    "Loaded {} symbols from {} ({})",
                    module.symbol_count,
                    module.path.display(),
                    module.identity.label()
                );
                session.modules.push(module);
            }
            Err(error) => session
                .diagnostics
                .push(format!("{}: {error}", path.display())),
        }
        session
    }

    pub(crate) fn symbol_count(&self) -> usize {
        self.store.len()
    }

    pub(crate) fn symbol_name_at_rom_offset(&self, offset: u64) -> Option<&str> {
        self.store
            .lookup_storage(StorageLocation {
                image: ImageId(0),
                region: RegionId(0),
                offset,
            })
            .next()
            .map(|symbol| symbol.name.as_str())
    }

    pub(crate) fn symbol_name_at_cpu_address(&self, address: u64) -> Option<&str> {
        self.store
            .lookup_cpu(CpuLocation {
                space: AddressSpaceId(0),
                address,
            })
            .next()
            .map(|symbol| symbol.name.as_str())
    }

    pub(crate) fn resolve_cpu_name(&self, name: &str) -> Option<u64> {
        self.store
            .lookup_name(name)
            .chain(self.store.lookup_name_case_insensitive(name))
            .find_map(|symbol| symbol.location.cpu.map(|location| location.address))
    }

    pub(crate) fn resolve_rom_name(&self, name: &str) -> Option<u64> {
        self.store
            .lookup_name(name)
            .chain(self.store.lookup_name_case_insensitive(name))
            .find_map(|symbol| symbol.location.storage.map(|location| location.offset))
    }

    pub(crate) fn annotate_disassembly(&self, view: &mut crate::debug::DisassemblyView) {
        for line in &mut view.lines {
            if line.symbol.is_none()
                && let Some(offset) = line.storage_offset
                && let Some(name) = self.symbol_name_at_rom_offset(offset)
            {
                line.symbol = Some(name.to_owned());
            }
        }
    }

    pub(crate) fn summary_fields(&self) -> Option<Vec<(&'static str, String)>> {
        let module = self.modules.first()?;
        Some(vec![
            ("Symbols", module.symbol_count.to_string()),
            ("Format", module.format.clone()),
            ("Identity", module.identity.label().to_owned()),
            ("Sidecar", module.path.display().to_string()),
            ("Warnings", module.diagnostics.len().to_string()),
        ])
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn discover_symbol_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if rom_path.is_absolute() || rom_path.exists() {
        candidates.push(rom_path.with_extension("sym"));
    }
    if let (Some(parent), Some(file_name)) = (source_path.parent(), rom_path.file_name()) {
        candidates.push(parent.join(file_name).with_extension("sym"));
    }
    candidates.push(source_path.with_extension("sym"));

    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
        let Some(parent) = candidate.parent() else {
            continue;
        };
        let Some(wanted) = candidate.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(parent) else {
            continue;
        };
        if let Some(path) = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
            })
        {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zeff-symbol-session-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn discovers_regular_and_archived_rom_sidecars() {
        let dir = temp_dir();
        let rom = dir.join("game.gbc");
        let sym = dir.join("game.SYM");
        std::fs::write(&rom, []).unwrap();
        std::fs::write(&sym, b"00:0150 Entry").unwrap();
        assert_eq!(
            std::fs::canonicalize(discover_symbol_sidecar(&rom, &rom).unwrap()).unwrap(),
            std::fs::canonicalize(&sym).unwrap()
        );

        let archive = dir.join("collection.zip");
        assert_eq!(
            std::fs::canonicalize(
                discover_symbol_sidecar(&archive, Path::new("game.gbc")).unwrap()
            )
            .unwrap(),
            std::fs::canonicalize(sym).unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn annotates_disassembly_by_physical_rom_offset() {
        let mut session = SymbolSession::default();
        let module = import_symbols(
            "game.sym",
            b"02:4560 UpdatePlayer\n00:C100 PlayerData",
            &ImportContext {
                target: TargetInfo {
                    system: zeff_emu_common::system::System::Gb,
                },
                image: ImageId(0),
                rom_region: RegionId(0),
                cpu_space: AddressSpaceId(0),
                source_name: None,
            },
        )
        .unwrap();
        session.store.extend(module.symbols);
        let mut view = crate::debug::DisassemblyView {
            pc: 0x4560,
            mapping: Some(2),
            is_navigation_target: false,
            lines: vec![crate::debug::DisassembledLine {
                address: 0x4560,
                storage_offset: Some(0x8560),
                symbol: None,
                bytes: Default::default(),
                mnemonic: Default::default(),
            }],
            breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
        };

        session.annotate_disassembly(&mut view);
        assert_eq!(view.lines[0].symbol.as_deref(), Some("UpdatePlayer"));
        assert_eq!(session.resolve_rom_name("updateplayer"), Some(0x8560));
        assert_eq!(session.resolve_cpu_name("PlayerData"), Some(0xC100));
        assert_eq!(
            session.symbol_name_at_cpu_address(0xC100),
            Some("PlayerData")
        );
    }
}
