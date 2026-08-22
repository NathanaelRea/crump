//! Canonical compilation of authored mixer routing.
//!
//! This module owns channel and processor identity, authored Buses, Sends,
//! sidechains, cycle analysis, and stable processing order. It is pure
//! in-process domain code: validation presents its findings and the audio
//! engine lowers its topology without reconstructing authored routing.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use serde::{Deserialize, Serialize};

use crate::{
    diagnostic::SourceLocation,
    model::{ChannelRef, ChannelStrip, OutputTarget, Project, TapPoint},
};

/// Domain role of an authored mixer channel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ChannelRole {
    /// Timeline Track source and channel strip.
    Track,
    /// Hierarchical summing Group.
    Group,
    /// Parallel Return.
    Return,
    /// Singleton final Master.
    Master,
}

impl ChannelRole {
    /// Human-facing role spelling used by diagnostics and inspection.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Track => "Track",
            Self::Group => "Group",
            Self::Return => "Return",
            Self::Master => "Master",
        }
    }
}

/// Stable typed identity of one authored mixer channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelIdentity {
    /// Timeline Track ID.
    Track(String),
    /// Group ID.
    Group(String),
    /// Return ID.
    Return(String),
    /// Intrinsic singleton Master identity.
    Master,
}

impl ChannelIdentity {
    /// Returns the channel's domain role.
    #[must_use]
    pub const fn role(&self) -> ChannelRole {
        match self {
            Self::Track(_) => ChannelRole::Track,
            Self::Group(_) => ChannelRole::Group,
            Self::Return(_) => ChannelRole::Return,
            Self::Master => ChannelRole::Master,
        }
    }

    fn from_ref(reference: &ChannelRef) -> Self {
        match reference {
            ChannelRef::Track(id) => Self::Track(id.clone()),
            ChannelRef::Group(id) => Self::Group(id.clone()),
            ChannelRef::Return(id) => Self::Return(id.clone()),
        }
    }

    fn from_output(output: &OutputTarget) -> Self {
        match output {
            OutputTarget::Master => Self::Master,
            OutputTarget::Group(id) => Self::Group(id.clone()),
            OutputTarget::Return(id) => Self::Return(id.clone()),
        }
    }
}

impl PartialOrd for ChannelIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ChannelIdentity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.to_string().cmp(&other.to_string())
    }
}

impl fmt::Display for ChannelIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Track(id) => write!(formatter, "track/{id}"),
            Self::Group(id) => write!(formatter, "group/{id}"),
            Self::Return(id) => write!(formatter, "return/{id}"),
            Self::Master => formatter.write_str("master"),
        }
    }
}

/// Source declaration that owns a canonical channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelDeclaration {
    /// Index in `Song::tracks`.
    Track(usize),
    /// Index in `Song::groups`.
    Group(usize),
    /// Index in `Song::returns`.
    Return(usize),
    /// Singleton Master declaration (which may be implicit).
    Master,
}

/// Canonical authored channel facts.
#[derive(Debug, Clone)]
pub struct TopologyChannel {
    identity: ChannelIdentity,
    declaration: ChannelDeclaration,
    location: SourceLocation,
}

impl TopologyChannel {
    /// Stable typed identity.
    #[must_use]
    pub fn identity(&self) -> &ChannelIdentity {
        &self.identity
    }

    /// Source declaration locator.
    #[must_use]
    pub const fn declaration(&self) -> ChannelDeclaration {
        self.declaration
    }

    /// Source location of the declaration.
    #[must_use]
    pub fn location(&self) -> &SourceLocation {
        &self.location
    }
}

/// Authored owner of one processor declaration.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcessorOwner {
    /// Reusable Instrument post chain.
    Instrument {
        instrument_index: usize,
        instrument_id: String,
    },
    /// Mixer channel insert chain.
    Channel {
        channel: ChannelIdentity,
        processor_index: usize,
    },
}

