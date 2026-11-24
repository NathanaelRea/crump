//! Validation-only presentation for canonical authored routing findings.

use std::collections::BTreeSet;

use crate::{
    diagnostic::{Diagnostic, Diagnostics, SourceLocation},
    model::{ChannelStrip, Panner, Processor, ProcessorKind, Project},
    processor_semantics::{
        ProcessorSettingError, validate_authored_setting, validate_relationships,
    },
    routing::{AuthoredTopology, ChannelIdentity, ChannelRole, RoutingFindingKind},
};

use super::{located_error, source_site_error, valid_key};

pub(super) fn validate_mix_graph(
    project: &Project,
    topology: &AuthoredTopology,
    diagnostics: &mut Diagnostics,
) {
    for (index, group) in project.song.groups.iter().enumerate() {
        if has_finding(topology, |kind| {
            matches!(kind,
                RoutingFindingKind::InvalidChannelId { role: ChannelRole::Group, index: found, .. }
                    if *found == index
            )
        }) {
            located_error(
                diagnostics,
                "CRV100",
                format!("Group id `{}` is invalid or duplicated", group.id),
                &group.location,
            );
        }
    }
    for (index, channel) in project.song.returns.iter().enumerate() {
        if has_finding(topology, |kind| {
            matches!(kind,
                RoutingFindingKind::InvalidChannelId { role: ChannelRole::Return, index: found, .. }
                    if *found == index
            )
        }) {
            located_error(
                diagnostics,
                "CRV101",
                format!("Return id `{}` is invalid or duplicated", channel.id),
                &channel.location,
            );
        }
    }

    for instrument in &project.song.instruments {
        for processor in &instrument.post_processors {
            validate_processor(processor, project, topology, diagnostics);
        }
    }
    for track in &project.song.tracks {
        validate_channel_strip(
            &ChannelIdentity::Track(track.id.clone()),
            "Track",
            &track.channel,
            &track.location,
            project,
            topology,
            diagnostics,
        );
    }
    for group in &project.song.groups {
        validate_channel_strip(
            &ChannelIdentity::Group(group.id.clone()),
            "Group",
            &group.channel,
            &group.location,
            project,
            topology,
            diagnostics,
        );
    }
    for channel in &project.song.returns {
        validate_channel_strip(
            &ChannelIdentity::Return(channel.id.clone()),
            "Return",
            &channel.channel,
            &channel.location,
            project,
            topology,
            diagnostics,
        );
    }
    if let Some(master) = &project.song.master {
        if !master.level_db.is_finite() || !(-96.0..=12.0).contains(&master.level_db) {
            source_site_error(
                diagnostics,
                "CRV102",
                "Master level must be between -96db and 12db",
                &master.level_site,
            );
        }
        for processor in &master.inserts {
            validate_processor(processor, project, topology, diagnostics);
        }
    }

    for (owner, channel, role, location) in authored_channel_strips(project) {
        if let Some((_, destination)) =
            topology
                .findings()
                .iter()
                .find_map(|finding| match &finding.kind {
                    RoutingFindingKind::UnresolvedMain {
                        owner: found,
                        destination,
                        ..
                    } if found == &owner => Some((found, destination)),
                    _ => None,
                })
        {
            located_error(
                diagnostics,
                "CRV103",
                format!("{role} `{owner}` routes to missing `{destination}`"),
                location,
            );
        }
        if has_finding(topology, |kind| {
            matches!(kind,
                RoutingFindingKind::IllegalReturnMain { owner: found, .. } if found == &owner
            )
        }) {
            located_error(
                diagnostics,
                "CRV104",
                format!("{role} `{owner}` cannot use a Return as its main output"),
                location,
            );
        }
        for send in &channel.sends {
            if finding_at(
                topology,
                &send.location,
                |kind| matches!(kind, RoutingFindingKind::InvalidSendId { id } if id == &send.id),
            ) {
                located_error(
                    diagnostics,
                    "CRV105",
                    format!("Send id `{}` is invalid or duplicated", send.id),
                    &send.location,
                );
            }
            if finding_at(
                topology,
                &send.location,
                |kind| matches!(kind, RoutingFindingKind::MissingSendReturn { send: id, .. } if id == &send.id),
            ) {
                located_error(
                    diagnostics,
                    "CRV106",
                    format!(
                        "Send `{}` targets missing Return `{}`",
                        send.id, send.target
                    ),
                    &send.location,
                );
            }
            if !send.level_db.is_finite() || !(-96.0..=12.0).contains(&send.level_db) {
                source_site_error(
                    diagnostics,
                    "CRV107",
                    "Send level must be between -96db and 12db",
                    &send.level_site,
                );
            }
        }
    }

    for sidechain in &project.song.sidechains {
        if finding_at(
            topology,
            &sidechain.location,
            |kind| matches!(kind, RoutingFindingKind::InvalidSidechainId { id } if id == &sidechain.id),
        ) {
            located_error(
                diagnostics,
                "CRV108",
                format!(
                    "sidechain route id `{}` is invalid or duplicated",
                    sidechain.id
                ),
                &sidechain.location,
            );
        }
        if finding_at(
            topology,
            &sidechain.location,
            |kind| matches!(kind, RoutingFindingKind::MissingSidechainSource { sidechain: id, .. } if id == &sidechain.id),
        ) {
            located_error(
                diagnostics,
                "CRV109",
                format!(
                    "sidechain `{}` references missing source `{}`",
                    sidechain.id, sidechain.from
                ),
                &sidechain.location,
            );
        }
        if finding_at(
            topology,
            &sidechain.location,
            |kind| matches!(kind, RoutingFindingKind::MissingSidechainProcessor { sidechain: id, .. } if id == &sidechain.id),
        ) {
            located_error(
                diagnostics,
                "CRV110",
                format!(
                    "sidechain `{}` references a missing or non-channel processor `{}`",
                    sidechain.id, sidechain.processor
                ),
                &sidechain.location,
            );
            continue;
        }
        if finding_at(
            topology,
            &sidechain.location,
            |kind| matches!(kind, RoutingFindingKind::UndeclaredSidechainInput { sidechain: id, .. } if id == &sidechain.id),
        ) {
            located_error(
                diagnostics,
                "CRV111",
                format!(
                    "sidechain `{}` targets undeclared input `{}` on processor `{}`",
                    sidechain.id, sidechain.input, sidechain.processor
                ),
                &sidechain.location,
            );
        }
        if finding_at(
            topology,
            &sidechain.location,
            |kind| matches!(kind, RoutingFindingKind::DuplicateSidechainDestination { processor, input } if processor == &sidechain.processor && input == &sidechain.input),
        ) {
            located_error(
                diagnostics,
                "CRV112",
                format!(
                    "processor `{}` input `{}` has more than one sidechain route",
                    sidechain.processor, sidechain.input
                ),
                &sidechain.location,
            );
        }
    }
    for processor in project
        .song
        .instruments
        .iter()
        .flat_map(|instrument| &instrument.post_processors)
        .chain(channel_processors(project))
    {
        for input in &processor.sidechain_inputs {
            if has_finding(topology, |kind| {
                matches!(kind,
                    RoutingFindingKind::UnconnectedSidechainInput { processor: id, input: found }
                        if id == &processor.id && found == input
                )
            }) {
                located_error(
                    diagnostics,
                    "CRV135",
                    format!(
                        "processor `{}` sidechain input `{input}` has no route",
                        processor.id
                    ),
                    &processor.location,
                );
            }
        }
    }
    if has_finding(topology, |kind| matches!(kind, RoutingFindingKind::Cycle)) {
        diagnostics.push(
            Diagnostic::error(
                "CRV113",
                "channel, Send, or sidechain routing contains a cycle",
            )
            .at(project.song.location.clone())
            .with_help(
                "route every main, Send, and auxiliary sidechain path acyclically toward Master",
            ),
        );
    }

    let mut locator_ids = BTreeSet::new();
    for locator in &project.song.locators {
        if !super::valid_id(&locator.id) || !locator_ids.insert(locator.id.as_str()) {
            located_error(
                diagnostics,
                "CRV114",
                format!("Locator id `{}` is invalid or duplicated", locator.id),
                &locator.location,
            );
        }
        if locator.at < crate::time::MusicalTime::ZERO || locator.at > project.song.end {
            source_site_error(
                diagnostics,
                "CRV115",
                "Locator time must be within the Song arrangement",
                &locator.at_site,
            );
        }
    }
}

