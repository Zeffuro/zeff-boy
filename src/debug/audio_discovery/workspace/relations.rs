use std::sync::atomic::AtomicBool;

use crate::audio_discovery::{
    ScanReport, ScanStatus, ScanStop, SourceSpan,
    catalog::SongId,
    relations::{
        AssetGraph, AssetKind, AssetLocation, GraphLimits, GraphStatus, GraphStop, Relation,
        RelationEvidence,
    },
};

use super::{AudioWorkspace, select_span};

const MAX_RELATED_NODES: usize = 24;

pub(super) struct CachedGraph {
    song: SongId,
    graph: AssetGraph,
    selected_node: Option<u32>,
    copy_error: Option<String>,
}

impl CachedGraph {
    pub(super) fn suppresses_hex_default_for(&self, song: SongId) -> bool {
        self.song == song
            && self.selected_node.is_some_and(|id| {
                !matches!(
                    self.graph
                        .nodes
                        .get(id as usize)
                        .filter(|node| node.id == id)
                        .and_then(|node| node.location),
                    Some(AssetLocation::MediaBytes { .. })
                )
            })
    }
}

pub(super) fn draw(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, report: &ScanReport) {
    egui::CollapsingHeader::new("Asset relationships").show(ui, |ui| {
        let Some(song) = workspace.selected_candidate else {
            ui.small("Select a song to view its retained relationships.");
            return;
        };
        let mut cached = workspace
            .relationships
            .take()
            .filter(|cached| cached.song == song);
        if cached.is_none() {
            cached = Some(CachedGraph {
                song,
                graph: report.asset_relations(
                    song,
                    GraphLimits::default(),
                    &AtomicBool::new(false),
                ),
                selected_node: None,
                copy_error: None,
            });
        }
        let mut cached = cached.expect("relationship graph is initialized");
        draw_graph(ui, workspace, &mut cached);
        workspace.relationships = Some(cached);
    });
}

fn draw_graph(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, cached: &mut CachedGraph) {
    let mut clicked_node = None;
    let default_selected = {
        let graph = &cached.graph;
        ui.small(format!(
            "{} items · {} relationships",
            graph.nodes.len(),
            graph.edges.len()
        ));
        status(ui, graph);
        ui.small("Relationships reflect retained scan data; unmapped assets are shown explicitly.");

        let selected_node = cached.selected_node;
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        egui::ScrollArea::vertical()
            .id_salt(("audio-relationship-nodes", cached.song))
            .max_height(180.0)
            .show_rows(ui, row_height, graph.nodes.len(), |ui, rows| {
                for index in rows {
                    let node = &graph.nodes[index];
                    let label = node_label(node.kind, &node.label);
                    if ui
                        .selectable_label(selected_node == Some(node.id), label)
                        .clicked()
                    {
                        clicked_node = Some(node.id);
                    }
                }
            });
        graph.root
    };
    if let Some(node) = clicked_node {
        select_node(workspace, cached, node);
    }
    let Some(selected) = cached.selected_node.or(default_selected) else {
        return;
    };
    if cached.selected_node.is_none() {
        cached.selected_node = Some(selected);
    }
    draw_selected_node(ui, workspace, cached, selected);
}
fn status(ui: &mut egui::Ui, graph: &AssetGraph) {
    match graph.status {
        GraphStatus::Complete => ui.small("Relationship view is complete."),
        GraphStatus::Incomplete(stop) => ui.colored_label(
            egui::Color32::YELLOW,
            format!("Relationship view is partial: {}.", graph_stop_label(stop)),
        ),
    };
    match graph.scan_status {
        ScanStatus::Complete => ui.small("The source scan is complete."),
        ScanStatus::Unsupported => ui.small("The source scan does not support this media."),
        ScanStatus::Malformed(_) => ui.colored_label(
            egui::Color32::YELLOW,
            "The source scan found malformed media.",
        ),
        ScanStatus::Incomplete(stop) => ui.colored_label(
            egui::Color32::YELLOW,
            format!("The source scan is partial: {}.", scan_stop_label(stop)),
        ),
    };
}