/// Canonical processor identity and ownership.
#[derive(Debug, Clone)]
pub struct TopologyProcessor {
    id: String,
    owner: ProcessorOwner,
    location: SourceLocation,
    sidechain_inputs: Vec<String>,
}

impl TopologyProcessor {
    /// Song-wide durable processor ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Exact authored chain owner and position.
    #[must_use]
    pub fn owner(&self) -> &ProcessorOwner {
        &self.owner
    }

    /// Declared auxiliary Audio inputs.
    #[must_use]
    pub fn sidechain_inputs(&self) -> &[String] {
        &self.sidechain_inputs
    }

    /// Processor declaration location.
    #[must_use]
    pub fn location(&self) -> &SourceLocation {
        &self.location
    }
}

/// Kind of one canonical authored routing edge.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TopologyRouteRole {
    Main,
    Send,
    Sidechain,
}

/// Authored declaration behind one route.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDeclaration {
    /// Main output of the named channel declaration.
    Main(ChannelDeclaration),
    /// Send at an index within a channel strip.
    Send {
        channel: ChannelDeclaration,
        send_index: usize,
    },
    /// Index in `Song::sidechains`.
    Sidechain(usize),
}

/// Typed destination of an authored route.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyDestination {
    /// Main or Send Bus into a channel's summing input.
    Bus(ChannelIdentity),
    /// Named auxiliary input on a channel-owned processor.
    SidechainInput {
        owner: ChannelIdentity,
        processor_id: String,
        processor_index: usize,
        input: String,
    },
}

/// One stable canonical route, sorted by durable route identity.
#[derive(Debug, Clone)]
pub struct TopologyRoute {
    id: String,
    role: TopologyRouteRole,
    source: ChannelIdentity,
    owner: ChannelIdentity,
    destination: TopologyDestination,
    tap: TapPoint,
    declaration: RouteDeclaration,
}

#[allow(missing_docs)]
impl TopologyRoute {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    #[must_use]
    pub const fn role(&self) -> TopologyRouteRole {
        self.role
    }
    #[must_use]
    pub fn source(&self) -> &ChannelIdentity {
        &self.source
    }
    /// Channel whose convergence point owns this route.
    #[must_use]
    pub fn owner(&self) -> &ChannelIdentity {
        &self.owner
    }
    #[must_use]
    pub fn destination(&self) -> &TopologyDestination {
        &self.destination
    }
    #[must_use]
    pub const fn tap(&self) -> TapPoint {
        self.tap
    }
    #[must_use]
    pub const fn declaration(&self) -> RouteDeclaration {
        self.declaration
    }
}

/// A source-located authored-routing problem. Validation owns presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingFinding {
    /// Typed condition discovered by canonical compilation.
    pub kind: RoutingFindingKind,
    /// Most specific authored source location.
    pub location: SourceLocation,
}

/// Typed authored-routing findings shared by validation and engine admission.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingFindingKind {
    InvalidChannelId {
        role: ChannelRole,
        index: usize,
        id: String,
    },
    InvalidProcessorId {
        id: String,
    },
    UnresolvedMain {
        role: ChannelRole,
        owner: ChannelIdentity,
        destination: ChannelIdentity,
    },
    IllegalReturnMain {
        role: ChannelRole,
        owner: ChannelIdentity,
    },
    InvalidSendId {
        id: String,
    },
    MissingSendReturn {
        send: String,
        target: String,
    },
    InvalidSidechainId {
        id: String,
    },
    MissingSidechainSource {
        sidechain: String,
        source: ChannelIdentity,
    },
    MissingSidechainProcessor {
        sidechain: String,
        processor: String,
    },
    UndeclaredSidechainInput {
        sidechain: String,
        input: String,
        processor: String,
    },
    DuplicateSidechainDestination {
        processor: String,
        input: String,
    },
    UnconnectedSidechainInput {
        processor: String,
        input: String,
    },
    Cycle,
}

