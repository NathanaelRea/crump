//! Generated, sound-neutral Instrument section inspection.

use std::{collections::BTreeSet, io::Cursor, path::PathBuf};

use crate::{
    diagnostic::SourceLocation,
    media::{AssetInventory, LoadedAsset, WavMetadata, WavSampleFormat},
    model::{
        GraphDomain, Instrument, MediaAsset, NodeKind, PresentationItem, PresentationItemKind,
        PresentationSectionKind, Project, SettingValue, ValueSite,
    },
};

pub(super) const SECTION_KINDS: [PresentationSectionKind; 8] = [
    PresentationSectionKind::Sources,
    PresentationSectionKind::SourceMixer,
    PresentationSectionKind::Filters,
    PresentationSectionKind::EnvelopesLfos,
    PresentationSectionKind::Modulation,
    PresentationSectionKind::Voicing,
    PresentationSectionKind::Routing,
    PresentationSectionKind::Effects,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InstrumentSectionView {
    pub(super) id: String,
    pub(super) kind: PresentationSectionKind,
    pub(super) name: String,
    pub(super) rows: Vec<InstrumentSectionRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InstrumentSectionRow {
    pub(super) kind: Option<PresentationItemKind>,
    pub(super) id: String,
    pub(super) detail: String,
    pub(super) source: String,
    pub(super) source_file: PathBuf,
    pub(super) source_offset: usize,
}

#[cfg(test)]
pub(super) fn instrument_section_views(
    project: &Project,
    instrument: &Instrument,
) -> Vec<InstrumentSectionView> {
    instrument_section_views_impl(project, instrument, None)
}

pub(super) fn instrument_section_views_with_assets(
    project: &Project,
    instrument: &Instrument,
    assets: &AssetInventory,
) -> Vec<InstrumentSectionView> {
    instrument_section_views_impl(project, instrument, Some(assets))
}

fn instrument_section_views_impl(
    project: &Project,
    instrument: &Instrument,
    assets: Option<&AssetInventory>,
) -> Vec<InstrumentSectionView> {
    let mut assigned = BTreeSet::new();
    let mut sections = instrument
        .presentation
        .sections
        .iter()
        .map(|section| InstrumentSectionView {
            id: section.id.clone(),
            kind: section.kind,
            name: section.name.clone(),
            rows: section
                .items
                .iter()
                .map(|item| {
                    assigned.insert((item.kind, item.id.clone()));
                    authored_row(project, instrument, item, assets)
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    for kind in SECTION_KINDS {
        if !sections.iter().any(|section| section.kind == kind) {
            sections.push(InstrumentSectionView {
                id: format!("generated-{}", kind.as_str().replace('_', "-")),
                kind,
                name: section_title(kind).to_owned(),
                rows: Vec::new(),
            });
        }
    }

    if !instrument.presentation.tags.is_empty() {
        append_row(
            &mut sections,
            PresentationSectionKind::Sources,
            InstrumentSectionRow {
                kind: None,
                id: "tags".to_owned(),
                detail: instrument.presentation.tags.join(", "),
                source: location_label(&instrument.location),
                source_file: instrument.location.file.clone(),
                source_offset: instrument.location.start,
            },
        );
    }
    if let Some(preview) = &instrument.presentation.preview {
        append_row(
            &mut sections,
            PresentationSectionKind::Voicing,
            InstrumentSectionRow {
                kind: None,
                id: "preview".to_owned(),
                detail: format!(
                    "{}..{} · velocity {}..{} · {}",
                    crate::model::format_pitch(preview.low_pitch),
                    crate::model::format_pitch(preview.high_pitch),
                    preview.low_velocity,
                    preview.high_velocity,
                    preview.note_duration
                ),
                source: location_label(&preview.location),
                source_file: preview.location.file.clone(),
                source_offset: preview.location.start,
            },
        );
    }

    for graph in instrument
        .graph
        .iter()
        .chain(instrument.shared_graph.iter())
    {
        for node in &graph.nodes {
            let key = (PresentationItemKind::Node, node.id.clone());
            if assigned.insert(key) {
                append_row(
                    &mut sections,
                    node_section(node.kind, graph.domain),
                    InstrumentSectionRow {
                        kind: Some(PresentationItemKind::Node),
                        id: node.id.clone(),
                        detail: format!("{:?} · {:?} graph", node.kind, graph.domain),
                        source: location_label(&node.location),
                        source_file: node.location.file.clone(),
                        source_offset: node.location.start,
                    },
                );
            }
        }
        for bus in &graph.connections {
            let key = (PresentationItemKind::Bus, bus.id.clone());
            if assigned.insert(key) {
                append_row(
                    &mut sections,
                    PresentationSectionKind::Routing,
                    InstrumentSectionRow {
                        kind: Some(PresentationItemKind::Bus),
                        id: bus.id.clone(),
                        detail: format!(
                            "{}.{} → {}.{} · {:?}",
                            bus.from.node, bus.from.port, bus.to.node, bus.to.port, graph.domain
                        ),
                        source: location_label(&bus.location),
                        source_file: bus.location.file.clone(),
                        source_offset: bus.location.start,
                    },
                );
            }
        }
    }

    for parameter in &instrument.parameters {
        let key = (PresentationItemKind::Parameter, parameter.key.clone());
        if assigned.insert(key) {
            let section = if parameter.macro_source {
                PresentationSectionKind::Modulation
            } else {
                instrument
                    .graph
                    .iter()
                    .chain(instrument.shared_graph.iter())
                    .flat_map(|graph| &graph.nodes)
                    .find(|node| node.references_parameter(&parameter.key))
                    .map_or(PresentationSectionKind::Sources, |node| {
                        node_section(node.kind, GraphDomain::Voice)
                    })
            };
            append_row(
                &mut sections,
                section,
                InstrumentSectionRow {
                    kind: Some(PresentationItemKind::Parameter),
                    id: parameter.key.clone(),
                    detail: format!(
                        "{} · {} {} · {}..{}",
                        parameter.name,
                        parameter.value,
                        parameter.unit,
                        parameter.minimum,
                        parameter.maximum
                    ),
                    source: location_label(&parameter.location),
                    source_file: parameter.location.file.clone(),
                    source_offset: parameter.location.start,
                },
            );
        }
    }

    for source in &instrument.modulation_sources {
        let key = (PresentationItemKind::Source, source.id.clone());
        if assigned.insert(key) {
            append_row(
                &mut sections,
                PresentationSectionKind::Modulation,
                InstrumentSectionRow {
                    kind: Some(PresentationItemKind::Source),
                    id: source.id.clone(),
                    detail: format!(
                        "{:?} · {:?} · {:?}",
                        source.kind, source.scope, source.polarity
                    ),
                    source: location_label(&source.location),
                    source_file: source.location.file.clone(),
                    source_offset: source.location.start,
                },
            );
        }
    }
    for route in &instrument.modulation_routes {
        let key = (PresentationItemKind::Route, route.id.clone());
        if assigned.insert(key) {
            append_row(
                &mut sections,
                PresentationSectionKind::Modulation,
                InstrumentSectionRow {
                    kind: Some(PresentationItemKind::Route),
                    id: route.id.clone(),
                    detail: format!(
                        "{} → {} · depth {} · order {}{}",
                        route.source,
                        route.target,
                        route.depth,
                        route.order,
                        if route.bypass { " · bypassed" } else { "" }
                    ),
                    source: location_label(&route.location),
                    source_file: route.location.file.clone(),
                    source_offset: route.location.start,
                },
            );
        }
    }
    for processor in &instrument.post_processors {
        let key = (PresentationItemKind::Processor, processor.id.clone());
        if assigned.insert(key) {
            append_row(
                &mut sections,
                PresentationSectionKind::Effects,
                InstrumentSectionRow {
                    kind: Some(PresentationItemKind::Processor),
                    id: processor.id.clone(),
                    detail: format!(
                        "{:?}{}",
                        processor.kind,
                        if processor.bypass { " · bypassed" } else { "" }
                    ),
                    source: location_label(&processor.location),
                    source_file: processor.location.file.clone(),
                    source_offset: processor.location.start,
                },
            );
        }
    }

    let mut referenced_assets = BTreeSet::new();
    for node in instrument
        .graph
        .iter()
        .chain(instrument.shared_graph.iter())
        .flat_map(|graph| &graph.nodes)
    {
        if matches!(
            node.kind,
            NodeKind::Wavetable | NodeKind::SampleSource | NodeKind::Granular | NodeKind::Spectral
        ) && let Some(SettingValue::Text(asset_id)) = node.setting("asset")
        {
            referenced_assets.insert(asset_id.as_str());
        }
        if node.kind == NodeKind::Multisample {
            for directive in &node.directives {
                if directive.tokens.len() >= 4
                    && directive.tokens[0].value == "zone"
                    && directive.tokens[2].value == "asset"
                {
                    referenced_assets.insert(directive.tokens[3].value.as_str());
                }
            }
        }
    }
    for asset_id in referenced_assets {
        let key = (PresentationItemKind::Asset, asset_id.to_owned());
        if assigned.insert(key)
            && let Some(asset) = project.assets.iter().find(|asset| asset.id == asset_id)
        {
            append_row(
                &mut sections,
                PresentationSectionKind::Sources,
                InstrumentSectionRow {
                    kind: Some(PresentationItemKind::Asset),
                    id: asset.id.clone(),
                    detail: asset_detail(asset, assets),
                    source: location_label(&asset.location),
                    source_file: asset.location.file.clone(),
                    source_offset: asset.location.start,
                },
            );
        }
    }

    append_row(
        &mut sections,
        PresentationSectionKind::Voicing,
        InstrumentSectionRow {
            kind: None,
            id: "policy".to_owned(),
            detail: format!(
                "{:?} · {} voices · {:?} retrigger · {:?} priority · {:?} steal · {:.3}s glide {:?}",
                instrument.voicing.mode,
                instrument.polyphony,
                instrument.voicing.retrigger,
                instrument.voicing.note_priority,
                instrument.voicing.steal_priority,
                instrument.voicing.glide_seconds,
                instrument.voicing.glide_mode
            ),
            source: location_label(&instrument.location),
            source_file: instrument.location.file.clone(),
            source_offset: instrument.location.start,
        },
    );
    sections
}

fn authored_row(
    project: &Project,
    instrument: &Instrument,
    item: &PresentationItem,
    assets: Option<&AssetInventory>,
) -> InstrumentSectionRow {
    let (source, source_file, source_offset) = item_location(project, instrument, item)
        .map_or_else(
            || {
                (
                    site_label(&item.id_site),
                    item.id_site.file.clone(),
                    item.id_site.start,
                )
            },
            |location| {
                (
                    location_label(location),
                    location.file.clone(),
                    location.start,
                )
            },
        );
    InstrumentSectionRow {
        kind: Some(item.kind),
        id: item.id.clone(),
        detail: item_detail(project, instrument, item, assets),
        source,
        source_file,
        source_offset,
    }
}

fn item_location<'a>(
    project: &'a Project,
    instrument: &'a Instrument,
    item: &PresentationItem,
) -> Option<&'a SourceLocation> {
    match item.kind {
        PresentationItemKind::Node => instrument
            .graph
            .iter()
            .chain(instrument.shared_graph.iter())
            .flat_map(|graph| &graph.nodes)
            .find(|node| node.id == item.id)
            .map(|node| &node.location),
        PresentationItemKind::Bus => instrument
            .graph
            .iter()
            .chain(instrument.shared_graph.iter())
            .flat_map(|graph| &graph.connections)
            .find(|bus| bus.id == item.id)
            .map(|bus| &bus.location),
        PresentationItemKind::Parameter => instrument
            .parameters
            .iter()
            .find(|parameter| parameter.key == item.id)
            .map(|parameter| &parameter.location),
        PresentationItemKind::Source => instrument
            .modulation_sources
            .iter()
            .find(|source| source.id == item.id)
            .map(|source| &source.location),
        PresentationItemKind::Route => instrument
            .modulation_routes
            .iter()
            .find(|route| route.id == item.id)
            .map(|route| &route.location),
        PresentationItemKind::Processor => instrument
            .post_processors
            .iter()
            .find(|processor| processor.id == item.id)
            .map(|processor| &processor.location),
        PresentationItemKind::Asset => project
            .assets
            .iter()
            .find(|asset| asset.id == item.id)
            .map(|asset| &asset.location),
    }
}

fn item_detail(
    project: &Project,
    instrument: &Instrument,
    item: &PresentationItem,
    assets: Option<&AssetInventory>,
) -> String {
    match item.kind {
        PresentationItemKind::Node => instrument
            .graph
            .iter()
            .chain(instrument.shared_graph.iter())
            .find_map(|graph| {
                graph
                    .nodes
                    .iter()
                    .find(|node| node.id == item.id)
                    .map(|node| node_detail(node, graph.domain))
            })
            .unwrap_or_else(|| "missing Node".to_owned()),
        PresentationItemKind::Bus => instrument
            .graph
            .iter()
            .chain(instrument.shared_graph.iter())
            .find_map(|graph| {
                graph
                    .connections
                    .iter()
                    .find(|bus| bus.id == item.id)
                    .map(|bus| {
                        format!(
                            "{}.{} → {}.{} · {:?}",
                            bus.from.node, bus.from.port, bus.to.node, bus.to.port, graph.domain
                        )
                    })
            })
            .unwrap_or_else(|| "missing Bus".to_owned()),
        PresentationItemKind::Parameter => instrument
            .parameters
            .iter()
            .find(|parameter| parameter.key == item.id)
            .map(|parameter| {
                format!(
                    "{} · {} {} · {}..{}",
                    parameter.name,
                    parameter.value,
                    parameter.unit,
                    parameter.minimum,
                    parameter.maximum
                )
            })
            .unwrap_or_else(|| "missing Parameter".to_owned()),
        PresentationItemKind::Source => instrument
            .modulation_sources
            .iter()
            .find(|source| source.id == item.id)
            .map(|source| {
                format!(
                    "{:?} · {:?} · {:?}",
                    source.kind, source.scope, source.polarity
                )
            })
            .unwrap_or_else(|| "missing source".to_owned()),
        PresentationItemKind::Route => instrument
            .modulation_routes
            .iter()
            .find(|route| route.id == item.id)
            .map(|route| {
                format!(
                    "{} → {} · depth {} · order {}{}",
                    route.source,
                    route.target,
                    route.depth,
                    route.order,
                    if route.bypass { " · bypassed" } else { "" }
                )
            })
            .unwrap_or_else(|| "missing route".to_owned()),
        PresentationItemKind::Processor => instrument
            .post_processors
            .iter()
            .find(|processor| processor.id == item.id)
            .map(|processor| format!("{:?}", processor.kind))
            .unwrap_or_else(|| "missing processor".to_owned()),
        PresentationItemKind::Asset => project
            .assets
            .iter()
            .find(|asset| asset.id == item.id)
            .map(|asset| asset_detail(asset, assets))
            .unwrap_or_else(|| "missing Project asset".to_owned()),
    }
}

fn node_detail(node: &crate::model::GraphNode, domain: GraphDomain) -> String {
    match node.kind {
        NodeKind::SampleSource => {
            let slices = node
                .directives
                .iter()
                .filter(|directive| {
                    directive
                        .tokens
                        .first()
                        .is_some_and(|token| token.value == "slice")
                })
                .map(|directive| {
                    format!(
                        "{}:{}..{}",
                        directive.tokens[1].value,
                        directive.tokens[2].value,
                        directive.tokens[3].value
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("SampleSource · {domain:?} graph · slices [{slices}]")
        }
        NodeKind::Multisample => {
            let zones = node
                .directives
                .iter()
                .filter(|directive| {
                    directive
                        .tokens
                        .first()
                        .is_some_and(|token| token.value == "zone")
                })
                .map(|directive| {
                    format!(
                        "{}:{} {}..{} v{}..{} rr {}/{}",
                        directive.tokens[1].value,
                        directive.tokens[3].value,
                        directive.tokens[5].value,
                        directive.tokens[6].value,
                        directive.tokens[8].value,
                        directive.tokens[9].value,
                        directive.tokens[17].value,
                        directive.tokens[18].value
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("Multisample · {domain:?} graph · zones [{zones}]")
        }
        NodeKind::Granular | NodeKind::Spectral => {
            let settings = node
                .settings
                .iter()
                .map(|setting| format!("{}={:?}", setting.key, setting.value))
                .collect::<Vec<_>>()
                .join(", ");
            let source = if node.kind == NodeKind::Granular {
                "Granular"
            } else {
                "Spectral"
            };
            format!("{source} · {domain:?} graph · {settings}")
        }
        _ => format!("{:?} · {domain:?} graph", node.kind),
    }
}

fn asset_detail(asset: &MediaAsset, assets: Option<&AssetInventory>) -> String {
    let Some(definition) = asset.wavetable.as_ref() else {
        let loaded = assets.and_then(|assets| assets.get(&asset.id));
        let sample_detail = loaded.map_or_else(String::new, |loaded| {
            let metadata = loaded.metadata();
            let waveform = sample_waveform_preview(loaded)
                .map_or_else(String::new, |waveform| format!(" · wave {waveform}"));
            format!(
                " · {}ch · {}Hz · {} frames{waveform}",
                metadata.channels, metadata.sample_rate, metadata.frames
            )
        });
        return format!(
            "{}{} · {} · sha256 {}",
            asset.name,
            sample_detail,
            asset.path.display(),
            asset.sha256
        );
    };
    let waveform = assets
        .and_then(|assets| assets.get(&asset.id))
        .and_then(|loaded| waveform_preview(loaded, definition));
    format!(
        "{} · {} frames × {} samples · {:?} · {:?}{} · {} · sha256 {}",
        asset.name,
        definition.frames,
        definition.frame_length,
        definition.normalization,
        definition.morph,
        waveform.map_or_else(String::new, |waveform| format!(" · wave {waveform}")),
        asset.path.display(),
        asset.sha256
    )
}

fn sample_waveform_preview(asset: &LoadedAsset) -> Option<String> {
    const POINTS: usize = 24;
    const LEVELS: [char; 9] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█', '▉'];
    let metadata = asset.metadata();
    if metadata.frames == 0 {
        return None;
    }
    let mut reader = hound::WavReader::new(Cursor::new(asset.bytes())).ok()?;
    let mut preview = String::with_capacity(POINTS);
    for point in 0..POINTS {
        let frame_index = u64::try_from(point).ok()?.saturating_mul(metadata.frames)
            / u64::try_from(POINTS).ok()?;
        let frame = preview_frame(&mut reader, metadata, frame_index)?;
        let sample = ((f64::from(frame[0]) + f64::from(frame[1])) * 0.5).clamp(-1.0, 1.0);
        let level = (((sample + 1.0) * 4.0).round() as usize).min(LEVELS.len() - 1);
        preview.push(LEVELS[level]);
    }
    Some(preview)
}

fn waveform_preview(
    asset: &LoadedAsset,
    definition: &crate::model::WavetableAssetDefinition,
) -> Option<String> {
    const POINTS: usize = 12;
    const FRAMES: usize = 8;
    const LEVELS: [char; 9] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█', '▉'];
    let metadata = asset.metadata();
    let frame_length = u64::from(definition.frame_length);
    let frame_count = usize::from(definition.frames);
    if metadata.frames != frame_length.checked_mul(u64::try_from(frame_count).ok()?)? {
        return None;
    }
    let mut reader = hound::WavReader::new(Cursor::new(asset.bytes())).ok()?;
    let shown = frame_count.min(FRAMES);
    let mut preview = String::new();
    for frame in 0..shown {
        if frame > 0 {
            preview.push('│');
        }
        for point in 0..POINTS {
            let local = u64::try_from(point).ok()?.saturating_mul(frame_length)
                / u64::try_from(POINTS).ok()?;
            let frame_index = u64::try_from(frame)
                .ok()?
                .saturating_mul(frame_length)
                .saturating_add(local);
            let sample =
                f64::from(preview_frame(&mut reader, metadata, frame_index)?[0]).clamp(-1.0, 1.0);
            let level = (((sample + 1.0) * 4.0).round() as usize).min(LEVELS.len() - 1);
            preview.push(LEVELS[level]);
        }
    }
    if shown < frame_count {
        preview.push_str(&format!("│…+{}", frame_count - shown));
    }
    Some(preview)
}

fn preview_frame(
    reader: &mut hound::WavReader<Cursor<&[u8]>>,
    metadata: WavMetadata,
    frame: u64,
) -> Option<[f32; 2]> {
    reader.seek(u32::try_from(frame).ok()?).ok()?;
    let mut next_sample = || match metadata.sample_format {
        WavSampleFormat::Integer => {
            let sample = reader.samples::<i32>().next()?.ok()?;
            let scale = 2.0_f64.powi(i32::from(metadata.bits_per_sample) - 1);
            Some((f64::from(sample) / scale).clamp(-1.0, 1.0) as f32)
        }
        WavSampleFormat::Float => {
            let sample = reader.samples::<f32>().next()?.ok()?;
            sample.is_finite().then_some(sample)
        }
    };
    let left = next_sample()?;
    let right = if metadata.channels == 1 {
        left
    } else {
        next_sample()?
    };
    Some([left, right])
}

fn node_section(kind: NodeKind, domain: GraphDomain) -> PresentationSectionKind {
    match kind {
        NodeKind::Exciter | NodeKind::ModalBank => PresentationSectionKind::Sources,
        NodeKind::Oscillator
        | NodeKind::Unison
        | NodeKind::AnalogOscillator
        | NodeKind::Noise
        | NodeKind::Wavetable
        | NodeKind::SampleSource
        | NodeKind::Multisample
        | NodeKind::Granular
        | NodeKind::Spectral => PresentationSectionKind::Sources,
        NodeKind::Envelope | NodeKind::Lfo => PresentationSectionKind::EnvelopesLfos,
        NodeKind::Lowpass | NodeKind::StereoFilter => PresentationSectionKind::Filters,
        NodeKind::Delay | NodeKind::Reverb | NodeKind::Saturation
            if domain == GraphDomain::Shared =>
        {
            PresentationSectionKind::Effects
        }
        NodeKind::Mixer
        | NodeKind::StereoMixer
        | NodeKind::StereoSum
        | NodeKind::MonoToStereo
        | NodeKind::StereoToMono
        | NodeKind::Amplifier
        | NodeKind::Pan
        | NodeKind::Saturation => PresentationSectionKind::SourceMixer,
        NodeKind::Delay | NodeKind::Reverb => PresentationSectionKind::Effects,
        NodeKind::Output => PresentationSectionKind::Routing,
    }
}

fn append_row(
    sections: &mut [InstrumentSectionView],
    kind: PresentationSectionKind,
    row: InstrumentSectionRow,
) {
    if let Some(section) = sections.iter_mut().find(|section| section.kind == kind) {
        section.rows.push(row);
    }
}

fn section_title(kind: PresentationSectionKind) -> &'static str {
    match kind {
        PresentationSectionKind::Sources => "Sources/Oscillators",
        PresentationSectionKind::SourceMixer => "Source Mixer",
        PresentationSectionKind::Filters => "Filters",
        PresentationSectionKind::EnvelopesLfos => "Envelopes/LFOs",
        PresentationSectionKind::Modulation => "Modulation",
        PresentationSectionKind::Voicing => "Voicing/Unison/Glide",
        PresentationSectionKind::Routing => "Routing",
        PresentationSectionKind::Effects => "Shared Effects",
    }
}

fn location_label(location: &SourceLocation) -> String {
    format!(
        "{}:{}:{}",
        location.file.display(),
        location.line,
        location.column
    )
}

fn site_label(site: &ValueSite) -> String {
    format!("{}:{}..{}", site.file.display(), site.start, site.end)
}
