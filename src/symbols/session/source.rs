use std::path::PathBuf;

use super::{SourceReference, SymbolSession};
use crate::symbols::{
    AddressSpaceId, CpuLocation, DebugSegment, ImageId, LoadInstance, RegionId,
    ResolvedLoadInstance, SegmentId, SourceFile, SourceLine, StorageLocation,
};

impl SymbolSession {
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

    pub(super) fn extend_source_metadata(
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
