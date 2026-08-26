use std::collections::HashMap;

use smallvec::SmallVec;

use super::{CpuLocation, ProvenanceKind, StorageLocation, SymbolId, SymbolKind, SymbolRecord};

#[derive(Default)]
pub(crate) struct SymbolStore {
    symbols: Vec<SymbolRecord>,
    name_index: HashMap<String, SymbolIds>,
    cpu_index: HashMap<CpuLocation, SymbolIds>,
    storage_index: Vec<(StorageLocation, SymbolIds)>,
    code_storage_index: Vec<CodeStorageEntry>,
    generation: u64,
}

#[derive(Clone, Copy)]
struct CodeStorageEntry {
    start: StorageLocation,
    end: u64,
    max_end: u64,
    id: SymbolId,
}

type SymbolIds = SmallVec<[SymbolId; 1]>;

impl SymbolStore {
    pub(crate) fn insert(&mut self, mut symbol: SymbolRecord) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        symbol.id = id;
        self.name_index
            .entry(symbol.name.clone())
            .or_default()
            .push(id);
        if let Some(cpu) = symbol.location.cpu {
            self.cpu_index.entry(cpu).or_default().push(id);
        }
        self.insert_storage(symbol.location.storage, id);
        self.symbols.push(symbol);
        self.rebuild_code_storage_index();
        self.generation = self.generation.wrapping_add(1);
        id
    }

    fn insert_storage(&mut self, location: Option<StorageLocation>, id: SymbolId) {
        let Some(location) = location else {
            return;
        };
        match self
            .storage_index
            .binary_search_by_key(&location, |(entry, _)| *entry)
        {
            Ok(index) => self.storage_index[index].1.push(id),
            Err(index) => self.storage_index.insert(index, (location, single_id(id))),
        }
    }

    pub(crate) fn extend(&mut self, symbols: impl IntoIterator<Item = SymbolRecord>) {
        let symbols = symbols.into_iter();
        let (lower, upper) = symbols.size_hint();
        let additional = upper.unwrap_or(lower);
        self.symbols.reserve(additional);
        self.name_index.reserve(additional);
        if additional <= 512 {
            for symbol in symbols {
                self.insert(symbol);
            }
            return;
        }
        let start = self.symbols.len();
        self.symbols
            .extend(symbols.enumerate().map(|(offset, mut symbol)| {
                symbol.id = SymbolId((start + offset) as u32);
                symbol
            }));
        self.rebuild_indices();
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn replace_user_symbols(&mut self, symbols: impl IntoIterator<Item = SymbolRecord>) {
        self.symbols
            .retain(|symbol| symbol.provenance.kind != ProvenanceKind::User);
        self.rebuild_indices();
        for symbol in symbols {
            self.insert(symbol);
        }
        self.generation = self.generation.wrapping_add(1);
    }

    fn rebuild_indices(&mut self) {
        self.name_index.clear();
        self.cpu_index.clear();
        self.storage_index.clear();
        self.code_storage_index.clear();
        self.name_index.reserve(self.symbols.len());
        self.cpu_index.reserve(self.symbols.len());
        let mut storage_entries = Vec::with_capacity(self.symbols.len());
        for (index, symbol) in self.symbols.iter_mut().enumerate() {
            let id = SymbolId(index as u32);
            symbol.id = id;
            self.name_index
                .entry(symbol.name.clone())
                .or_default()
                .push(id);
            if let Some(cpu) = symbol.location.cpu {
                self.cpu_index.entry(cpu).or_default().push(id);
            }
            if let Some(storage) = symbol.location.storage {
                storage_entries.push((storage, id));
            }
        }
        storage_entries.sort_unstable_by_key(|(location, _)| *location);
        self.storage_index.reserve(storage_entries.len());
        for (location, id) in storage_entries {
            if let Some((last_location, ids)) = self.storage_index.last_mut()
                && *last_location == location
            {
                ids.push(id);
            } else {
                self.storage_index.push((location, single_id(id)));
            }
        }
        self.rebuild_code_storage_index();
    }

    fn rebuild_code_storage_index(&mut self) {
        self.code_storage_index.clear();
        self.code_storage_index.extend(
            self.symbols
                .iter()
                .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Label))
                .filter_map(|symbol| {
                    let start = symbol.location.storage?;
                    Some(CodeStorageEntry {
                        start,
                        end: start.offset.saturating_add(symbol.size.unwrap_or(1).max(1)),
                        max_end: 0,
                        id: symbol.id,
                    })
                }),
        );
        self.code_storage_index
            .sort_unstable_by_key(|entry| entry.start);
        let mut previous = None;
        let mut max_end = 0;
        for entry in &mut self.code_storage_index {
            let group = (entry.start.image, entry.start.region);
            if previous != Some(group) {
                previous = Some(group);
                max_end = 0;
            }
            max_end = max_end.max(entry.end);
            entry.max_end = max_end;
        }
    }

    pub(crate) fn symbol(&self, id: SymbolId) -> Option<&SymbolRecord> {
        self.symbols.get(id.0 as usize)
    }

    pub(crate) fn lookup_name(&self, name: &str) -> impl Iterator<Item = &SymbolRecord> {
        self.name_index
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|id| self.symbol(*id))
    }

    pub(crate) fn lookup_name_case_insensitive(
        &self,
        name: &str,
    ) -> impl Iterator<Item = &SymbolRecord> {
        self.symbols
            .iter()
            .filter(move |symbol| symbol.name.eq_ignore_ascii_case(name))
    }

    pub(crate) fn search_ids(&self, query: &str, limit: usize) -> Vec<SymbolId> {
        self.search_ids_matching(query, limit, |_| true)
    }

    pub(crate) fn search_ids_matching(
        &self,
        query: &str,
        limit: usize,
        matches: impl Fn(&SymbolRecord) -> bool,
    ) -> Vec<SymbolId> {
        let query = query.trim().to_ascii_lowercase();
        let mut matches = self
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| matches(symbol))
            .filter_map(|(index, symbol)| {
                let name = symbol.name.to_ascii_lowercase();
                let rank = if query.is_empty() || name == query {
                    0
                } else if name.starts_with(&query) {
                    1
                } else if name.contains(&query) {
                    2
                } else {
                    return None;
                };
                Some((rank, index, symbol.id))
            })
            .collect::<Vec<_>>();
        if !query.is_empty() {
            matches.sort_unstable_by_key(|(rank, index, _)| (*rank, *index));
        }
        matches
            .into_iter()
            .take(limit)
            .map(|(_, _, id)| id)
            .collect()
    }

    pub(crate) fn lookup_cpu(&self, location: CpuLocation) -> impl Iterator<Item = &SymbolRecord> {
        self.cpu_index
            .get(&location)
            .into_iter()
            .flatten()
            .filter_map(|id| self.symbol(*id))
    }

    pub(crate) fn lookup_cpu_mapped(
        &self,
        location: CpuLocation,
        bank: Option<u32>,
    ) -> impl Iterator<Item = &SymbolRecord> {
        self.lookup_cpu(location).filter(move |symbol| {
            bank.zip(symbol.location.bank)
                .is_none_or(|(mapped, symbol)| mapped == symbol)
        })
    }

    pub(crate) fn lookup_storage(
        &self,
        location: StorageLocation,
    ) -> impl Iterator<Item = &SymbolRecord> {
        self.storage_index
            .binary_search_by_key(&location, |(entry, _)| *entry)
            .ok()
            .into_iter()
            .flat_map(|index| self.storage_index[index].1.iter())
            .filter_map(|id| self.symbol(*id))
    }

    pub(crate) fn lookup_storage_containing(
        &self,
        location: StorageLocation,
    ) -> impl Iterator<Item = &SymbolRecord> {
        let end = self
            .storage_index
            .partition_point(|(start, _)| *start <= location);
        self.storage_index[..end]
            .iter()
            .rev()
            .take_while(move |(start, _)| {
                start.image == location.image && start.region == location.region
            })
            .flat_map(|(_, ids)| ids.iter())
            .filter_map(|id| self.symbol(*id))
            .filter(move |symbol| {
                let Some(start) = symbol.location.storage else {
                    return false;
                };
                let end = start.offset.saturating_add(symbol.size.unwrap_or(1));
                start.offset <= location.offset && location.offset < end
            })
    }

    pub(crate) fn lookup_code_storage_containing(
        &self,
        location: StorageLocation,
    ) -> impl Iterator<Item = &SymbolRecord> {
        let mut ids = SmallVec::<[SymbolId; 4]>::new();
        let mut index = self
            .code_storage_index
            .partition_point(|entry| entry.start <= location);
        while index != 0 {
            index -= 1;
            let entry = self.code_storage_index[index];
            if entry.start.image != location.image || entry.start.region != location.region {
                break;
            }
            if entry.max_end <= location.offset {
                break;
            }
            if entry.end > location.offset {
                ids.push(entry.id);
            }
        }
        ids.into_iter().filter_map(|id| self.symbol(id))
    }

    pub(crate) fn len(&self) -> usize {
        self.symbols.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

fn single_id(id: SymbolId) -> SymbolIds {
    let mut ids = SymbolIds::new();
    ids.push(id);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::import::{ImportContext, TargetInfo, import_symbols};
    use crate::symbols::{
        AddressSpaceId, Confidence, ExecMode, ImageId, Provenance, ProvenanceKind, RegionId,
        SymbolKind, SymbolLocation, SymbolScope,
    };
    use zeff_emu_common::system::System;

    fn symbol(name: &str, cpu: u64, offset: u64) -> SymbolRecord {
        SymbolRecord {
            id: SymbolId(0),
            name: name.into(),
            location: SymbolLocation {
                cpu: Some(CpuLocation {
                    space: AddressSpaceId(0),
                    address: cpu,
                }),
                storage: Some(StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset,
                }),
                bank: Some((offset / 0x4000) as u32),
                exec_mode: ExecMode::Sm83,
            },
            value: None,
            size: None,
            kind: SymbolKind::Label,
            scope: SymbolScope::Global,
            provenance: Provenance {
                kind: ProvenanceKind::Build,
                source: None,
            },
            confidence: Confidence::Exact,
            comment: None,
        }
    }

    #[test]
    fn store_keeps_duplicate_names_and_aliases() {
        let mut store = SymbolStore::default();
        store.insert(symbol("Update", 0x4560, 0x8560));
        store.insert(symbol("Update", 0x4560, 0xC560));
        store.insert(symbol("UpdateAlias", 0x4560, 0x8560));

        assert_eq!(store.lookup_name("Update").count(), 2);
        assert_eq!(
            store
                .lookup_storage(StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset: 0x8560,
                })
                .count(),
            2
        );
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn store_finds_symbols_containing_an_offset() {
        let mut store = SymbolStore::default();
        let mut function = symbol("Update", 0x4560, 0x8560);
        function.size = Some(0x20);
        store.insert(function);

        let at = |offset| StorageLocation {
            image: ImageId(0),
            region: RegionId(0),
            offset,
        };
        assert_eq!(store.lookup_storage_containing(at(0x857F)).count(), 1);
        assert_eq!(store.lookup_storage_containing(at(0x8580)).count(), 0);
    }

    #[test]
    fn code_range_index_handles_labels_inside_functions() {
        let mut store = SymbolStore::default();
        let mut function = symbol("Update", 0x4560, 0x8560);
        function.kind = SymbolKind::Function;
        function.size = Some(0x20);
        store.insert(function);
        store.insert(symbol("Loop", 0x4570, 0x8570));

        let at = |offset| StorageLocation {
            image: ImageId(0),
            region: RegionId(0),
            offset,
        };
        let names = store
            .lookup_code_storage_containing(at(0x8570))
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"Update"));
        assert!(names.contains(&"Loop"));
        assert_eq!(store.lookup_code_storage_containing(at(0x857F)).count(), 1);
        assert_eq!(store.lookup_code_storage_containing(at(0x8580)).count(), 0);
    }

    #[test]
    fn indexes_large_generated_gnu_nm_symbols() {
        let data = crate::symbols::import::gnu_nm::generated_large_fixture(70_001);
        let module = import_symbols(
            "game.sym",
            &data,
            &ImportContext {
                target: TargetInfo {
                    system: System::Gba,
                },
                image: ImageId(0),
                rom_region: RegionId(0),
                cpu_space: AddressSpaceId(0),
                source_name: None,
            },
        )
        .unwrap();
        let started = std::time::Instant::now();
        let mut store = SymbolStore::default();
        store.extend(module.symbols);
        eprintln!("large GNU nm index: {} ms", started.elapsed().as_millis());
        assert!(store.len() > 70_000);
    }

    #[test]
    fn symbol_search_is_case_insensitive_and_limited() {
        let mut store = SymbolStore::default();
        store.insert(symbol("UpdatePlayer", 0x4560, 0x8560));
        store.insert(symbol("UpdateNpc", 0x4660, 0x8660));
        store.insert(symbol("DrawPlayer", 0x4760, 0x8760));

        let results = store.search_ids("update", 1);
        assert_eq!(results.len(), 1);
        assert_eq!(store.symbol(results[0]).unwrap().name, "UpdatePlayer");
        assert_eq!(store.search_ids("PLAYER", 10).len(), 2);
    }

    #[test]
    fn symbol_search_ranks_exact_and_prefix_matches_first() {
        let mut store = SymbolStore::default();
        store.insert(symbol("BeforeUpdate", 0x4560, 0x8560));
        store.insert(symbol("UpdateNpc", 0x4660, 0x8660));
        store.insert(symbol("Update", 0x4760, 0x8760));

        let names = store
            .search_ids("update", 10)
            .into_iter()
            .map(|id| store.symbol(id).unwrap().name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Update", "UpdateNpc", "BeforeUpdate"]);
    }

    #[test]
    fn replacing_user_symbols_keeps_imported_symbols() {
        let mut store = SymbolStore::default();
        store.insert(symbol("Imported", 0x4560, 0x8560));
        let mut first = symbol("First", 0x4660, 0x8660);
        first.provenance.kind = ProvenanceKind::User;
        store.replace_user_symbols([first]);

        let mut second = symbol("Second", 0x4760, 0x8760);
        second.provenance.kind = ProvenanceKind::User;
        store.replace_user_symbols([second]);

        assert_eq!(store.len(), 2);
        assert_eq!(store.lookup_name("Imported").count(), 1);
        assert_eq!(store.lookup_name("First").count(), 0);
        assert_eq!(store.lookup_name("Second").count(), 1);
    }
}