/// Canonical typed topology plus non-fatal authored findings.
#[derive(Debug, Clone)]
pub struct AuthoredTopology {
    channels: Vec<TopologyChannel>,
    processors: Vec<TopologyProcessor>,
    routes: Vec<TopologyRoute>,
    order: Vec<ChannelIdentity>,
    findings: Vec<RoutingFinding>,
}

#[allow(missing_docs)]
impl AuthoredTopology {
    /// Compiles the complete authored routing model exactly once.
    #[must_use]
    pub fn compile(project: &Project) -> Self {
        Compiler::new(project).compile()
    }

    #[must_use]
    pub fn channels(&self) -> &[TopologyChannel] {
        &self.channels
    }
    #[must_use]
    pub fn processors(&self) -> &[TopologyProcessor] {
        &self.processors
    }
    #[must_use]
    pub fn routes(&self) -> &[TopologyRoute] {
        &self.routes
    }
    /// Stable channel processing order, independent of declaration order.
    #[must_use]
    pub fn order(&self) -> &[ChannelIdentity] {
        &self.order
    }
    #[must_use]
    pub fn findings(&self) -> &[RoutingFinding] {
        &self.findings
    }
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }

    #[must_use]
    pub fn channel(&self, identity: &ChannelIdentity) -> Option<&TopologyChannel> {
        self.channels
            .iter()
            .find(|channel| channel.identity == *identity)
    }

    #[must_use]
    pub fn processor(&self, id: &str) -> Option<&TopologyProcessor> {
        self.processors.iter().find(|processor| processor.id == id)
    }
}

struct Compiler<'a> {
    project: &'a Project,
    channels: BTreeMap<ChannelIdentity, TopologyChannel>,
    processors: Vec<TopologyProcessor>,
    processor_indexes: BTreeMap<String, usize>,
    routes: Vec<TopologyRoute>,
    findings: Vec<RoutingFinding>,
    channel_ids: BTreeSet<(ChannelRole, String)>,
    processor_ids: BTreeSet<String>,
    send_ids: BTreeSet<String>,
    sidechain_ids: BTreeSet<String>,
}

impl<'a> Compiler<'a> {
    fn new(project: &'a Project) -> Self {
        Self {
            project,
            channels: BTreeMap::new(),
            processors: Vec::new(),
            processor_indexes: BTreeMap::new(),
            routes: Vec::new(),
            findings: Vec::new(),
            channel_ids: BTreeSet::new(),
            processor_ids: BTreeSet::new(),
            send_ids: BTreeSet::new(),
            sidechain_ids: BTreeSet::new(),
        }
    }

    fn compile(mut self) -> AuthoredTopology {
        self.collect_channels();
        self.collect_processors();
        self.collect_main_and_send_routes();
        self.collect_sidechains();
        self.find_unconnected_sidechains();
        self.routes.sort_by(|left, right| left.id.cmp(&right.id));
        let (order, cycle) = stable_order(self.channels.keys(), &self.routes);
        if cycle {
            self.findings.push(RoutingFinding {
                kind: RoutingFindingKind::Cycle,
                location: self.project.song.location.clone(),
            });
        }
        AuthoredTopology {
            channels: self.channels.into_values().collect(),
            processors: self.processors,
            routes: self.routes,
            order,
            findings: self.findings,
        }
    }

    fn collect_channels(&mut self) {
        for (index, track) in self.project.song.tracks.iter().enumerate() {
            self.insert_channel(
                ChannelIdentity::Track(track.id.clone()),
                ChannelDeclaration::Track(index),
                track.location.clone(),
                index,
            );
        }
        for (index, group) in self.project.song.groups.iter().enumerate() {
            self.insert_channel(
                ChannelIdentity::Group(group.id.clone()),
                ChannelDeclaration::Group(index),
                group.location.clone(),
                index,
            );
        }
        for (index, channel) in self.project.song.returns.iter().enumerate() {
            self.insert_channel(
                ChannelIdentity::Return(channel.id.clone()),
                ChannelDeclaration::Return(index),
                channel.location.clone(),
                index,
            );
        }
        self.channels.insert(
            ChannelIdentity::Master,
            TopologyChannel {
                identity: ChannelIdentity::Master,
                declaration: ChannelDeclaration::Master,
                location: self.project.song.master.as_ref().map_or_else(
                    || self.project.song.location.clone(),
                    |master| master.location.clone(),
                ),
            },
        );
    }

