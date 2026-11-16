//! Same-frame processing order of a Composable DSP Graph.
//!
//! A Node must run after every Node whose output it reads in the same frame. Two kinds of
//! dependency exist: a Bus, and in voice graphs a graph-port Modulation route, whose source
//! Node must run before every Node consuming the route's target Parameter. A dependency into a
//! feedback-breaking Node (`delay_line`) does not order anything, because that Node reads its
//! inputs after the frame; it may therefore close a Feedback Loop. Graph Validation, the graph
//! runtime and the Graph View all read the order from here.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::model::{Graph, GraphDomain, Instrument, InstrumentModulationSourceKind};

/// Same-frame dependencies between the Nodes of one graph, keyed by Node ID.
#[derive(Debug)]
pub(crate) struct SameFrameDependencies<'a> {
    /// Graph declaration order, so callers can map IDs back to indexes.
    nodes: Vec<&'a str>,
    edges: BTreeMap<&'a str, Vec<&'a str>>,
}

impl<'a> SameFrameDependencies<'a> {
    /// Dependencies created by Buses alone.
    pub(crate) fn buses(graph: &'a Graph) -> Self {
        let mut dependencies = Self {
            nodes: graph.nodes.iter().map(|node| node.id.as_str()).collect(),
            edges: graph
                .nodes
                .iter()
                .map(|node| (node.id.as_str(), Vec::new()))
                .collect(),
        };
        for connection in &graph.connections {
            dependencies.add(graph, &connection.from.node, &connection.to.node);
        }
        dependencies
    }

    /// Dependencies created by Buses and, in a voice graph, by graph-port Modulation routes.
    pub(crate) fn of(instrument: &'a Instrument, graph: &'a Graph) -> Self {
        let mut dependencies = Self::buses(graph);
        // Shared graphs omit every per-voice route and therefore have no route dependencies.
        if graph.domain != GraphDomain::Voice {
            return dependencies;
        }
        for route in &instrument.modulation_routes {
            let source_nodes = std::iter::once(route.source.as_str())
                .chain(route.auxiliary.as_deref())
                .filter_map(|source_id| {
                    instrument
                        .modulation_sources
                        .iter()
                        .find(|source| source.id == source_id)
                })
                .filter_map(|source| match &source.kind {
                    InstrumentModulationSourceKind::GraphPort { node, .. } => Some(node.as_str()),
                    _ => None,
                });
            for source_node in source_nodes {
                for consumer in graph
                    .nodes
                    .iter()
                    .filter(|node| node.references_parameter(&route.target))
                {
                    dependencies.add(graph, source_node, &consumer.id);
                }
            }
        }
        dependencies
    }

    fn add(&mut self, graph: &Graph, from: &str, to: &'a str) {
        let breaks_feedback = graph
            .nodes
            .iter()
            .find(|node| node.id == to)
            .is_some_and(|node| node.kind.breaks_feedback());
        if breaks_feedback || !self.edges.contains_key(to) {
            return;
        }
        // `note` and `voice_sum` are host sources rather than Nodes and order nothing.
        if let Some(destinations) = self.edges.get_mut(from) {
            destinations.push(to);
        }
    }

    /// Whether `to` runs after `from` through at least one dependency.
    pub(crate) fn reaches(&self, from: &str, to: &str) -> bool {
        let mut pending = self.edges.get(from).cloned().unwrap_or_default();
        let mut seen = BTreeSet::new();
        while let Some(node) = pending.pop() {
            if node == to {
                return true;
            }
            if seen.insert(node)
                && let Some(next) = self.edges.get(node)
            {
                pending.extend(next.iter().copied());
            }
        }
        false
    }

    /// Node indexes in processing order, breaking ties by Node ID.
    ///
    /// # Errors
    ///
    /// Returns one cycle that no `delay_line` breaks, starting and ending at its smallest
    /// Node ID.
    pub(crate) fn order(&self) -> Result<Vec<usize>, Vec<&'a str>> {
        let mut incoming: BTreeMap<&str, usize> =
            self.edges.keys().map(|node| (*node, 0)).collect();
        for destinations in self.edges.values() {
            for destination in destinations {
                if let Some(count) = incoming.get_mut(destination) {
                    *count += 1;
                }
            }
        }
        let mut ready: BTreeSet<_> = incoming
            .iter()
            .filter_map(|(node, count)| (*count == 0).then_some(*node))
            .collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(node) = ready.pop_first() {
            if let Some(index) = self.nodes.iter().position(|id| *id == node) {
                order.push(index);
            }
            for destination in &self.edges[node] {
                if let Some(count) = incoming.get_mut(destination) {
                    *count -= 1;
                    if *count == 0 {
                        ready.insert(destination);
                    }
                }
            }
        }
        if order.len() == self.nodes.len() {
            Ok(order)
        } else {
            Err(self.cycle().unwrap_or_default())
        }
    }

    fn cycle(&self) -> Option<Vec<&'a str>> {
        let mut indegree: BTreeMap<&'a str, usize> =
            self.edges.keys().map(|node| (*node, 0)).collect();
        let mut predecessors: BTreeMap<&'a str, Vec<&'a str>> =
            self.edges.keys().map(|node| (*node, Vec::new())).collect();
        for (source, destinations) in &self.edges {
            for destination in destinations {
                if let Some(count) = indegree.get_mut(destination) {
                    *count += 1;
                    predecessors.get_mut(destination)?.push(*source);
                }
            }
        }
        let mut ready: VecDeque<_> = indegree
            .iter()
            .filter_map(|(node, count)| (*count == 0).then_some(*node))
            .collect();
        while let Some(node) = ready.pop_front() {
            indegree.remove(node);
            if let Some(destinations) = self.edges.get(node) {
                for destination in destinations {
                    if let Some(count) = indegree.get_mut(destination) {
                        *count -= 1;
                        if *count == 0 {
                            ready.push_back(destination);
                        }
                    }
                }
            }
        }
        // Every remaining Node has a remaining predecessor, including downstream sinks.
        // Walking backwards must therefore revisit a Node on a cycle.
        let start = *indegree.keys().next()?;
        let mut path = vec![start];
        let mut node = start;
        loop {
            let next = predecessors
                .get(node)?
                .iter()
                .copied()
                .filter(|next| indegree.contains_key(next))
                .min()?;
            if let Some(position) = path.iter().position(|seen| *seen == next) {
                let mut cycle = path.split_off(position);
                cycle.reverse();
                let smallest = cycle
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, id)| **id)
                    .map_or(0, |(index, _)| index);
                cycle.rotate_left(smallest);
                cycle.push(cycle[0]);
                return Some(cycle);
            }
            path.push(next);
            node = next;
        }
    }
}
