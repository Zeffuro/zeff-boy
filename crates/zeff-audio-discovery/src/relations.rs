use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::{ScanReport, ScanStatus, SourceSpan, catalog::SongId, detectors::DetectorDescriptor};

mod gax;
mod gba;
mod other;
#[cfg(test)]
mod tests;

pub const MAX_NODES: u32 = 16_384;
pub const MAX_EDGES: u32 = 65_536;
pub const MAX_WORK: u32 = 1_000_000;
const MAX_LABEL_BYTES: usize = 192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GraphLimits {
    pub max_nodes: u32,
    pub max_edges: u32,
    pub max_work: u32,
}

impl Default for GraphLimits {
    fn default() -> Self {
        Self {
            max_nodes: 4096,
            max_edges: 8192,
            max_work: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum GraphStatus {
    Complete,
    Incomplete(GraphStop),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphStop {
    Cancelled,
    InvalidLimits,
    MissingSong,
    InvalidLocation,
    NodeLimit,
    EdgeLimit,
    WorkLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetLocation {
    MediaBytes {
        offset: u32,
        byte_len: u32,
        cpu_address: Option<u32>,
    },
    LogicalVgm {
        offset: u32,
        byte_len: u32,
    },
    DiscTrack {
        number: u8,
        index1_lba: u32,
        end_lba: u32,
        pcm_frames: u64,
    },
}

impl From<SourceSpan> for AssetLocation {
    fn from(span: SourceSpan) -> Self {
        Self::MediaBytes {
            offset: span.effective_offset,
            byte_len: span.byte_len,
            cpu_address: span.canonical_cpu_address,
        }
    }
}

impl From<crate::RomSpan> for AssetLocation {
    fn from(span: crate::RomSpan) -> Self {
        SourceSpan::from(span).into()
    }
}

impl From<crate::tracker::FileSpan> for AssetLocation {
    fn from(span: crate::tracker::FileSpan) -> Self {
        SourceSpan::from(span).into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Song,
    SongTableEntry,
    Channel,
    Section,
    SequenceData,
    MappedData,
    OrderTable,
    Order,
    Pattern,
    Instrument,
    KeyMap,
    Sample,
    SampleData,
    Waveform,
    Synthesis,
    Envelope,
    Module,
    Container,
    Program,
    EntryPoint,
    RegisterLog,
    LogicalImage,
    Metadata,
    Chip,
    CdAudio,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AssetNode {
    pub id: u32,
    pub kind: AssetKind,
    pub label: String,
    pub location: Option<AssetLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<crate::SampleInventory>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Relation {
    Contains,
    Selects { index: u32 },
    UsesInstrument { index: u32 },
    UsesSample { slot: u32 },
    DeclaresSampleSlot { slot: u32 },
    MapsKeys { first: u8, last: u8 },
    References,
    Declares,
    DecodesTo,
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationEvidence {
    ParsedStructure,
    DeclaredMetadata,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct AssetEdge {
    pub from: u32,
    pub to: u32,
    pub relation: Relation,
    pub evidence: RelationEvidence,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct AssetGraph {
    pub schema: &'static str,
    pub song: SongId,
    pub media_system: &'static str,
    pub media_byte_len: u64,
    pub media_sha256: Option<String>,
    pub scan_status: ScanStatus,
    pub detector: Option<DetectorDescriptor>,
    pub profile: Option<String>,
    pub mp2k_evidence: Option<Mp2kEvidence>,
    pub logical_source: Option<crate::vgm::LogicalIdentity>,
    pub limits: GraphLimits,
    pub status: GraphStatus,
    pub work_used: u32,
    pub root: Option<u32>,
    pub nodes: Vec<AssetNode>,
    pub edges: Vec<AssetEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Mp2kEvidence {
    pub engine: crate::EngineProfile,
    pub confidence: crate::Confidence,
    pub evidence: crate::CandidateEvidence,
}

impl ScanReport {
    /// Projects retained inventory only. Completion does not imply complete discovery,
    /// playback fidelity, or permission to export a source range.
    pub fn asset_relations(
        &self,
        song: SongId,
        limits: GraphLimits,
        cancel: &AtomicBool,
    ) -> AssetGraph {
        let mut builder = Builder {
            media: &self.media,
            graph: AssetGraph {
                schema: "zeff-audio-relations/1",
                song,
                media_system: self.media.system,
                media_byte_len: self.media.byte_len,
                media_sha256: self.media.sha256.as_deref().map(label),
                scan_status: self.status,
                detector: None,
                profile: None,
                mp2k_evidence: None,
                logical_source: None,
                limits,
                status: GraphStatus::Complete,
                work_used: 0,
                root: None,
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            cancel,
            located: BTreeMap::new(),
            edges: BTreeSet::new(),
            expanded: BTreeSet::new(),
        };
        let result = (|| {
            if limits.max_nodes > MAX_NODES
                || limits.max_edges > MAX_EDGES
                || limits.max_work > MAX_WORK
            {
                return Err(GraphStop::InvalidLimits);
            }
            builder.charge()?;
            let selected = self.song(song).ok_or(GraphStop::MissingSong)?;
            for descriptor in self.applicable_detectors {
                builder.charge()?;
                if descriptor.id == selected.detector_id() {
                    builder.graph.detector = Some(*descriptor);
                    break;
                }
            }
            match selected {
                crate::catalog::SongRef::Mp2k(song) => gba::mp2k(&mut builder, song),
                crate::catalog::SongRef::Gax(song) => gax::project(&mut builder, song),
                song => other::project(&mut builder, song),
            }
        })();
        if let Err(reason) = result {
            builder.graph.status = GraphStatus::Incomplete(reason);
        }
        builder.graph
    }
}

type Result<T> = std::result::Result<T, GraphStop>;

struct Builder<'a> {
    media: &'a crate::MediaIdentity,
    graph: AssetGraph,
    cancel: &'a AtomicBool,
    located: BTreeMap<(AssetKind, AssetLocation), u32>,
    edges: BTreeSet<AssetEdge>,
    expanded: BTreeSet<u32>,
}

impl Builder<'_> {
    fn charge(&mut self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(GraphStop::Cancelled);
        }
        if self.graph.work_used >= self.graph.limits.max_work {
            return Err(GraphStop::WorkLimit);
        }
        self.graph.work_used += 1;
        Ok(())
    }

    fn node(
        &mut self,
        kind: AssetKind,
        name: &str,
        location: Option<AssetLocation>,
    ) -> Result<u32> {
        self.charge()?;
        if let Some(location) = location {
            self.validate_location(location)?;
            if !matches!(
                kind,
                AssetKind::Channel
                    | AssetKind::Section
                    | AssetKind::Order
                    | AssetKind::EntryPoint
                    | AssetKind::Sample
            ) && let Some(&id) = self.located.get(&(kind, location))
            {
                return Ok(id);
            }
        }
        if self.graph.nodes.len() >= self.graph.limits.max_nodes as usize {
            return Err(GraphStop::NodeLimit);
        }
        let id = self.graph.nodes.len() as u32;
        self.graph.nodes.push(AssetNode {
            id,
            kind,
            label: label(name),
            location,
            sample: None,
        });
        if let Some(location) = location {
            self.located.insert((kind, location), id);
        }
        Ok(id)
    }

    fn root(
        &mut self,
        kind: AssetKind,
        name: &str,
        location: Option<AssetLocation>,
    ) -> Result<u32> {
        let id = self.node(kind, name, location)?;
        self.graph.root = Some(id);
        Ok(id)
    }

    fn edge(&mut self, from: u32, to: u32, relation: Relation) -> Result<()> {
        self.charge()?;
        let evidence = match relation {
            Relation::Declares | Relation::DeclaresSampleSlot { .. } => {
                RelationEvidence::DeclaredMetadata
            }
            Relation::Unresolved => RelationEvidence::Unresolved,
            _ => RelationEvidence::ParsedStructure,
        };
        let edge = AssetEdge {
            from,
            to,
            relation,
            evidence,
        };
        if self.edges.contains(&edge) {
            return Ok(());
        }
        if self.graph.edges.len() >= self.graph.limits.max_edges as usize {
            return Err(GraphStop::EdgeLimit);
        }
        self.edges.insert(edge.clone());
        self.graph.edges.push(edge);
        Ok(())
    }

    fn child(
        &mut self,
        parent: u32,
        kind: AssetKind,
        name: &str,
        location: Option<AssetLocation>,
        relation: Relation,
    ) -> Result<u32> {
        let child = self.node(kind, name, location)?;
        self.edge(parent, child, relation)?;
        Ok(child)
    }

    fn unresolved(&mut self, parent: u32, reason: &str) -> Result<u32> {
        self.child(
            parent,
            AssetKind::Unresolved,
            reason,
            None,
            Relation::Unresolved,
        )
    }

    fn validate_location(&self, location: AssetLocation) -> Result<()> {
        let valid = match location {
            AssetLocation::MediaBytes {
                offset,
                byte_len,
                cpu_address,
            } => {
                byte_len > 0
                    && u64::from(offset) + u64::from(byte_len) <= self.graph.media_byte_len
                    && cpu_address.is_none_or(|address| {
                        (self.graph.media_system == "gba"
                            && u64::from(address) == 0x0800_0000 + u64::from(offset)
                            && u64::from(offset) + u64::from(byte_len)
                                <= crate::MAX_ROM_BYTES as u64)
                            || crate::gb_native::source_span_matches(
                                self.media,
                                SourceSpan {
                                    effective_offset: offset,
                                    byte_len,
                                    canonical_cpu_address: Some(address),
                                },
                            )
                            || crate::nes_native::source_span_matches(
                                self.media,
                                SourceSpan {
                                    effective_offset: offset,
                                    byte_len,
                                    canonical_cpu_address: Some(address),
                                },
                            )
                            || crate::sega_psg::source_span_matches(
                                self.media,
                                SourceSpan {
                                    effective_offset: offset,
                                    byte_len,
                                    canonical_cpu_address: Some(address),
                                },
                            )
                    })
            }
            AssetLocation::LogicalVgm { offset, byte_len } => {
                byte_len > 0
                    && self.graph.logical_source.as_ref().is_some_and(|source| {
                        source.address_space == crate::vgm::VgmAddressSpace::DecompressedVgm
                            && u64::from(offset) + u64::from(byte_len) <= u64::from(source.byte_len)
                    })
            }
            AssetLocation::DiscTrack {
                number,
                index1_lba,
                end_lba,
                pcm_frames,
            } => {
                number > 0
                    && end_lba > index1_lba
                    && pcm_frames == u64::from(end_lba - index1_lba) * 588
            }
        };
        valid.then_some(()).ok_or(GraphStop::InvalidLocation)
    }
}

fn label(value: &str) -> String {
    let mut end = value.len().min(MAX_LABEL_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