    fn insert_channel(
        &mut self,
        identity: ChannelIdentity,
        declaration: ChannelDeclaration,
        location: SourceLocation,
        index: usize,
    ) {
        let id = match &identity {
            ChannelIdentity::Track(id)
            | ChannelIdentity::Group(id)
            | ChannelIdentity::Return(id) => id,
            ChannelIdentity::Master => unreachable!("Master is intrinsic"),
        };
        if !valid_id(id) || !self.channel_ids.insert((identity.role(), id.clone())) {
            self.findings.push(RoutingFinding {
                kind: RoutingFindingKind::InvalidChannelId {
                    role: identity.role(),
                    index,
                    id: id.clone(),
                },
                location: location.clone(),
            });
        }
        self.channels
            .entry(identity.clone())
            .or_insert(TopologyChannel {
                identity,
                declaration,
                location,
            });
    }

    fn collect_processors(&mut self) {
        for (instrument_index, instrument) in self.project.song.instruments.iter().enumerate() {
            for processor in &instrument.post_processors {
                self.insert_processor(
                    processor,
                    ProcessorOwner::Instrument {
                        instrument_index,
                        instrument_id: instrument.id.clone(),
                    },
                );
            }
        }
        let channels = authored_channels(self.project);
        for (identity, declaration, _) in channels {
            let strip = channel_strip(self.project, declaration)
                .expect("authored non-Master channel strip");
            for (processor_index, processor) in strip.inserts.iter().enumerate() {
                self.insert_processor(
                    processor,
                    ProcessorOwner::Channel {
                        channel: identity.clone(),
                        processor_index,
                    },
                );
            }
        }
        if let Some(master) = &self.project.song.master {
            for (processor_index, processor) in master.inserts.iter().enumerate() {
                self.insert_processor(
                    processor,
                    ProcessorOwner::Channel {
                        channel: ChannelIdentity::Master,
                        processor_index,
                    },
                );
            }
        }
    }

    fn insert_processor(&mut self, processor: &crate::model::Processor, owner: ProcessorOwner) {
        if !valid_id(&processor.id) || !self.processor_ids.insert(processor.id.clone()) {
            self.findings.push(RoutingFinding {
                kind: RoutingFindingKind::InvalidProcessorId {
                    id: processor.id.clone(),
                },
                location: processor.location.clone(),
            });
        }
        let index = self.processors.len();
        self.processor_indexes
            .entry(processor.id.clone())
            .or_insert(index);
        self.processors.push(TopologyProcessor {
            id: processor.id.clone(),
            owner,
            location: processor.location.clone(),
            sidechain_inputs: processor.sidechain_inputs.clone(),
        });
    }