fn draw_selected_node(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    cached: &mut CachedGraph,
    selected: u32,
) {
    let Some(node) = cached
        .graph
        .nodes
        .get(selected as usize)
        .filter(|node| node.id == selected)
    else {
        return;
    };
    let heading = node_label(node.kind, &node.label);
    let location = node.location;
    ui.separator();
    ui.strong(heading);
    if let Some(location) = location {
        ui.small(location_label(location));
    } else {
        ui.small("No mapped source location");
    }

    let related = related_nodes(&cached.graph, selected);
    if related.items.is_empty() {
        ui.small("No retained incoming or outgoing relationships.");
    } else {
        ui.label("Connected items");
        if related.total > related.items.len() {
            ui.small(format!(
                "Showing first {} of {} connected items.",
                related.items.len(),
                related.total
            ));
        }
        let mut clicked_node = None;
        for related in &related.items {
            if ui.selectable_label(false, &related.label).clicked() {
                clicked_node = Some(related.node);
            }
        }
        if let Some(node) = clicked_node {
            select_node(workspace, cached, node);
        }
    }

    egui::CollapsingHeader::new("Technical relationship details").show(ui, |ui| {
        ui.small(format!("Relationship status: {:?}", cached.graph.status));
        ui.small(format!(
            "Source scan status: {:?}",
            cached.graph.scan_status
        ));
        ui.small(format!("Projection work: {} units", cached.graph.work_used));
        if ui.button("Copy relationships JSON").clicked() {
            match serde_json::to_string_pretty(&cached.graph) {
                Ok(json) => {
                    ui.ctx().copy_text(json);
                    cached.copy_error = None;
                }
                Err(error) => {
                    cached.copy_error = Some(format!("Could not copy relationships: {error}"))
                }
            }
        }
        if let Some(error) = &cached.copy_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        for related in &related.items {
            ui.small(format!(
                "{} · {}",
                related.label,
                evidence_label(related.evidence)
            ));
        }
    });
}

struct RelatedNode {
    node: u32,
    label: String,
    evidence: RelationEvidence,
}

struct RelatedNodes {
    items: Vec<RelatedNode>,
    total: usize,
}

fn related_nodes(graph: &AssetGraph, selected: u32) -> RelatedNodes {
    let mut total = 0;
    let mut items = Vec::new();
    for edge in &graph.edges {
        let (node, direction) = if edge.from == selected {
            (edge.to, "to")
        } else if edge.to == selected {
            (edge.from, "from")
        } else {
            continue;
        };
        let Some(target) = graph
            .nodes
            .get(node as usize)
            .filter(|target| target.id == node)
        else {
            continue;
        };
        total += 1;
        if items.len() < MAX_RELATED_NODES {
            items.push(RelatedNode {
                node,
                label: format!(
                    "{} {} {}",
                    relation_label(edge.relation),
                    direction,
                    node_label(target.kind, &target.label)
                ),
                evidence: edge.evidence,
            });
        }
    }
    RelatedNodes { items, total }
}

fn select_node(workspace: &mut AudioWorkspace, cached: &mut CachedGraph, id: u32) {
    let Some(node) = cached
        .graph
        .nodes
        .get(id as usize)
        .filter(|node| node.id == id)
    else {
        return;
    };
    let location = node.location;
    let label = node_label(node.kind, &node.label);
    cached.selected_node = Some(id);
    if let Some(AssetLocation::MediaBytes {
        offset,
        byte_len,
        cpu_address,
    }) = location
    {
        select_span(
            workspace,
            SourceSpan {
                effective_offset: offset,
                byte_len,
                canonical_cpu_address: cpu_address,
            },
            label,
        );
    } else {
        workspace.selected_span = None;
        workspace.hex_reset = true;
    }
}

fn node_label(kind: AssetKind, label: &str) -> String {
    format!("{} · {label}", kind_label(kind))
}

