#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;

use crate::emu_backend::EmuBackend;
use anyhow::Context;
use zeff_emu_common::system::System;

use super::identity::IdentityStatus;
use super::identity::RomIdentity;
#[cfg(not(target_arch = "wasm32"))]
use super::import::{ImportContext, TargetInfo, import_symbols};
use super::store::SymbolStore;
use super::{
    AddressSpaceId, Confidence, CpuLocation, ImageId, Provenance, ProvenanceKind, RegionId,
    StorageLocation, SymbolId, SymbolRecord, SymbolScope, UserSymbolDraft,
};

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

impl LoadedSymbolModule {
    pub(crate) fn is_builtin(&self) -> bool {
        self.identity == IdentityStatus::BuiltIn
    }
}

#[derive(Default)]
pub(crate) struct SymbolSession {
    pub(crate) store: SymbolStore,
    pub(crate) modules: Vec<LoadedSymbolModule>,
    pub(crate) diagnostics: Vec<String>,
    loading: bool,
    identity: Option<RomIdentity>,
    user_symbols: Vec<SymbolRecord>,
    #[cfg(not(target_arch = "wasm32"))]
    user_path: Option<PathBuf>,
}

impl SymbolSession {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_for_backend(backend: &EmuBackend) -> Self {
        Self::load_for_paths(
            backend.system(),
            backend.rom_path(),
            backend.source_path(),
            backend.rom_hash(),
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_for_paths(
        system: System,
        rom_path: &Path,
        source_path: &Path,
        rom_sha256: [u8; 32],
    ) -> Self {
        Self::load_sidecar(system, rom_path, source_path, rom_sha256)
    }

    pub(crate) fn loading() -> Self {
        Self {
            loading: true,
            ..Self::default()
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn load_for_backend(backend: &EmuBackend) -> Self {
        let mut session = Self {
            identity: Some(RomIdentity {
                system: backend.system(),
                sha256: backend.rom_hash(),
            }),
            ..SymbolSession::default()
        };
        session.load_platform_symbols(backend.system());
        session
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_sidecar(
        system: System,
        rom_path: &Path,
        source_path: &Path,
        rom_sha256: [u8; 32],
    ) -> Self {
        let identity = RomIdentity {
            system,
            sha256: rom_sha256,
        };
        let mut session = Self {
            identity: Some(identity),
            user_path: Some(user_symbol_sidecar_path(source_path, rom_path)),
            ..Self::default()
        };
        let mut labels_loaded = false;
        if let Some(path) = discover_zdbg_sidecar(source_path, rom_path) {
            labels_loaded = session.load_zdbg_sidecar(path, identity);
        }
        if let Some(path) = discover_symbol_sidecar(source_path, rom_path) {
            labels_loaded |= session.load_imported_sidecar(path, identity, labels_loaded);
        }
        if let Some(path) = discover_map_sidecar(source_path, rom_path) {
            session.load_imported_sidecar(path, identity, labels_loaded);
        }
        if let Some(path) = discover_dbg_sidecar(source_path, rom_path) {
            session.load_imported_sidecar(path, identity, false);
        }
        for path in discover_namelist_sidecars(system, source_path, rom_path) {
            session.load_imported_sidecar(path, identity, false);
        }
        session.load_user_sidecar();
        session.load_platform_symbols(system);
        session
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_zdbg_sidecar(&mut self, path: PathBuf, identity: RomIdentity) -> bool {
        match super::user::load_zdbg_symbols(&path, identity) {
            Ok(symbols) => {
                let supplied_labels = symbols
                    .iter()
                    .any(|symbol| symbol.kind != super::SymbolKind::Section);
                let count = symbols.len();
                self.store.extend(symbols);
                self.modules.push(LoadedSymbolModule {
                    path,
                    format: "Zeff Debug Symbols".to_owned(),
                    identity: IdentityStatus::Exact,
                    symbol_count: count,
                    diagnostics: Vec::new(),
                });
                supplied_labels
            }
            Err(error) => {
                self.diagnostics
                    .push(format!("{}: {error}", path.display()));
                false
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_imported_sidecar(
        &mut self,
        path: PathBuf,
        identity: RomIdentity,
        sections_only: bool,
    ) -> bool {
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
                    target: TargetInfo {
                        system: identity.system,
                    },
                    image: ImageId(0),
                    rom_region: RegionId(0),
                    cpu_space: AddressSpaceId(0),
                    source_name: Some(path.display().to_string()),
                },
            )?;
            let supplied_labels = module
                .symbols
                .iter()
                .any(|symbol| symbol.kind != super::SymbolKind::Section);
            if sections_only {
                module
                    .symbols
                    .retain(|symbol| symbol.kind == super::SymbolKind::Section);
            }
            let count = module.symbols.len();
            self.store.extend(module.symbols.drain(..));
            Ok((
                LoadedSymbolModule {
                    path: path.clone(),
                    format: module.format,
                    identity: IdentityStatus::compare(None, identity),
                    symbol_count: count,
                    diagnostics: module.diagnostics,
                },
                supplied_labels,
            ))
        })();
        match result {
            Ok((module, supplied_labels)) => {
                log::info!(
                    "Loaded {} symbols from {} ({})",
                    module.symbol_count,
                    module.path.display(),
                    module.identity.label()
                );
                self.modules.push(module);
                supplied_labels
            }
            Err(error) => {
                self.diagnostics
                    .push(format!("{}: {error}", path.display()));
                false
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_user_sidecar(&mut self) {
        let (Some(path), Some(identity)) = (&self.user_path, self.identity) else {
            return;
        };
        if !path.is_file() {
            return;
        }
        match super::user::load_user_symbols(path, identity) {
            Ok(symbols) => {
                log::info!(
                    "Loaded {} user symbols from {}",
                    symbols.len(),
                    path.display()
                );
                self.user_symbols = symbols;
                self.store.replace_user_symbols(self.user_symbols.clone());
            }
            Err(error) => self
                .diagnostics
                .push(format!("{}: {error}", path.display())),
        }
    }

    fn load_platform_symbols(&mut self, system: System) {
        let symbols = super::platform::symbols(system);
        if symbols.is_empty() {
            return;
        }
        let count = symbols.len();
        self.store.extend(symbols);
        self.modules.push(LoadedSymbolModule {
            path: PathBuf::from("Built-in"),
            format: "Platform labels".to_owned(),
            identity: IdentityStatus::BuiltIn,
            symbol_count: count,
            diagnostics: Vec::new(),
        });
    }

    pub(crate) fn upsert_user_symbol(
        &mut self,
        draft: UserSymbolDraft,
    ) -> anyhow::Result<Option<PathBuf>> {
        anyhow::ensure!(!draft.name.trim().is_empty(), "symbol name is empty");
        anyhow::ensure!(draft.name.len() <= 256, "symbol name is too long");
        anyhow::ensure!(
            !draft.name.chars().any(char::is_control),
            "symbol name contains a control character"
        );
        anyhow::ensure!(
            draft.location.cpu.is_some()
                || draft.location.storage.is_some()
                || draft.value.is_some(),
            "symbol has no location or value"
        );
        let identity = self.identity.context("no ROM is loaded")?;
        let source = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.user_path
                    .as_ref()
                    .map(|path| path.display().to_string())
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        };
        let record = SymbolRecord {
            id: SymbolId(0),
            name: draft.name,
            location: draft.location,
            value: draft.value,
            size: draft.size,
            kind: draft.kind,
            scope: SymbolScope::Global,
            provenance: Provenance {
                kind: ProvenanceKind::User,
                source,
            },
            confidence: Confidence::Exact,
            comment: draft.comment,
        };
        let mut symbols = self.user_symbols.clone();
        symbols.retain(|symbol| !symbol.name.eq_ignore_ascii_case(&record.name));
        symbols.push(record);

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = &self.user_path {
            super::user::save_user_symbols(path, identity, &symbols)?;
        }

        self.user_symbols = symbols;
        self.store.replace_user_symbols(self.user_symbols.clone());
        #[cfg(not(target_arch = "wasm32"))]
        return Ok(self.user_path.clone());
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }

    pub(crate) fn remove_user_symbol(&mut self, name: &str) -> anyhow::Result<Option<PathBuf>> {
        let identity = self.identity.context("no ROM is loaded")?;
        let mut symbols = self.user_symbols.clone();
        let original_len = symbols.len();
        symbols.retain(|symbol| !symbol.name.eq_ignore_ascii_case(name));
        anyhow::ensure!(symbols.len() != original_len, "no user label named {name}");

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = &self.user_path {
            super::user::save_user_symbols(path, identity, &symbols)?;
        }

        self.user_symbols = symbols;
        self.store.replace_user_symbols(self.user_symbols.clone());
        #[cfg(not(target_arch = "wasm32"))]
        return Ok(self.user_path.clone());
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }

    pub(crate) fn symbol_count(&self) -> usize {
        self.store.len()
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(crate) fn exec_mode(&self) -> super::ExecMode {
        match self.identity.map(|identity| identity.system) {
            Some(System::Gb) => super::ExecMode::Sm83,
            Some(System::Gba) => super::ExecMode::Arm,
            Some(System::Nes) => super::ExecMode::Mos6502,
            Some(System::Ws) => super::ExecMode::V30,
            Some(System::Sms | System::Gg | System::Sg) => super::ExecMode::Z80,
            None => super::ExecMode::Unknown,
        }
    }

    pub(crate) fn symbol_name_at_rom_offset(&self, offset: u64) -> Option<&str> {
        self.store
            .lookup_storage(StorageLocation {
                image: ImageId(0),
                region: RegionId(0),
                offset,
            })
            .max_by_key(|symbol| symbol_priority(symbol.provenance.kind))
            .map(|symbol| symbol.name.as_str())
    }

    pub(crate) fn symbol_name_at_cpu_address(&self, address: u64) -> Option<&str> {
        self.store
            .lookup_cpu(CpuLocation {
                space: AddressSpaceId(0),
                address,
            })
            .max_by_key(|symbol| symbol_priority(symbol.provenance.kind))
            .map(|symbol| symbol.name.as_str())
    }

    fn unique_symbol_name_at_cpu_address(&self, address: u64) -> Option<&str> {
        let mut symbols = self.store.lookup_cpu(CpuLocation {
            space: AddressSpaceId(0),
            address,
        });
        let symbol = symbols.next()?;
        symbols.next().is_none().then_some(symbol.name.as_str())
    }

    pub(crate) fn resolve_cpu_name(&self, name: &str) -> Option<u64> {
        self.store
            .lookup_name(name)
            .chain(self.store.lookup_name_case_insensitive(name))
            .filter_map(|symbol| {
                symbol
                    .location
                    .cpu
                    .map(|location| (symbol_priority(symbol.provenance.kind), location.address))
            })
            .max_by_key(|(priority, _)| *priority)
            .map(|(_, address)| address)
    }

    pub(crate) fn resolve_rom_name(&self, name: &str) -> Option<u64> {
        self.store
            .lookup_name(name)
            .chain(self.store.lookup_name_case_insensitive(name))
            .filter_map(|symbol| {
                symbol
                    .location
                    .storage
                    .map(|location| (symbol_priority(symbol.provenance.kind), location.offset))
            })
            .max_by_key(|(priority, _)| *priority)
            .map(|(_, offset)| offset)
    }

    pub(crate) fn annotate_disassembly(&self, view: &mut crate::debug::DisassemblyView) {
        for line in &mut view.lines {
            if line.symbol.is_none()
                && let Some(name) = line
                    .storage_offset
                    .and_then(|offset| self.symbol_name_at_rom_offset(offset))
                    .or_else(|| self.unique_symbol_name_at_cpu_address(line.address.into()))
            {
                line.symbol = Some(name.to_owned());
            }
            if line.control_target_symbol.is_none() {
                let name = line
                    .control_target_storage
                    .and_then(|offset| self.symbol_name_at_rom_offset(offset))
                    .or_else(|| {
                        line.control_target
                            .and_then(|address| self.symbol_name_at_cpu_address(address.into()))
                    });
                if let Some(name) = name {
                    line.control_target_symbol = Some(name.to_owned());
                }
            }
        }
    }

    pub(crate) fn summary_fields(&self) -> Option<Vec<(&'static str, String)>> {
        if self.loading {
            return Some(vec![("Symbols", "Loading...".to_owned())]);
        }
        let mut fields = if let Some(module) = self
            .modules
            .iter()
            .find(|module| module.identity != IdentityStatus::BuiltIn)
            .or_else(|| self.modules.first())
        {
            vec![
                ("Symbols", module.symbol_count.to_string()),
                ("Format", module.format.clone()),
                ("Identity", module.identity.label().to_owned()),
                ("Sidecar", module.path.display().to_string()),
                ("Warnings", module.diagnostics.len().to_string()),
            ]
        } else if !self.user_symbols.is_empty() {
            vec![("Symbols", "0".to_owned())]
        } else {
            return None;
        };
        if !self.user_symbols.is_empty() {
            fields.push(("User labels", self.user_symbols.len().to_string()));
        }
        Some(fields)
    }
}

fn symbol_priority(kind: ProvenanceKind) -> u8 {
    match kind {
        ProvenanceKind::User => 4,
        ProvenanceKind::Build
        | ProvenanceKind::DebugFormat
        | ProvenanceKind::LinkMap
        | ProvenanceKind::ReverseEngineering => 3,
        ProvenanceKind::RuntimeInference => 2,
        ProvenanceKind::Platform => 1,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn discover_symbol_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "sym")
}

#[cfg(not(target_arch = "wasm32"))]
fn discover_zdbg_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "zdbg.json")
}

#[cfg(not(target_arch = "wasm32"))]
fn discover_map_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "map")
}

#[cfg(not(target_arch = "wasm32"))]
fn discover_dbg_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "dbg")
}

#[cfg(not(target_arch = "wasm32"))]
fn discover_namelist_sidecars(system: System, source_path: &Path, rom_path: &Path) -> Vec<PathBuf> {
    if system != System::Nes {
        return Vec::new();
    }
    let (Some(parent), Some(file_name)) = (source_path.parent(), rom_path.file_name()) else {
        return Vec::new();
    };
    let Some(file_name) = file_name.to_str() else {
        return Vec::new();
    };
    let prefix = format!("{}.", file_name.to_ascii_lowercase());
    let mut paths = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        let name = name.to_ascii_lowercase();
                        name.starts_with(&prefix) && name.ends_with(".nl")
                    })
        })
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths
}