    fn collect_main_and_send_routes(&mut self) {
        let channels = authored_channels(self.project);
        for (source, declaration, location) in channels {
            let strip = channel_strip(self.project, declaration)
                .expect("authored non-Master channel strip");
            let destination = ChannelIdentity::from_output(&strip.output);
            if !self.channels.contains_key(&destination) {
                self.findings.push(RoutingFinding {
                    kind: RoutingFindingKind::UnresolvedMain {
                        role: source.role(),
                        owner: source.clone(),
                        destination: destination.clone(),
                    },
                    location: location.clone(),
                });
            } else {
                self.routes.push(TopologyRoute {
                    id: format!("main/{source}"),
                    role: TopologyRouteRole::Main,
                    source: source.clone(),
                    owner: destination.clone(),
                    destination: TopologyDestination::Bus(destination),
                    tap: TapPoint::PostFader,
                    declaration: RouteDeclaration::Main(declaration),
                });
            }
            if source.role() != ChannelRole::Return
                && matches!(strip.output, OutputTarget::Return(_))
            {
                self.findings.push(RoutingFinding {
                    kind: RoutingFindingKind::IllegalReturnMain {
                        role: source.role(),
                        owner: source.clone(),
                    },
                    location: location.clone(),
                });
            }
            for (send_index, send) in strip.sends.iter().enumerate() {
                if !valid_id(&send.id) || !self.send_ids.insert(send.id.clone()) {
                    self.findings.push(RoutingFinding {
                        kind: RoutingFindingKind::InvalidSendId {
                            id: send.id.clone(),
                        },
                        location: send.location.clone(),
                    });
                }
                let destination = ChannelIdentity::Return(send.target.clone());
                if !self.channels.contains_key(&destination) {
                    self.findings.push(RoutingFinding {
                        kind: RoutingFindingKind::MissingSendReturn {
                            send: send.id.clone(),
                            target: send.target.clone(),
                        },
                        location: send.location.clone(),
                    });
                } else {
                    self.routes.push(TopologyRoute {
                        id: format!("send/{}", send.id),
                        role: TopologyRouteRole::Send,
                        source: source.clone(),
                        owner: destination.clone(),
                        destination: TopologyDestination::Bus(destination),
                        tap: send.tap,
                        declaration: RouteDeclaration::Send {
                            channel: declaration,
                            send_index,
                        },
                    });
                }
            }
        }
    }

    fn collect_sidechains(&mut self) {
        let mut destinations = BTreeSet::new();
        for (index, sidechain) in self.project.song.sidechains.iter().enumerate() {
            if !valid_id(&sidechain.id) || !self.sidechain_ids.insert(sidechain.id.clone()) {
                self.findings.push(RoutingFinding {
                    kind: RoutingFindingKind::InvalidSidechainId {
                        id: sidechain.id.clone(),
                    },
                    location: sidechain.location.clone(),
                });
            }
            let source = ChannelIdentity::from_ref(&sidechain.from);
            if !self.channels.contains_key(&source) {
                self.findings.push(RoutingFinding {
                    kind: RoutingFindingKind::MissingSidechainSource {
                        sidechain: sidechain.id.clone(),
                        source: source.clone(),
                    },
                    location: sidechain.location.clone(),
                });
            }
            let Some(processor_topology_index) =
                self.processor_indexes.get(&sidechain.processor).copied()
            else {
                self.findings.push(RoutingFinding {
                    kind: RoutingFindingKind::MissingSidechainProcessor {
                        sidechain: sidechain.id.clone(),
                        processor: sidechain.processor.clone(),
                    },
                    location: sidechain.location.clone(),
                });
                continue;
            };
            let processor = &self.processors[processor_topology_index];
            let ProcessorOwner::Channel {
                channel: owner,
                processor_index,
            } = &processor.owner
            else {
                self.findings.push(RoutingFinding {
                    kind: RoutingFindingKind::MissingSidechainProcessor {
                        sidechain: sidechain.id.clone(),
                        processor: sidechain.processor.clone(),
                    },
                    location: sidechain.location.clone(),
                });
                continue;
            };
            if !processor
                .sidechain_inputs
                .iter()
                .any(|input| input == &sidechain.input)
            {
                self.findings.push(RoutingFinding {
                    kind: RoutingFindingKind::UndeclaredSidechainInput {
                        sidechain: sidechain.id.clone(),
                        input: sidechain.input.clone(),
                        processor: sidechain.processor.clone(),
                    },
                    location: sidechain.location.clone(),
                });
            }
            if !destinations.insert((sidechain.processor.clone(), sidechain.input.clone())) {
                self.findings.push(RoutingFinding {
                    kind: RoutingFindingKind::DuplicateSidechainDestination {
                        processor: sidechain.processor.clone(),
                        input: sidechain.input.clone(),
                    },
                    location: sidechain.location.clone(),
                });
            }
            if self.channels.contains_key(&source) {
                self.routes.push(TopologyRoute {
                    id: format!("sidechain/{}", sidechain.id),
                    role: TopologyRouteRole::Sidechain,
                    source,
                    owner: owner.clone(),
                    destination: TopologyDestination::SidechainInput {
                        owner: owner.clone(),
                        processor_id: sidechain.processor.clone(),
                        processor_index: *processor_index,
                        input: sidechain.input.clone(),
                    },
                    tap: sidechain.tap,
                    declaration: RouteDeclaration::Sidechain(index),
                });
            }
        }
    }

