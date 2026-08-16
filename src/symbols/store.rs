use std::collections::{BTreeMap, HashMap};

use super::{CpuLocation, StorageLocation, SymbolId, SymbolRecord};

#[derive(Default)]
pub(crate) struct SymbolStore {
    symbols: Vec<SymbolRecord>,
    name_index: HashMap<String, Vec<SymbolId>>,
    cpu_index: BTreeMap<CpuLocation, Vec<SymbolId>>,
    storage_index: BTreeMap<StorageLocation, Vec<SymbolId>>,
    generation: u64,
}

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
        if let Some(storage) = symbol.location.storage {
            self.storage_index.entry(storage).or_default().push(id);
        }
        self.symbols.push(symbol);
        self.generation = self.generation.wrapping_add(1);
        id
    }

    pub(crate) fn extend(&mut self, symbols: impl IntoIterator<Item = SymbolRecord>) {
        for symbol in symbols {
            self.insert(symbol);
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
        let query = query.trim().to_ascii_lowercase();
        self.symbols
            .iter()
            .filter(|symbol| query.is_empty() || symbol.name.to_ascii_lowercase().contains(&query))
            .take(limit)
            .map(|symbol| symbol.id)
            .collect()
    }

    pub(crate) fn lookup_cpu(&self, location: CpuLocation) -> impl Iterator<Item = &SymbolRecord> {
        self.cpu_index
            .get(&location)
            .into_iter()
            .flatten()
            .filter_map(|id| self.symbol(*id))
    }

    pub(crate) fn lookup_storage(
        &self,
        location: StorageLocation,
    ) -> impl Iterator<Item = &SymbolRecord> {
        self.storage_index
            .get(&location)
            .into_iter()
            .flatten()
            .filter_map(|id| self.symbol(*id))
    }

    pub(crate) fn lookup_storage_containing(
        &self,
        location: StorageLocation,
    ) -> impl Iterator<Item = &SymbolRecord> {
        let region_start = StorageLocation {
            offset: 0,
            ..location
        };
        self.storage_index
            .range(region_start..=location)
            .rev()
            .flat_map(|(_, ids)| ids)
            .filter_map(|id| self.symbol(*id))
            .filter(move |symbol| {
                let Some(start) = symbol.location.storage else {
                    return false;
                };
                let end = start.offset.saturating_add(symbol.size.unwrap_or(1));
                start.offset <= location.offset && location.offset < end
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{
        AddressSpaceId, Confidence, ExecMode, ImageId, Provenance, ProvenanceKind, RegionId,
        SymbolKind, SymbolLocation, SymbolScope,
    };

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
}