#[cfg(not(target_arch = "wasm32"))]
fn discover_sidecar_with_extension(
    source_path: &Path,
    rom_path: &Path,
    extension: &str,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if rom_path.is_absolute() || rom_path.exists() {
        candidates.push(rom_path.with_extension(extension));
    }
    if let (Some(parent), Some(file_name)) = (source_path.parent(), rom_path.file_name()) {
        candidates.push(parent.join(file_name).with_extension(extension));
    }
    candidates.push(source_path.with_extension(extension));

    for candidate in &candidates {
        if candidate.is_file() {
            return Some(candidate.clone());
        }
    }

    #[cfg(windows)]
    {
        None
    }

    #[cfg(not(windows))]
    {
        for candidate in candidates {
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
}

#[cfg(not(target_arch = "wasm32"))]
fn user_symbol_sidecar_path(source_path: &Path, rom_path: &Path) -> PathBuf {
    if rom_path.is_absolute() || rom_path.exists() {
        return rom_path.with_extension("user.zdbg.json");
    }
    if let (Some(parent), Some(file_name)) = (source_path.parent(), rom_path.file_name()) {
        return parent.join(file_name).with_extension("user.zdbg.json");
    }
    source_path.with_extension("user.zdbg.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{ExecMode, SymbolKind, SymbolLocation};
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

        let map = dir.join("game.map");
        std::fs::write(&map, b"ROM0 bank #0:").unwrap();
        assert_eq!(
            std::fs::canonicalize(discover_map_sidecar(&archive, Path::new("game.gbc")).unwrap())
                .unwrap(),
            std::fs::canonicalize(map).unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loads_nocash_gba_sidecar() {
        let dir = temp_dir();
        let rom = dir.join("game.gba");
        let sym = dir.join("game.sym");
        std::fs::write(&rom, []).unwrap();
        std::fs::write(&sym, b"080001EC .thumb\n080001EC InitVideo").unwrap();

        let session =
            SymbolSession::load_sidecar(zeff_emu_common::system::System::Gba, &rom, &rom, [0; 32]);
        assert_eq!(session.modules[0].symbol_count, 1);
        assert_eq!(session.modules[0].format, "no$gba .sym");
        assert_eq!(session.resolve_rom_name("InitVideo"), Some(0x1EC));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loads_gnu_nm_gba_sidecar() {
        let dir = temp_dir();
        let rom = dir.join("game.gba");
        let sym = dir.join("game.sym");
        std::fs::write(&rom, []).unwrap();
        std::fs::write(&sym, b"08000204 g 00000020 Init\n02000000 l 0001c000 gHeap").unwrap();

        let session = SymbolSession::load_sidecar(System::Gba, &rom, &rom, [0; 32]);
        assert_eq!(session.modules[0].format, "GNU nm .sym");
        assert_eq!(session.modules[0].symbol_count, 2);
        assert_eq!(session.resolve_rom_name("Init"), Some(0x204));
        assert_eq!(session.resolve_cpu_name("gHeap"), Some(0x0200_0000));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loads_external_gnu_nm_session_when_configured() {
        let Ok(path) = std::env::var("ZEFF_TEST_GNU_NM_SYM") else {
            return;
        };
        let sym = PathBuf::from(path);
        let rom = sym.with_extension("gba");
        let started = std::time::Instant::now();
        let session = SymbolSession::load_sidecar(System::Gba, &rom, &rom, [0; 32]);
        eprintln!(
            "external GNU nm session: {} ms",
            started.elapsed().as_millis()
        );
        assert_eq!(session.modules[0].format, "GNU nm .sym");
        assert!(session.modules[0].symbol_count > 70_000);
    }

    #[test]
    fn discovers_fceux_namelists_for_nes() {
        let dir = temp_dir();
        let rom = dir.join("contra.nes");
        std::fs::write(&rom, []).unwrap();
        std::fs::write(dir.join("contra.nes.0.nl"), b"$8000#Start#").unwrap();
        std::fs::write(dir.join("contra.nes.ram.nl"), b"$0010#Frame#").unwrap();
        let paths = discover_namelist_sidecars(System::Nes, &rom, &rom);
        assert_eq!(paths.len(), 2);
        let session = SymbolSession::load_sidecar(System::Nes, &rom, &rom, [0; 32]);
        assert_eq!(
            session
                .modules
                .iter()
                .filter(|module| !module.is_builtin())
                .map(|module| module.symbol_count)
                .sum::<usize>(),
            2
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn map_supplements_sym_with_sections_only() {
        let dir = temp_dir();
        let rom = dir.join("game.gbc");
        std::fs::write(&rom, []).unwrap();
        std::fs::write(dir.join("game.sym"), b"02:4560 UpdatePlayer").unwrap();
        std::fs::write(
            dir.join("game.map"),
            b"ROMX bank #2:\n\tSECTION: $4560-$457f ($0020 bytes) [\"Player code\"]\n\t         $4560 = MapCopy",
        )
        .unwrap();

        let session = SymbolSession::load_sidecar(System::Gb, &rom, &rom, [0; 32]);
        assert_eq!(session.modules.len(), 3);
        assert_eq!(
            session
                .modules
                .iter()
                .filter(|module| !module.is_builtin())
                .map(|module| module.symbol_count)
                .sum::<usize>(),
            2
        );
        assert_eq!(session.resolve_cpu_name("UpdatePlayer"), Some(0x4560));
        assert_eq!(session.resolve_cpu_name("MapCopy"), None);
        assert_eq!(
            session
                .store
                .lookup_name("Player code")
                .next()
                .unwrap()
                .size,
            Some(0x20)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn map_labels_are_a_fallback_without_sym() {
        let dir = temp_dir();
        let rom = dir.join("game.gbc");
        std::fs::write(&rom, []).unwrap();
        std::fs::write(
            dir.join("game.map"),
            b"ROMX bank #2:\n\tSECTION: $4560-$457f ($0020 bytes) [\"Player code\"]\n\t         $4560 = UpdatePlayer",
        )
        .unwrap();

        let session = SymbolSession::load_sidecar(System::Gb, &rom, &rom, [0; 32]);
        assert_eq!(session.resolve_cpu_name("UpdatePlayer"), Some(0x4560));
        assert_eq!(
            session
                .modules
                .iter()
                .filter(|module| !module.is_builtin())
                .map(|module| module.symbol_count)
                .sum::<usize>(),
            2
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saves_and_reloads_user_labels_without_a_build_sidecar() {
        let dir = temp_dir();
        let rom = dir.join("game.gbc");
        std::fs::write(&rom, []).unwrap();
        let hash = [7; 32];
        let mut session = SymbolSession::load_sidecar(System::Gb, &rom, &rom, hash);
        let path = session
            .upsert_user_symbol(UserSymbolDraft {
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
                kind: SymbolKind::Label,
                size: None,
                comment: Some("Movement update".to_owned()),
            })
            .unwrap()
            .unwrap();
        assert_eq!(path, rom.with_extension("user.zdbg.json"));

        let loaded = SymbolSession::load_sidecar(System::Gb, &rom, &rom, hash);
        assert_eq!(
            loaded
                .modules
                .iter()
                .filter(|module| !module.is_builtin())
                .map(|module| module.symbol_count)
                .sum::<usize>(),
            0
        );
        assert_eq!(
            loaded.symbol_count(),
            1 + super::super::platform::symbols(System::Gb).len()
        );
        assert_eq!(
            loaded.symbol_name_at_rom_offset(0x8560),
            Some("UpdatePlayer")
        );
        assert_eq!(
            loaded
                .store
                .lookup_name("UpdatePlayer")
                .next()
                .unwrap()
                .comment
                .as_deref(),
            Some("Movement update")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loads_exact_hash_zdbg_before_other_symbol_files() {
        let dir = temp_dir();
        let rom = dir.join("game.gbc");
        let zdbg = dir.join("game.zdbg.json");
        std::fs::write(&rom, []).unwrap();
        let identity = RomIdentity {
            system: System::Gb,
            sha256: [9; 32],
        };
        let mut source = SymbolSession {
            identity: Some(identity),
            user_path: Some(zdbg.clone()),
            ..SymbolSession::default()
        };
        source
            .upsert_user_symbol(UserSymbolDraft {
                name: "ExactLabel".to_owned(),
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
                kind: SymbolKind::Function,
                size: Some(8),
                comment: None,
            })
            .unwrap();
        std::fs::write(dir.join("game.sym"), b"02:4560 SymLabel").unwrap();

        let loaded = SymbolSession::load_sidecar(System::Gb, &rom, &rom, [9; 32]);
        assert_eq!(loaded.modules[0].format, "Zeff Debug Symbols");
        assert_eq!(loaded.resolve_cpu_name("ExactLabel"), Some(0x4560));
        assert_eq!(loaded.resolve_cpu_name("SymLabel"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn annotates_disassembly_by_physical_rom_offset() {
        let mut session = SymbolSession::default();
        let module = import_symbols(
            "game.sym",
            b"02:4560 UpdatePlayer\n02:4660 UpdateNpc\n00:C100 PlayerData",
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
                control_target: Some(0x4660),
                control_target_storage: Some(0x8660),
                control_target_symbol: None,
                bytes: Default::default(),
                mnemonic: Default::default(),
            }],
            breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
        };

        session.annotate_disassembly(&mut view);
        assert_eq!(view.lines[0].symbol.as_deref(), Some("UpdatePlayer"));
        assert_eq!(
            view.lines[0].control_target_symbol.as_deref(),
            Some("UpdateNpc")
        );
        assert_eq!(session.resolve_rom_name("updateplayer"), Some(0x8560));
        assert_eq!(session.resolve_cpu_name("PlayerData"), Some(0xC100));
        assert_eq!(
            session.symbol_name_at_cpu_address(0xC100),
            Some("PlayerData")
        );
    }
}