    fn find_unconnected_sidechains(&mut self) {
        let connected = self
            .routes
            .iter()
            .filter_map(|route| match &route.destination {
                TopologyDestination::SidechainInput {
                    processor_id,
                    input,
                    ..
                } => Some((processor_id.clone(), input.clone())),
                TopologyDestination::Bus(_) => None,
            })
            .collect::<BTreeSet<_>>();
        for processor in &self.processors {
            for input in &processor.sidechain_inputs {
                if !connected.contains(&(processor.id.clone(), input.clone())) {
                    self.findings.push(RoutingFinding {
                        kind: RoutingFindingKind::UnconnectedSidechainInput {
                            processor: processor.id.clone(),
                            input: input.clone(),
                        },
                        location: processor.location.clone(),
                    });
                }
            }
        }
    }
}

fn authored_channels(
    project: &Project,
) -> Vec<(ChannelIdentity, ChannelDeclaration, SourceLocation)> {
    project
        .song
        .tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            (
                ChannelIdentity::Track(track.id.clone()),
                ChannelDeclaration::Track(index),
                track.location.clone(),
            )
        })
        .chain(
            project
                .song
                .groups
                .iter()
                .enumerate()
                .map(|(index, group)| {
                    (
                        ChannelIdentity::Group(group.id.clone()),
                        ChannelDeclaration::Group(index),
                        group.location.clone(),
                    )
                }),
        )
        .chain(
            project
                .song
                .returns
                .iter()
                .enumerate()
                .map(|(index, channel)| {
                    (
                        ChannelIdentity::Return(channel.id.clone()),
                        ChannelDeclaration::Return(index),
                        channel.location.clone(),
                    )
                }),
        )
        .collect()
}

fn channel_strip(project: &Project, declaration: ChannelDeclaration) -> Option<&ChannelStrip> {
    match declaration {
        ChannelDeclaration::Track(index) => {
            project.song.tracks.get(index).map(|track| &track.channel)
        }
        ChannelDeclaration::Group(index) => {
            project.song.groups.get(index).map(|group| &group.channel)
        }
        ChannelDeclaration::Return(index) => project
            .song
            .returns
            .get(index)
            .map(|channel| &channel.channel),
        ChannelDeclaration::Master => None,
    }
}