fn authored_channel_strips(
    project: &Project,
) -> impl Iterator<
    Item = (
        ChannelIdentity,
        &ChannelStrip,
        &'static str,
        &SourceLocation,
    ),
> {
    project
        .song
        .tracks
        .iter()
        .map(|track| {
            (
                ChannelIdentity::Track(track.id.clone()),
                &track.channel,
                "Track",
                &track.location,
            )
        })
        .chain(project.song.groups.iter().map(|group| {
            (
                ChannelIdentity::Group(group.id.clone()),
                &group.channel,
                "Group",
                &group.location,
            )
        }))
        .chain(project.song.returns.iter().map(|channel| {
            (
                ChannelIdentity::Return(channel.id.clone()),
                &channel.channel,
                "Return",
                &channel.location,
            )
        }))
}

fn validate_channel_strip(
    owner: &ChannelIdentity,
    role: &str,
    channel: &ChannelStrip,
    location: &SourceLocation,
    project: &Project,
    topology: &AuthoredTopology,
    diagnostics: &mut Diagnostics,
) {
    if !channel.level_db.is_finite() || !(-96.0..=12.0).contains(&channel.level_db) {
        source_site_error(
            diagnostics,
            "CRV116",
            format!("{role} level must be between -96db and 12db"),
            &channel.level_site,
        );
    }
    let positions = match &channel.panner {
        Panner::MonoPan { position, site } | Panner::Balance { position, site } => {
            vec![(*position, site)]
        }
        Panner::StereoPan {
            left,
            right,
            left_site,
            right_site,
        } => vec![(*left, left_site), (*right, right_site)],
    };
    for (position, site) in positions {
        if !position.is_finite() || !(-1.0..=1.0).contains(&position) {
            source_site_error(
                diagnostics,
                "CRV117",
                format!("{role} panner positions must be between -1 and 1"),
                site,
            );
        }
    }
    if has_finding(
        topology,
        |kind| matches!(kind, RoutingFindingKind::UnresolvedMain { owner: found, .. } if found == owner),
    ) {
        located_error(
            diagnostics,
            "CRV118",
            format!("{role} `{owner}` has an unresolved main output"),
            location,
        );
    }
    for processor in &channel.inserts {
        validate_processor(processor, project, topology, diagnostics);
    }
}

