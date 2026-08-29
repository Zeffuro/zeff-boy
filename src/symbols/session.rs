use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use zeff_emu_common::system::System;

use super::identity::IdentityStatus;
use super::identity::RomIdentity;
use super::store::SymbolStore;
use super::{
    AddressSpaceId, CpuLocation, DebugSegment, ImageId, LoadInstance, ProvenanceKind, RegionId,
    SourceFile, SourceLine, StorageLocation, SymbolId, SymbolKind, SymbolRecord,
};

#[cfg(not(target_arch = "wasm32"))]
mod discovery;
mod loading;
mod source;

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
            Some(System::Coleco) => super::ExecMode::Z80,
            Some(System::Pce) => super::ExecMode::Unknown,
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

    pub(crate) fn symbol_name_at_debug_location(
        &self,
        location: super::ResolvedDebugLocation,
    ) -> Option<&str> {
        location
            .storage
            .and_then(|storage| {
                self.store
                    .lookup_storage(storage)
                    .max_by_key(|symbol| symbol_priority(symbol.provenance.kind))
            })
            .or_else(|| {
                self.store
                    .lookup_cpu_mapped(location.cpu, location.bank)
                    .max_by_key(|symbol| {
                        (
                            symbol_priority(symbol.provenance.kind),
                            symbol.location.bank == location.bank,
                        )
                    })
            })
            .map(|symbol| symbol.name.as_str())
    }

    pub(crate) fn unique_symbol_name_at_debug_location(
        &self,
        location: super::ResolvedDebugLocation,
    ) -> Option<&str> {
        if let Some(storage) = location.storage {
            let mut symbols = self.store.lookup_storage(storage);
            if let Some(symbol) = symbols.next() {
                return symbols.next().is_none().then_some(symbol.name.as_str());
            }
        }
        let mut symbols = self.store.lookup_cpu_mapped(location.cpu, location.bank);
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
            self.store
                .lookup_cpu_mapped(cpu, location.bank)
                .any(|symbol| {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