fn kind_label(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Song => "Song",
        AssetKind::SongTableEntry => "Song table entry",
        AssetKind::Channel => "Channel",
        AssetKind::Section => "Section",
        AssetKind::SequenceData => "Sequence data",
        AssetKind::MappedData => "Mapped data",
        AssetKind::OrderTable => "Order table",
        AssetKind::Order => "Order",
        AssetKind::Pattern => "Pattern",
        AssetKind::Instrument => "Instrument",
        AssetKind::KeyMap => "Key map",
        AssetKind::Sample => "Sample",
        AssetKind::SampleData => "Sample data",
        AssetKind::Waveform => "Waveform",
        AssetKind::Synthesis => "Synthesis",
        AssetKind::Envelope => "Envelope",
        AssetKind::Module => "Module",
        AssetKind::Container => "Container",
        AssetKind::Program => "Program",
        AssetKind::EntryPoint => "Entry point",
        AssetKind::RegisterLog => "Register log",
        AssetKind::LogicalImage => "Logical image",
        AssetKind::Metadata => "Metadata",
        AssetKind::Chip => "Chip",
        AssetKind::CdAudio => "CD audio",
        AssetKind::Unresolved => "Unmapped item",
    }
}

fn location_label(location: AssetLocation) -> String {
    match location {
        AssetLocation::MediaBytes {
            offset,
            byte_len,
            cpu_address,
        } => match cpu_address {
            Some(address) => {
                format!("CPU ROM 0x{address:08X} · +0x{offset:06X} · {byte_len} bytes")
            }
            None => format!("File +0x{offset:06X} · {byte_len} bytes"),
        },
        AssetLocation::LogicalVgm { offset, byte_len } => {
            format!("VGM logical data +0x{offset:06X} · {byte_len} bytes")
        }
        AssetLocation::DiscTrack {
            number,
            index1_lba,
            end_lba,
            pcm_frames,
        } => {
            format!("CD track {number} · sectors {index1_lba}–{end_lba} · {pcm_frames} PCM frames")
        }
    }
}

fn relation_label(relation: Relation) -> String {
    match relation {
        Relation::Contains => "Contains".to_owned(),
        Relation::Selects { index } => format!("Selects item {index}"),
        Relation::UsesInstrument { index } => format!("Uses instrument {index}"),
        Relation::UsesSample { slot } => format!("Uses sample {slot}"),
        Relation::DeclaresSampleSlot { slot } => format!("Declares sample slot {slot}"),
        Relation::MapsKeys { first, last } => format!("Maps keys {first}–{last}"),
        Relation::References => "References".to_owned(),
        Relation::Declares => "Declares".to_owned(),
        Relation::DecodesTo => "Decodes to".to_owned(),
        Relation::Unresolved => "Is unresolved for".to_owned(),
    }
}

fn evidence_label(evidence: RelationEvidence) -> &'static str {
    match evidence {
        RelationEvidence::ParsedStructure => "parsed structure",
        RelationEvidence::DeclaredMetadata => "declared metadata",
        RelationEvidence::Unresolved => "unresolved",
    }
}

fn graph_stop_label(stop: GraphStop) -> &'static str {
    match stop {
        GraphStop::Cancelled => "cancelled",
        GraphStop::InvalidLimits => "invalid limits",
        GraphStop::MissingSong => "the selected song is unavailable",
        GraphStop::InvalidLocation => "an invalid source location was rejected",
        GraphStop::NodeLimit => "the item limit was reached",
        GraphStop::EdgeLimit => "the relationship limit was reached",
        GraphStop::WorkLimit => "the analysis work limit was reached",
    }
}

fn scan_stop_label(stop: ScanStop) -> &'static str {
    match stop {
        ScanStop::Cancelled => "cancelled",
        ScanStop::WorkLimit => "the analysis work limit was reached",
        ScanStop::CandidateLimit => "the song limit was reached",
        ScanStop::MediaLimit => "the media size limit was reached",
        ScanStop::InvalidLimits => "invalid limits",
        ScanStop::InventoryLimit => "the inventory limit was reached",
        ScanStop::ValidationLimit => "the validation limit was reached",
    }
}

#[cfg(test)]
#[path = "relations_tests.rs"]
mod tests;