fn validate_processor(
    processor: &Processor,
    project: &Project,
    topology: &AuthoredTopology,
    diagnostics: &mut Diagnostics,
) {
    if finding_at(
        topology,
        &processor.location,
        |kind| matches!(kind, RoutingFindingKind::InvalidProcessorId { id } if id == &processor.id),
    ) {
        located_error(
            diagnostics,
            "CRV119",
            format!("processor id `{}` is invalid or duplicated", processor.id),
            &processor.location,
        );
    }
    if u64::from(processor.latency_frames) > u64::from(project.sample_rate).saturating_mul(10) {
        located_error(
            diagnostics,
            "CRV120",
            format!(
                "processor `{}` declares more than ten seconds of latency",
                processor.id
            ),
            &processor.location,
        );
    }
    let mut keys = BTreeSet::new();
    for setting in &processor.settings {
        if !valid_key(&setting.key) || !keys.insert(setting.key.as_str()) {
            located_error(
                diagnostics,
                "CRV121",
                format!(
                    "processor `{}` setting key `{}` is invalid or duplicated",
                    processor.id, setting.key
                ),
                &setting.location,
            );
            continue;
        }
        match validate_authored_setting(processor.kind, &setting.key, &setting.value) {
            Ok(_) => {}
            Err(ProcessorSettingError::OutOfRange {
                unit,
                minimum,
                maximum,
                ..
            }) => source_site_error(
                diagnostics,
                "CRV122",
                format!(
                    "processor `{}` setting `{}` must be within {}..={} {:?}",
                    processor.id, setting.key, minimum, maximum, unit
                ),
                &setting.value_site,
            ),
            Err(ProcessorSettingError::InvalidPhysical { source, .. }) => source_site_error(
                diagnostics,
                "CRV123",
                format!(
                    "processor `{}` setting `{}` has an invalid physical literal: {source}",
                    processor.id, setting.key
                ),
                &setting.value_site,
            ),
            Err(
                ProcessorSettingError::Unsupported { .. }
                | ProcessorSettingError::Conflict(_)
                | ProcessorSettingError::InvalidTempoDivision { .. },
            ) => source_site_error(
                diagnostics,
                "CRV124",
                format!(
                    "processor `{}` kind {:?} does not accept `{} {}`",
                    processor.id, processor.kind, setting.key, setting.value
                ),
                &setting.value_site,
            ),
        }
    }
    if processor.kind == ProcessorKind::Convolution {
        match crate::processor_semantics::setting_literal(processor, "ir") {
            None => located_error(
                diagnostics,
                "CRV124",
                format!(
                    "convolution `{}` requires setting `ir` with a Media Asset ID",
                    processor.id
                ),
                &processor.location,
            ),
            Some(id)
                if !project.assets.iter().any(|asset| {
                    asset.id == id
                        && asset
                            .usages
                            .contains(&crate::model::MediaAssetUsage::ImpulseResponse)
                }) =>
            {
                located_error(
                    diagnostics,
                    "CRV124",
                    format!(
                        "convolution `{}` requires Media Asset `{id}` declared with `usage impulse_response`",
                        processor.id
                    ),
                    &processor.location,
                )
            }
            Some(_) => {}
        }
    }
    for conflict in validate_relationships(processor) {
        if let Some(setting) = processor
            .settings
            .iter()
            .find(|setting| setting.key == conflict.key())
        {
            source_site_error(
                diagnostics,
                "CRV136",
                format!("delay `{}` {conflict}", processor.id),
                &setting.value_site,
            );
        }
    }
    if !matches!(
        processor.kind,
        ProcessorKind::Compressor | ProcessorKind::Gate
    ) && !processor.sidechain_inputs.is_empty()
    {
        located_error(
            diagnostics,
            "CRV133",
            format!(
                "processor `{}` kind {:?} does not consume sidechain Audio",
                processor.id, processor.kind
            ),
            &processor.location,
        );
    }
    if matches!(
        processor.kind,
        ProcessorKind::Compressor | ProcessorKind::Gate
    ) && processor.sidechain_inputs.len() > 1
    {
        located_error(
            diagnostics,
            "CRV134",
            format!(
                "sidechain processor `{}` accepts at most one named sidechain input",
                processor.id
            ),
            &processor.location,
        );
    }
    let mut inputs = BTreeSet::new();
    for input in &processor.sidechain_inputs {
        if !valid_key(input) || !inputs.insert(input.as_str()) {
            located_error(
                diagnostics,
                "CRV125",
                format!(
                    "processor `{}` sidechain input `{input}` is invalid or duplicated",
                    processor.id
                ),
                &processor.location,
            );
        }
    }
}

fn channel_processors(project: &Project) -> impl Iterator<Item = &Processor> {
    project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.channel.inserts)
        .chain(
            project
                .song
                .groups
                .iter()
                .flat_map(|group| &group.channel.inserts),
        )
        .chain(
            project
                .song
                .returns
                .iter()
                .flat_map(|channel| &channel.channel.inserts),
        )
        .chain(
            project
                .song
                .master
                .iter()
                .flat_map(|master| &master.inserts),
        )
}

fn has_finding(
    topology: &AuthoredTopology,
    predicate: impl Fn(&RoutingFindingKind) -> bool,
) -> bool {
    topology
        .findings()
        .iter()
        .any(|finding| predicate(&finding.kind))
}

fn finding_at(
    topology: &AuthoredTopology,
    location: &SourceLocation,
    predicate: impl Fn(&RoutingFindingKind) -> bool,
) -> bool {
    topology
        .findings()
        .iter()
        .any(|finding| &finding.location == location && predicate(&finding.kind))
}
