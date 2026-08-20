use std::collections::{BTreeMap, HashMap};
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
    AddressSpaceId, Confidence, CpuLocation, DebugSegment, ImageId, LoadInstance, Provenance,
    ProvenanceKind, RegionId, ResolvedLoadInstance, SegmentId, SourceFile, SourceLine,
    StorageLocation, SymbolId, SymbolKind, SymbolRecord, SymbolScope, UserSymbolDraft,
};

#[cfg(not(target_arch = "wasm32"))]
mod discovery;
#[cfg(not(target_arch = "wasm32"))]
use discovery::{
    discover_dbg_sidecar, discover_elf_sidecar, discover_map_sidecar, discover_namelist_sidecars,
    discover_symbol_sidecar, discover_zdbg_sidecar, user_symbol_sidecar_path,
};

#[cfg(not(target_arch = "wasm32"))]
const MAX_SYMBOL_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct LoadedSymbolModule {
    pub(crate) path: PathBuf,
    pub(crate) format: String,
    pub(crate) identity: IdentityStatus,
    pub(crate) symbol_count: usize,
    pub(crate) source_line_count: usize,
    pub(crate) diagnostics: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct SourceReference {
    pub(crate) source_file: usize,
    pub(crate) path: PathBuf,
    pub(crate) display_path: String,
    pub(crate) line: u32,
    pub(crate) crc32: Option<u32>,
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
    source_files: Vec<SourceFile>,
    source_roots: Vec<Option<PathBuf>>,
    source_by_storage: BTreeMap<StorageLocation, SourceLine>,
    source_by_cpu: BTreeMap<CpuLocation, SourceLine>,
    source_offsets_by_line: HashMap<(usize, u32), Vec<u64>>,
    source_addresses_by_line: HashMap<(usize, u32), Vec<zeff_emu_common::address::Address>>,
    loading: bool,
    identity: Option<RomIdentity>,
    user_symbols: Vec<SymbolRecord>,
    segments: Vec<DebugSegment>,
    load_instances: Vec<LoadInstance>,
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_for_paths_with_sidecar(
        system: System,
        rom_path: &Path,
        source_path: &Path,
        rom_sha256: [u8; 32],
        sidecar_path: &Path,
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
        let selected_user_path = session.user_path.as_deref() == Some(sidecar_path);
        if sidecar_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".zdbg.json"))
        {
            session.load_zdbg_sidecar(sidecar_path.to_path_buf(), identity);
        } else {
            session.load_imported_sidecar(sidecar_path.to_path_buf(), identity, false);
        }
        if !selected_user_path {
            session.load_user_sidecar();
        }
        session.load_platform_symbols(system);
        session
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
        if system == System::Gba
            && let Some(path) = discover_elf_sidecar(source_path, rom_path)
        {
            labels_loaded |= session.load_imported_sidecar(path, identity, labels_loaded);
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
            Ok(loaded) => {
                let supplied_labels = loaded
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind != super::SymbolKind::Section);
                let count = loaded.symbols.len();
                self.store.extend(loaded.symbols);
                self.segments.extend(loaded.segments);
                self.load_instances.extend(loaded.load_instances);
                self.modules.push(LoadedSymbolModule {
                    path,
                    format: "Zeff Debug Symbols".to_owned(),
                    identity: IdentityStatus::Exact,
                    symbol_count: count,
                    source_line_count: 0,
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
            let source_line_count = module.source_lines.len();
            self.extend_source_metadata(
                module.source_files,
                module.source_lines,
                path.parent().map(Path::to_path_buf),
            );
            self.store.extend(module.symbols.drain(..));
            Ok((
                LoadedSymbolModule {
                    path: path.clone(),
                    format: module.format,
                    identity: IdentityStatus::compare(None, identity),
                    symbol_count: count,
                    source_line_count,
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
            source_line_count: 0,
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

    pub(crate) fn symbol_context_at_rom_offset(&self, offset: u64) -> Option<String> {
        let location = StorageLocation {
            image: ImageId(0),
            region: RegionId(0),
            offset,
        };
        let symbol = self
            .store
            .lookup_storage_containing(location)
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    SymbolKind::Function | SymbolKind::Label | SymbolKind::Section
                )
            })
            .max_by_key(|symbol| {
                let kind = match symbol.kind {
                    SymbolKind::Function => 3,
                    SymbolKind::Label => 2,
                    SymbolKind::Section => 1,
                    _ => 0,
                };
                let start = symbol
                    .location
                    .storage
                    .map_or(0, |location| location.offset);
                (kind, symbol_priority(symbol.provenance.kind), start)
            })?;
        let start = symbol.location.storage?.offset;
        let delta = offset.saturating_sub(start);
        Some(if delta == 0 {
            symbol.name.clone()
        } else {
            format!("{}+${delta:X}", symbol.name)
        })
    }

    pub(crate) fn symbol_name_at_cpu_address(&self, address: u64) -> Option<&str> {
        let direct = self
            .store
            .lookup_cpu(CpuLocation {
                space: AddressSpaceId(0),
                address,
            })
            .max_by_key(|symbol| symbol_priority(symbol.provenance.kind))
            .map(|symbol| symbol.name.as_str());
        direct.or_else(|| {
            let resolved = self.resolve_load_instance(CpuLocation {
                space: AddressSpaceId(0),
                address,
            })?;
            self.store
                .lookup_storage(resolved.storage)
                .max_by_key(|symbol| symbol_priority(symbol.provenance.kind))
                .map(|symbol| symbol.name.as_str())
        })
    }

    pub(crate) fn unique_symbol_name_at_cpu_address(&self, address: u64) -> Option<&str> {
        let cpu = CpuLocation {
            space: AddressSpaceId(0),
            address,
        };
        let mut symbols = self.store.lookup_cpu(cpu);
        if let Some(symbol) = symbols.next() {
            return symbols.next().is_none().then_some(symbol.name.as_str());
        }
        let resolved = self.resolve_load_instance(cpu)?;
        let mut symbols = self.store.lookup_storage(resolved.storage);
        let symbol = symbols.next()?;
        symbols.next().is_none().then_some(symbol.name.as_str())
    }

    pub(crate) fn execution_symbol_ids(
        &self,
        physical_rom_offset: Option<u64>,
        cpu_address: u64,
        exec_mode: super::ExecMode,
    ) -> Vec<SymbolId> {
        if let Some(offset) = physical_rom_offset {
            return self
                .store
                .lookup_code_storage_containing(StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset,
                })
                .map(|symbol| symbol.id)
                .collect();
        }

        let storage = self
            .resolve_load_instance(CpuLocation {
                space: AddressSpaceId(0),
                address: cpu_address,
            })
            .map(|resolved| resolved.storage);
        if let Some(storage) = storage {
            return self
                .store
                .lookup_code_storage_containing(storage)
                .map(|symbol| symbol.id)
                .collect();
        }

        self.store
            .lookup_cpu(CpuLocation {
                space: AddressSpaceId(0),
                address: cpu_address,
            })
            .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Label))
            .filter(|symbol| {
                symbol.location.exec_mode == exec_mode
                    || symbol.location.exec_mode == super::ExecMode::Unknown
            })
            .map(|symbol| symbol.id)
            .collect()
    }

    pub(crate) fn has_code_symbol(&self, location: super::SymbolLocation) -> bool {
        if let Some(storage) = location.storage
            && self
                .store
                .lookup_code_storage_containing(storage)
                .next()
                .is_some()
        {
            return true;
        }
        location.cpu.is_some_and(|cpu| {
            self.store.lookup_cpu(cpu).any(|symbol| {
                matches!(symbol.kind, SymbolKind::Function | SymbolKind::Label)
                    && (symbol.location.exec_mode == location.exec_mode
                        || symbol.location.exec_mode == super::ExecMode::Unknown)
            })
        })
    }

    pub(crate) fn resolve_cpu_name(&self, name: &str) -> Option<u64> {
        self.store
            .lookup_name(name)
            .chain(self.store.lookup_name_case_insensitive(name))
            .filter_map(|symbol| {
                let runtime = symbol
                    .location
                    .storage
                    .and_then(|storage| self.runtime_cpu_for_storage(storage));
                runtime
                    .or(symbol.location.cpu)
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
            if line.storage_offset.is_none() {
                line.storage_offset = self
                    .resolve_load_instance(CpuLocation {
                        space: AddressSpaceId(0),
                        address: line.address.into(),
                    })
                    .map(|resolved| resolved.storage.offset);
            }
            if line.control_target_storage.is_none() {
                line.control_target_storage = line.control_target.and_then(|address| {
                    self.resolve_load_instance(CpuLocation {
                        space: AddressSpaceId(0),
                        address: address.into(),
                    })
                    .map(|resolved| resolved.storage.offset)
                });
            }
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
            if line.source.is_none()
                && let Some(source) = line
                    .storage_offset
                    .and_then(|offset| {
                        self.source_at_storage(StorageLocation {
                            image: ImageId(0),
                            region: RegionId(0),
                            offset,
                        })
                    })
                    .or_else(|| {
                        self.source_at_cpu(CpuLocation {
                            space: AddressSpaceId(0),
                            address: line.address.into(),
                        })
                    })
            {
                line.source = self.format_source_line(source);
            }
        }
        view.location_symbol = view
            .lines
            .iter()
            .find(|line| line.address == view.pc)
            .and_then(|line| {
                line.symbol.clone().or_else(|| {
                    line.storage_offset
                        .and_then(|offset| self.symbol_context_at_rom_offset(offset))
                })
            });
    }

    pub(crate) fn source_reference_for_disassembly(
        &self,
        view: &crate::debug::DisassemblyView,
    ) -> Option<SourceReference> {
        let line = view.lines.iter().find(|line| line.address == view.pc)?;
        let source = line
            .storage_offset
            .and_then(|offset| {
                self.source_at_storage(StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset,
                })
            })
            .or_else(|| {
                self.source_at_cpu(CpuLocation {
                    space: AddressSpaceId(0),
                    address: line.address.into(),
                })
            })?;
        self.source_reference(source)
    }

    pub(crate) fn source_reference_at_rom_offset(&self, offset: u64) -> Option<SourceReference> {
        let source = self.source_at_storage(StorageLocation {
            image: ImageId(0),
            region: RegionId(0),
            offset,
        })?;
        self.source_reference(source)
    }

    pub(crate) fn source_breakpoint_offsets(&self, source_file: usize, line: u32) -> &[u64] {
        self.source_offsets_by_line
            .get(&(source_file, line))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn source_breakpoint_addresses(
        &self,
        source_file: usize,
        line: u32,
    ) -> &[zeff_emu_common::address::Address] {
        self.source_addresses_by_line
            .get(&(source_file, line))
            .map(Vec::as_slice)
            .unwrap_or_default()
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
        if !self.source_by_storage.is_empty() {
            fields.push(("Source lines", self.source_by_storage.len().to_string()));
        }
        if !self.load_instances.is_empty() {
            fields.push(("Load instances", self.load_instances.len().to_string()));
        }
        Some(fields)
    }

    pub(crate) fn segments(&self) -> &[DebugSegment] {
        &self.segments
    }

    pub(crate) fn load_instances(&self) -> &[LoadInstance] {
        &self.load_instances
    }

    pub(crate) fn resolve_load_instance(&self, cpu: CpuLocation) -> Option<ResolvedLoadInstance> {
        self.load_instances
            .iter()
            .filter(|instance| instance.active && instance.runtime_base.space == cpu.space)
            .filter_map(|instance| {
                let segment = self.segment(instance.segment)?;
                let delta = cpu.address.checked_sub(instance.runtime_base.address)?;
                (delta < segment.storage.size).then(|| ResolvedLoadInstance {
                    instance: instance.id,
                    segment: segment.id,
                    cpu,
                    storage: StorageLocation {
                        offset: segment.storage.start.offset + delta,
                        ..segment.storage.start
                    },
                    exec_mode: segment.exec_mode,
                })
            })
            .max_by_key(|resolved| {
                let instance = self
                    .load_instances
                    .iter()
                    .find(|instance| instance.id == resolved.instance)
                    .expect("resolved load instance");
                (instance.generation, instance.created_cycle, instance.id)
            })
    }

    pub(crate) fn runtime_cpu_for_storage(&self, storage: StorageLocation) -> Option<CpuLocation> {
        self.active_runtime_cpu_for_storage(storage).or_else(|| {
            self.segments.iter().find_map(|segment| {
                let linked = segment.linked_cpu?;
                let delta = storage.offset.checked_sub(segment.storage.start.offset)?;
                (storage.image == segment.storage.start.image
                    && storage.region == segment.storage.start.region
                    && delta < segment.storage.size)
                    .then(|| CpuLocation {
                        space: linked.space,
                        address: linked.address + delta,
                    })
            })
        })
    }

    pub(crate) fn active_runtime_cpu_for_storage(
        &self,
        storage: StorageLocation,
    ) -> Option<CpuLocation> {
        self.load_instances
            .iter()
            .filter(|instance| instance.active)
            .filter_map(|instance| {
                let segment = self.segment(instance.segment)?;
                let delta = storage.offset.checked_sub(segment.storage.start.offset)?;
                (storage.image == segment.storage.start.image
                    && storage.region == segment.storage.start.region
                    && delta < segment.storage.size)
                    .then(|| {
                        (
                            instance,
                            CpuLocation {
                                space: instance.runtime_base.space,
                                address: instance.runtime_base.address + delta,
                            },
                        )
                    })
            })
            .max_by_key(|(instance, _)| (instance.generation, instance.created_cycle, instance.id))
            .map(|(_, cpu)| cpu)
    }

    fn segment(&self, id: SegmentId) -> Option<&DebugSegment> {
        self.segments.iter().find(|segment| segment.id == id)
    }

    fn extend_source_metadata(
        &mut self,
        files: Vec<SourceFile>,
        lines: Vec<SourceLine>,
        source_root: Option<PathBuf>,
    ) {
        let file_offset = self.source_files.len();
        self.source_roots
            .extend(std::iter::repeat_n(source_root, files.len()));
        self.source_files.extend(files);
        for mut source in lines {
            source.source_file += file_offset;
            if let Some(storage) = source.location.storage {
                let offsets = self
                    .source_offsets_by_line
                    .entry((source.source_file, source.line))
                    .or_default();
                if !offsets.contains(&storage.offset) {
                    offsets.push(storage.offset);
                }
                self.source_by_storage
                    .entry(storage)
                    .or_insert(source.clone());
            }
            if let Some(cpu) = source.location.cpu {
                if let Ok(address) = zeff_emu_common::address::Address::try_from(cpu.address) {
                    let addresses = self
                        .source_addresses_by_line
                        .entry((source.source_file, source.line))
                        .or_default();
                    if !addresses.contains(&address) {
                        addresses.push(address);
                    }
                }
                self.source_by_cpu.entry(cpu).or_insert(source);
            }
        }
    }

    fn format_source_line(&self, source: &SourceLine) -> Option<String> {
        let file = self.source_files.get(source.source_file)?;
        Some(format!("{}:{}", file.path, source.line))
    }

    fn source_at_storage(&self, location: StorageLocation) -> Option<&SourceLine> {
        let (start, source) = self.source_by_storage.range(..=location).next_back()?;
        (start.image == location.image
            && start.region == location.region
            && location.offset.saturating_sub(start.offset) < source.size)
            .then_some(source)
    }

    fn source_at_cpu(&self, location: CpuLocation) -> Option<&SourceLine> {
        let (start, source) = self.source_by_cpu.range(..=location).next_back()?;
        (start.space == location.space
            && location.address.saturating_sub(start.address) < source.size)
            .then_some(source)
    }

    fn source_reference(&self, source: &SourceLine) -> Option<SourceReference> {
        let file = self.source_files.get(source.source_file)?;
        let raw_path = PathBuf::from(&file.path);
        let path = if raw_path.is_absolute() {
            raw_path
        } else {
            self.source_roots
                .get(source.source_file)
                .and_then(|root| root.as_ref())
                .map_or(raw_path.clone(), |root| root.join(&raw_path))
        };
        Some(SourceReference {
            source_file: source.source_file,
            path,
            display_path: file.path.clone(),
            line: source.line,
            crc32: file.crc32,
        })
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
        let elf = dir.join("game.elf");
        std::fs::write(&elf, b"\x7FELF").unwrap();
        assert_eq!(
            std::fs::canonicalize(discover_elf_sidecar(&archive, Path::new("game.gba")).unwrap())
                .unwrap(),
            std::fs::canonicalize(elf).unwrap()
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
    fn explicit_sidecar_overrides_sibling_discovery() {
        let dir = temp_dir();
        let rom = dir.join("game.gbc");
        let selected = dir.join("selected.sym");
        std::fs::write(&rom, []).unwrap();
        std::fs::write(dir.join("game.sym"), b"01:4000 Sibling").unwrap();
        std::fs::write(&selected, b"02:4560 Selected").unwrap();

        let session =
            SymbolSession::load_for_paths_with_sidecar(System::Gb, &rom, &rom, [0; 32], &selected);
        assert_eq!(session.resolve_cpu_name("Selected"), Some(0x4560));
        assert_eq!(session.resolve_cpu_name("Sibling"), None);
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
        let mut module = import_symbols(
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
        module.symbols[0].size = Some(8);
        session.store.extend(module.symbols);
        let mut view = crate::debug::DisassemblyView {
            pc: 0x4560,
            mapping: Some(2),
            is_navigation_target: false,
            is_static_target: false,
            location_symbol: None,
            lines: vec![crate::debug::DisassembledLine {
                address: 0x4560,
                storage_offset: Some(0x8560),
                symbol: None,
                control_target: Some(0x4660),
                control_target_storage: Some(0x8660),
                control_target_symbol: None,
                source: None,
                bytes: Default::default(),
                mnemonic: Default::default(),
            }],
            breakpoints: Vec::new(),
            one_shot_breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
        };

        session.annotate_disassembly(&mut view);
        assert_eq!(view.lines[0].symbol.as_deref(), Some("UpdatePlayer"));
        assert_eq!(view.location_symbol.as_deref(), Some("UpdatePlayer"));
        assert_eq!(
            view.lines[0].control_target_symbol.as_deref(),
            Some("UpdateNpc")
        );
        assert_eq!(session.resolve_rom_name("updateplayer"), Some(0x8560));
        assert_eq!(
            session.symbol_context_at_rom_offset(0x8562).as_deref(),
            Some("UpdatePlayer+$2")
        );
        assert_eq!(session.resolve_cpu_name("PlayerData"), Some(0xC100));
        assert_eq!(
            session.symbol_name_at_cpu_address(0xC100),
            Some("PlayerData")
        );
    }

    #[test]
    fn annotates_disassembly_with_wla_source_location() {
        let mut session = SymbolSession::default();
        let mut module = import_symbols(
            "game.sym",
            b"[information]\nversion 3\n[source files v2]\n0001:0002 12345678 src/player.asm\n[addr-to-line mapping v2]\n00008560 02:4560 4560 0001:0002:0000002A\n00008564 02:4564 4564 0001:0002:0000002B",
            &ImportContext {
                target: TargetInfo { system: System::Gb },
                image: ImageId(0),
                rom_region: RegionId(0),
                cpu_space: AddressSpaceId(0),
                source_name: None,
            },
        )
        .unwrap();
        session.extend_source_metadata(module.source_files, module.source_lines, None);
        session.store.extend(module.symbols.drain(..));
        let mut view = crate::debug::DisassemblyView {
            pc: 0x4560,
            mapping: Some(2),
            is_navigation_target: false,
            is_static_target: false,
            location_symbol: None,
            lines: vec![crate::debug::DisassembledLine {
                address: 0x4560,
                storage_offset: Some(0x8560),
                symbol: None,
                control_target: None,
                control_target_storage: None,
                control_target_symbol: None,
                source: None,
                bytes: Default::default(),
                mnemonic: Default::default(),
            }],
            breakpoints: Vec::new(),
            one_shot_breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
        };

        session.annotate_disassembly(&mut view);
        assert_eq!(view.lines[0].source.as_deref(), Some("src/player.asm:42"));
        let source = session.source_reference_for_disassembly(&view).unwrap();
        assert_eq!(source.path, PathBuf::from("src/player.asm"));
        assert_eq!(source.line, 42);
        assert_eq!(source.crc32, Some(0x1234_5678));
        assert_eq!(
            session.source_reference_at_rom_offset(0x8563).unwrap().line,
            42
        );
        assert_eq!(
            session.source_reference_at_rom_offset(0x8564).unwrap().line,
            43
        );
        assert_eq!(
            session.source_breakpoint_offsets(source.source_file, 42),
            [0x8560]
        );
        assert_eq!(
            session.source_breakpoint_addresses(source.source_file, 42),
            [0x4560]
        );
    }

    #[test]
    fn resolves_explicit_overlay_instances() {
        let dir = temp_dir();
        let rom = dir.join("game.gbc");
        let sidecar = dir.join("game.zdbg.json");
        std::fs::write(&rom, []).unwrap();
        let symbols = serde_json::json!({
            "format": "zeff-debug-symbols",
            "version": 2,
            "system": "gb",
            "rom_sha256": "0b".repeat(32),
            "symbols": [
                {
                    "name": "OldOverlay",
                    "cpu_address": null,
                    "rom_offset": 32768,
                    "bank": null,
                    "exec_mode": "sm83",
                    "value": null,
                    "size": 16,
                    "kind": "function",
                    "scope": "global",
                    "comment": null
                },
                {
                    "name": "CurrentOverlay",
                    "cpu_address": null,
                    "rom_offset": 36864,
                    "bank": null,
                    "exec_mode": "sm83",
                    "value": null,
                    "size": 16,
                    "kind": "function",
                    "scope": "global",
                    "comment": null
                }
            ],
            "segments": [
                {"id": 1, "name": "old", "rom_offset": 32768, "size": 16,
                 "linked_cpu_address": null, "exec_mode": "sm83"},
                {"id": 2, "name": "current", "rom_offset": 36864, "size": 16,
                 "linked_cpu_address": null, "exec_mode": "sm83"}
            ],
            "load_instances": [
                {"id": 10, "segment_id": 1, "cpu_address": 49152,
                 "generation": 1, "created_cycle": 100, "active": true},
                {"id": 11, "segment_id": 2, "cpu_address": 49152,
                 "generation": 2, "created_cycle": 200, "active": true}
            ]
        });
        std::fs::write(&sidecar, serde_json::to_vec(&symbols).unwrap()).unwrap();

        let session =
            SymbolSession::load_for_paths_with_sidecar(System::Gb, &rom, &rom, [11; 32], &sidecar);
        assert_eq!(session.segments().len(), 2);
        assert_eq!(session.load_instances().len(), 2);
        let resolved = session
            .resolve_load_instance(CpuLocation {
                space: AddressSpaceId(0),
                address: 0xC004,
            })
            .unwrap();
        assert_eq!(resolved.segment, SegmentId(2));
        assert_eq!(resolved.storage.offset, 0x9004);
        assert_eq!(session.resolve_cpu_name("CurrentOverlay"), Some(0xC000));
        assert_eq!(
            session.symbol_name_at_cpu_address(0xC000),
            Some("CurrentOverlay")
        );

        let mut view = crate::debug::DisassemblyView {
            pc: 0xC000,
            mapping: None,
            is_navigation_target: false,
            is_static_target: false,
            location_symbol: None,
            lines: vec![crate::debug::DisassembledLine {
                address: 0xC000,
                storage_offset: None,
                symbol: None,
                control_target: None,
                control_target_storage: None,
                control_target_symbol: None,
                source: None,
                bytes: Default::default(),
                mnemonic: Default::default(),
            }],
            breakpoints: Vec::new(),
            one_shot_breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
        };
        session.annotate_disassembly(&mut view);
        assert_eq!(view.lines[0].storage_offset, Some(0x9000));
        assert_eq!(view.lines[0].symbol.as_deref(), Some("CurrentOverlay"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