fn valid_id(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn stable_order<'a>(
    identities: impl Iterator<Item = &'a ChannelIdentity>,
    routes: &[TopologyRoute],
) -> (Vec<ChannelIdentity>, bool) {
    let mut indegree = BTreeMap::new();
    let mut outgoing: BTreeMap<ChannelIdentity, BTreeSet<ChannelIdentity>> = BTreeMap::new();
    for identity in identities {
        indegree.insert(identity.clone(), 0_usize);
        outgoing.insert(identity.clone(), BTreeSet::new());
    }
    for route in routes {
        if outgoing
            .entry(route.source.clone())
            .or_default()
            .insert(route.owner.clone())
        {
            *indegree.entry(route.owner.clone()).or_default() += 1;
        }
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(identity, degree)| (*degree == 0).then_some(identity.clone()))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(indegree.len());
    while let Some(identity) = ready.pop_first() {
        order.push(identity.clone());
        for destination in outgoing.get(&identity).into_iter().flatten() {
            let degree = indegree
                .get_mut(destination)
                .expect("canonical route destination");
            *degree -= 1;
            if *degree == 0 {
                ready.insert(destination.clone());
            }
        }
    }
    let cycle = order.len() != indegree.len();
    if cycle {
        // Keep the partial order inspectable; invalid topology is never lowered.
        let remaining = indegree
            .keys()
            .filter(|identity| !order.contains(identity))
            .cloned()
            .collect::<Vec<_>>();
        order.extend(remaining);
    }
    (order, cycle)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{
        dsl::parse_project,
        model::{ChannelRef, OutputTarget},
        validate::validate_project,
    };

    use super::*;

    fn production_project() -> Project {
        let parsed = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../tests/fixtures/production_mix.crump"),
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        parsed.project.expect("Project")
    }

    fn has_code(project: &Project, code: &str) -> bool {
        validate_project(project)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == code)
    }

    #[test]
    fn declaration_reordering_preserves_canonical_channels_routes_and_owners() {
        let first = production_project();
        let mut second = first.clone();
        second.song.tracks.reverse();
        second.song.groups.reverse();
        second.song.returns.reverse();
        second.song.sidechains.reverse();

        let first = AuthoredTopology::compile(&first);
        let second = AuthoredTopology::compile(&second);
        assert!(first.is_valid(), "{:?}", first.findings());
        assert!(second.is_valid(), "{:?}", second.findings());
        assert_eq!(first.order(), second.order());
        assert_eq!(
            first
                .routes()
                .iter()
                .map(|route| route.id())
                .collect::<Vec<_>>(),
            second
                .routes()
                .iter()
                .map(|route| route.id())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            first
                .processors()
                .iter()
                .map(|processor| (processor.id(), processor.owner().clone()))
                .collect::<BTreeSet<_>>(),
            second
                .processors()
                .iter()
                .map(|processor| (processor.id(), processor.owner().clone()))
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn validation_codes_are_adapted_from_canonical_routing_findings() {
        let project = production_project();

        let mut cycle = project.clone();
        cycle.song.returns[0].channel.output = OutputTarget::Group("mix_bus".to_owned());
        assert!(
            AuthoredTopology::compile(&cycle)
                .findings()
                .iter()
                .any(|finding| matches!(finding.kind, RoutingFindingKind::Cycle))
        );
        assert!(has_code(&cycle, "CRV113"));

        let mut missing = project.clone();
        missing.song.groups[0].channel.output = OutputTarget::Group("missing".to_owned());
        assert!(
            AuthoredTopology::compile(&missing)
                .findings()
                .iter()
                .any(|finding| {
                    matches!(finding.kind, RoutingFindingKind::UnresolvedMain { .. })
                })
        );
        assert!(has_code(&missing, "CRV103"));
        assert!(has_code(&missing, "CRV118"));

        let mut duplicate = project.clone();
        let mut route = duplicate.song.sidechains[0].clone();
        route.id = "duplicate_detector".to_owned();
        duplicate.song.sidechains.push(route);
        assert!(
            AuthoredTopology::compile(&duplicate)
                .findings()
                .iter()
                .any(|finding| {
                    matches!(
                        finding.kind,
                        RoutingFindingKind::DuplicateSidechainDestination { .. }
                    )
                })
        );
        assert!(has_code(&duplicate, "CRV112"));

        let mut sidechain = project;
        sidechain.song.sidechains[0].from = ChannelRef::Group("mix_bus".to_owned());
        assert!(
            AuthoredTopology::compile(&sidechain)
                .routes()
                .iter()
                .any(|route| route.role() == TopologyRouteRole::Sidechain)
        );
        assert!(has_code(&sidechain, "CRV113"));
    }
}
