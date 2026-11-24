//! Synth module of the History time machine: one instrument as it was in one version.
//!
//! Draws the instrument like the Sound page: parameters grouped into synth modules (Oscillator /
//! Source, Filter, Envelope & Mod) as dials alongside read-only module scopes with previous curves
//! ghosted, a visual Modulation Matrix with bipolar depth bars, post-processors via `fx_rack`, and
//! compact visual panels for voicing, sampler zones, faceplate macros and graph node settings.

use std::collections::HashSet;
use std::f32::consts::PI;

use super::*;
use crate::model::{
    ControlPolarity, FaceplateControl, GraphNode, Instrument, InstrumentKind,
    InstrumentModulationRate, InstrumentModulationRoute, InstrumentModulationScope,
    InstrumentModulationSource, InstrumentModulationSourceKind, LfoWaveform, ModulationCurve,
    NodeKind, Parameter, PresentationItemKind, PresentationSectionKind, SampleDirection,
    SampleMapping, SamplePlayback, SamplerDefinition, SettingValue, Song, Unit,
};

fn old_inst<'a>(song: Option<&'a Song>, id: &str) -> Option<&'a Instrument> {
    song.and_then(|s| s.instruments.iter().find(|i| i.id == id))
}

/// Which Sound-page module a synth parameter belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SynthModuleKind {
    Oscillator,
    Filter,
    Envelope,
    Other,
}

impl SynthModuleKind {
    fn title(self, kind: InstrumentKind) -> &'static str {
        match self {
            Self::Oscillator => match kind {
                InstrumentKind::StockDrums => "PERCUSSION ENGINE",
                InstrumentKind::Sampler => "SAMPLER SOURCE",
                InstrumentKind::Graph => "OSCILLATOR / SOURCE",
            },
            Self::Filter => "FILTER RESPONSE",
            Self::Envelope => "ENVELOPE & MOD",
            Self::Other => "CONTROLS",
        }
    }

    fn accent(self) -> Color32 {
        match self {
            Self::Oscillator => theme::ACCENT_AMBER,
            Self::Filter => theme::ACCENT_CYAN,
            Self::Envelope => theme::ACCENT_VIOLET,
            Self::Other => Kind::Instrument.color(),
        }
    }
}

/// One grouped synth module card to draw.
struct SynthModule<'a> {
    kind: SynthModuleKind,
    params: Vec<&'a Parameter>,
}

/// Categorizes one instrument parameter into a Sound-page module using authored presentation
/// sections first, then graph node references, then keyword conventions matching `sound_design.rs`.
fn classify_parameter(inst: &Instrument, param: &Parameter) -> SynthModuleKind {
    for section in &inst.presentation.sections {
        if section
            .items
            .iter()
            .any(|item| item.kind == PresentationItemKind::Parameter && item.id == param.key)
        {
            return match section.kind {
                PresentationSectionKind::Sources | PresentationSectionKind::SourceMixer => {
                    SynthModuleKind::Oscillator
                }
                PresentationSectionKind::Filters => SynthModuleKind::Filter,
                PresentationSectionKind::EnvelopesLfos | PresentationSectionKind::Modulation => {
                    SynthModuleKind::Envelope
                }
                PresentationSectionKind::Voicing
                | PresentationSectionKind::Routing
                | PresentationSectionKind::Effects => SynthModuleKind::Other,
            };
        }
    }

    for graph in [inst.graph.as_ref(), inst.shared_graph.as_ref()]
        .into_iter()
        .flatten()
    {
        for node in &graph.nodes {
            if !node.references_parameter(&param.key) {
                continue;
            }
            if is_filter_node(node) {
                return SynthModuleKind::Filter;
            }
            if is_envelope_node(node) {
                return SynthModuleKind::Envelope;
            }
            if is_source_node(node) {
                return SynthModuleKind::Oscillator;
            }
        }
    }

    let k = param.key.to_lowercase();
    let n = param.name.to_lowercase();
    let has_any = |words: &[&str]| words.iter().any(|w| k.contains(w) || n.contains(w));
    if has_any(&["cut", "freq", "filter", "res", "q", "bright", "damp"]) {
        SynthModuleKind::Filter
    } else if has_any(&[
        "attack", "decay", "sustain", "release", "rate", "depth", "lfo", "env",
    ]) {
        SynthModuleKind::Envelope
    } else {
        SynthModuleKind::Oscillator
    }
}

fn is_filter_node(node: &GraphNode) -> bool {
    let label = theme::node_kind_label(node.kind).to_lowercase();
    let id = node.id.to_lowercase();
    label.contains("filter")
        || label.contains("eq")
        || id.contains("filter")
        || id.contains("lpf")
        || id.contains("hpf")
}

fn is_envelope_node(node: &GraphNode) -> bool {
    matches!(node.kind, NodeKind::Envelope)
        || theme::node_kind_label(node.kind)
            .to_lowercase()
            .contains("env")
        || node.id.to_lowercase().contains("env")
}

fn is_source_node(node: &GraphNode) -> bool {
    matches!(
        node.kind,
        NodeKind::Oscillator
            | NodeKind::AnalogOscillator
            | NodeKind::Unison
            | NodeKind::Wavetable
            | NodeKind::Noise
            | NodeKind::SampleSource
            | NodeKind::Multisample
            | NodeKind::Granular
            | NodeKind::Spectral
            | NodeKind::Exciter
            | NodeKind::ModalBank
    )
}

fn all_graph_nodes(inst: &Instrument) -> impl Iterator<Item = &GraphNode> {
    inst.graph
        .iter()
        .chain(inst.shared_graph.iter())
        .flat_map(|g| g.nodes.iter())
}

fn has_filter_node(inst: &Instrument) -> bool {
    all_graph_nodes(inst).any(is_filter_node)
}

fn has_envelope_node(inst: &Instrument) -> bool {
    all_graph_nodes(inst).any(is_envelope_node)
}

/// Groups every parameter of `inst` into Sound-page modules without dropping or duplicating any.
fn build_synth_modules(inst: &Instrument) -> Vec<SynthModule<'_>> {
    let mut osc = Vec::new();
    let mut flt = Vec::new();
    let mut env = Vec::new();
    let mut other = Vec::new();
    for param in &inst.parameters {
        match classify_parameter(inst, param) {
            SynthModuleKind::Oscillator => osc.push(param),
            SynthModuleKind::Filter => flt.push(param),
            SynthModuleKind::Envelope => env.push(param),
            SynthModuleKind::Other => other.push(param),
        }
    }

    let mut modules = Vec::new();
    let has_sources = all_graph_nodes(inst).any(is_source_node);
    if !osc.is_empty()
        || has_sources
        || matches!(
            inst.kind,
            InstrumentKind::StockDrums | InstrumentKind::Sampler
        )
    {
        modules.push(SynthModule {
            kind: SynthModuleKind::Oscillator,
            params: osc,
        });
    }
    if !flt.is_empty() || has_filter_node(inst) {
        modules.push(SynthModule {
            kind: SynthModuleKind::Filter,
            params: flt,
        });
    }
    if !env.is_empty() || has_envelope_node(inst) || !inst.modulation_sources.is_empty() {
        modules.push(SynthModule {
            kind: SynthModuleKind::Envelope,
            params: env,
        });
    }
    if !other.is_empty() {
        modules.push(SynthModule {
            kind: SynthModuleKind::Other,
            params: other,
        });
    }
    modules
}

/// Draws instrument `id` as it is in `view.shown`, marking what this edit moved (against
/// `view.previous`) and what differs from the other side of Then / Now (`view.other`).
/// `changes` are the measured changes inside this instrument.
pub(super) fn draw(ui: &mut Ui, view: &SoundView<'_>, id: &str, changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);
    let prev_inst = old_inst(prev_song, id);
    let other_inst = old_inst(other_song, id);

    let Some(inst) = song.instruments.iter().find(|i| i.id == id) else {
        let name = prev_inst.map_or(id, |i| i.name.as_str());
        section(ui, Kind::Instrument, name, |ui| {
            setting_chip(ui, "Instrument removed", true, false);
            change_chips(ui, changes);
        });
        return;
    };

    let name = match song.tracks.iter().find(|t| t.instrument == inst.id) {
        Some(track) => format!("{} on {}", inst.name, track.name),
        None => inst.name.clone(),
    };

    section(ui, Kind::Instrument, &name, |ui| {
        let mut covered_roots: HashSet<&str> = HashSet::new();

        // Surface edited voicing/polyphony right at the top when synth parameters weren't touched,
        // so the changed control is immediately visible without scrolling past unchanged modules.
        let voicing_touched = has_root(changes, "voicing") || has_root(changes, "polyphony");
        let params_touched = has_root(changes, "parameters");
        if voicing_touched && !params_touched {
            draw_voicing_card(ui, inst, prev_inst, other_inst, view.other_word);
            ui.add_space(10.0);
            covered_roots.insert("voicing");
            covered_roots.insert("polyphony");
        }

        // 1. Synth module cards (Oscillator/Source, Filter, Envelope & Mod, Controls).
        let modules = build_synth_modules(inst);
        if !modules.is_empty() {
            draw_module_grid(ui, view, inst, prev_inst, other_inst, &modules);
            covered_roots.insert("parameters");
        }

        // 2. Modulation matrix (routes + edited sources).
        let mod_routes_touched = has_root(changes, "modulation_routes");
        let mod_sources_touched = has_root(changes, "modulation_sources");
        let has_routes = !inst.modulation_routes.is_empty()
            || prev_inst.is_some_and(|p| !p.modulation_routes.is_empty());
        if has_routes || mod_routes_touched || mod_sources_touched {
            ui.add_space(10.0);
            draw_modulation_matrix(
                ui,
                inst,
                prev_inst,
                other_inst,
                view.other_word,
                mod_sources_touched,
            );
            covered_roots.insert("modulation_routes");
            if mod_sources_touched {
                covered_roots.insert("modulation_sources");
            }
        }

        // 3. Voicing & polyphony panel (when not already surfaced at the top).
        if voicing_touched && params_touched {
            ui.add_space(10.0);
            draw_voicing_card(ui, inst, prev_inst, other_inst, view.other_word);
            covered_roots.insert("voicing");
            covered_roots.insert("polyphony");
        }

        // 4. Sampler keymap (shown for sampler instruments or when sampler mappings changed).
        if inst.sampler.is_some()
            || prev_inst.is_some_and(|p| p.sampler.is_some())
            || has_root(changes, "sampler")
        {
            ui.add_space(10.0);
            draw_sampler_card(
                ui,
                inst.sampler.as_ref(),
                prev_inst.and_then(|p| p.sampler.as_ref()),
                other_inst.and_then(|o| o.sampler.as_ref()),
                view.other_word,
            );
            covered_roots.insert("sampler");
        }

        // 5. Graph / shared_graph node settings when a node setting was edited directly.
        let graph_changes: Vec<&ValueChange> = changes
            .iter()
            .filter(|c| {
                c.path
                    .first()
                    .is_some_and(|p| p == "graph" || p == "shared_graph")
            })
            .copied()
            .collect();
        let covered_graph = draw_edited_graph_nodes(
            ui,
            inst,
            prev_inst,
            other_inst,
            &graph_changes,
            view.other_word,
        );

        // 6. Faceplate controls when faceplate changed.
        if has_root(changes, "faceplate") {
            ui.add_space(10.0);
            draw_faceplate_card(
                ui,
                &inst.faceplate,
                prev_inst.map(|p| p.faceplate.as_slice()),
                other_inst.map(|o| o.faceplate.as_slice()),
                view.other_word,
            );
            covered_roots.insert("faceplate");
        }

        // 7. Instrument post-processors through the shared FX rack module.
        let fx: Vec<&ValueChange> = changes
            .iter()
            .filter(|c| c.path.first().is_some_and(|p| p == "post_processors"))
            .copied()
            .collect();
        let has_post = !inst.post_processors.is_empty()
            || prev_inst.is_some_and(|p| !p.post_processors.is_empty())
            || !fx.is_empty();
        if has_post {
            ui.add_space(8.0);
            fx_rack::chain(
                ui,
                view,
                &inst.post_processors,
                prev_inst.map(|i| i.post_processors.as_slice()),
                other_inst.map(|i| i.post_processors.as_slice()),
                &fx,
            );
            covered_roots.insert("post_processors");
        }

        // 8. Anything genuinely without a visual still falls back to chips so no change is lost.
        let remaining: Vec<&ValueChange> = changes
            .iter()
            .filter(|c| {
                let Some(root) = c.path.first().map(String::as_str) else {
                    return true;
                };
                if covered_roots.contains(root) {
                    return false;
                }
                if (root == "graph" || root == "shared_graph") && covered_graph {
                    return !is_node_setting_path(&c.path);
                }
                true
            })
            .copied()
            .collect();
        change_chips(ui, &remaining);
    });
}

fn has_root(changes: &[&ValueChange], root: &str) -> bool {
    changes
        .iter()
        .any(|c| c.path.first().is_some_and(|p| p == root))
}

fn is_node_setting_path(path: &[String]) -> bool {
    path.len() >= 4
        && (path[0] == "graph" || path[0] == "shared_graph")
        && path[1] == "nodes"
        && path.get(3).is_some_and(|p| p == "settings")
}

/// Lays out synth module cards responsively: up to 3 across on wide windows, 2 across on medium,
/// stacked on narrow, so scopes and dials never clip past the right edge.
fn draw_module_grid(
    ui: &mut Ui,
    view: &SoundView<'_>,
    inst: &Instrument,
    prev_inst: Option<&Instrument>,
    other_inst: Option<&Instrument>,
    modules: &[SynthModule<'_>],
) {
    let avail = ui.available_width();
    let cols = if avail >= 740.0 {
        modules.len().min(3)
    } else if avail >= 480.0 {
        modules.len().min(2)
    } else {
        1
    };

    for row in modules.chunks(cols.max(1)) {
        if row.len() == 1 {
            draw_module_card(ui, view, inst, prev_inst, other_inst, &row[0]);
        } else {
            ui.columns(row.len(), |columns| {
                for (idx, module) in row.iter().enumerate() {
                    draw_module_card(&mut columns[idx], view, inst, prev_inst, other_inst, module);
                }
            });
        }
        ui.add_space(8.0);
    }
}

fn param_value(inst: Option<&Instrument>, key: &str) -> Option<f64> {
    inst.and_then(|i| i.parameters.iter().find(|p| p.key == key))
        .map(|p| p.value)
}

fn module_edit_state(
    module: &SynthModule<'_>,
    inst: &Instrument,
    prev_inst: Option<&Instrument>,
    other_inst: Option<&Instrument>,
) -> (bool, bool) {
    let mut edited = false;
    let mut differs = false;
    for param in &module.params {
        match mark(
            param.value,
            param_value(prev_inst, &param.key),
            param_value(other_inst, &param.key),
        ) {
            Some(Mark::Edited(_)) => edited = true,
            Some(Mark::Differs(_)) => differs = true,
            None => {}
        }
    }
    match module.kind {
        SynthModuleKind::Filter => {
            let curr = read_filter_state(inst);
            if let Some(prev) = prev_inst.map(read_filter_state)
                && curr.differs_from(&prev)
            {
                edited = true;
            } else if let Some(other) = other_inst.map(read_filter_state)
                && curr.differs_from(&other)
            {
                differs = true;
            }
        }
        SynthModuleKind::Envelope => {
            let curr = read_adsr_state(inst);
            if let Some(prev) = prev_inst.map(read_adsr_state)
                && curr.differs_from(&prev)
            {
                edited = true;
            } else if let Some(other) = other_inst.map(read_adsr_state)
                && curr.differs_from(&other)
            {
                differs = true;
            }
        }
        SynthModuleKind::Oscillator => {
            let curr_wave = primary_waveform(inst);
            if prev_inst.is_some_and(|p| primary_waveform(p) != curr_wave) {
                edited = true;
            } else if other_inst.is_some_and(|o| primary_waveform(o) != curr_wave) {
                differs = true;
            }
        }
        SynthModuleKind::Other => {}
    }
    (edited, differs)
}

fn draw_module_card(
    ui: &mut Ui,
    view: &SoundView<'_>,
    inst: &Instrument,
    prev_inst: Option<&Instrument>,
    other_inst: Option<&Instrument>,
    module: &SynthModule<'_>,
) {
    let accent = module.kind.accent();
    let (edited, differs) = module_edit_state(module, inst, prev_inst, other_inst);
    let border = if edited {
        theme::SELECTION
    } else if differs {
        theme::BORDER_STRONG
    } else {
        theme::BORDER_SUBTLE
    };

    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(if edited { 1.5_f32 } else { 1.0_f32 }, border))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                widgets::section_header(ui, module.kind.title(inst.kind), accent);
                ui.add_space(6.0);

                let paint_scope_and_summary = |ui: &mut Ui, scope_w: f32| match module.kind {
                    SynthModuleKind::Oscillator => {
                        paint_osc_or_source_scope(ui, inst, prev_inst, other_inst, scope_w, 72.0);
                        ui.add_space(5.0);
                        ui.label(
                            RichText::new(summarize_source(inst))
                                .size(10.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                    SynthModuleKind::Filter => {
                        let curr = read_filter_state(inst);
                        let prev = prev_inst.map(read_filter_state);
                        let other = other_inst.map(read_filter_state);
                        paint_filter_bode_scope(
                            ui,
                            inst,
                            &curr,
                            prev.as_ref(),
                            other.as_ref(),
                            scope_w,
                            72.0,
                        );
                        ui.add_space(5.0);
                        let mut summary = format!(
                            "{} · Cut {} · Res {:.0}%",
                            curr.mode_tag(),
                            theme::format_compact_value(curr.cutoff_hz, Unit::Hertz),
                            curr.resonance * 100.0
                        );
                        if let Some(p) = &prev
                            && (p.cutoff_hz - curr.cutoff_hz).abs() > 1e-3
                        {
                            summary.push_str(&format!(
                                " (prev {})",
                                theme::format_compact_value(p.cutoff_hz, Unit::Hertz)
                            ));
                        }
                        ui.label(
                            RichText::new(summary)
                                .size(10.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                    SynthModuleKind::Envelope => {
                        let curr = read_adsr_state(inst);
                        let prev = prev_inst.map(read_adsr_state);
                        let other = other_inst.map(read_adsr_state);
                        paint_adsr_scope(
                            ui,
                            inst,
                            &curr,
                            prev.as_ref(),
                            other.as_ref(),
                            scope_w,
                            72.0,
                        );
                        ui.add_space(5.0);
                        let env_summary = format!(
                            "A {} · D {} · S {:.0}% · R {}",
                            format_ms(curr.attack_s),
                            format_ms(curr.decay_s),
                            curr.sustain * 100.0,
                            format_ms(curr.release_s)
                        );
                        ui.label(
                            RichText::new(env_summary)
                                .monospace()
                                .size(9.5)
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                    SynthModuleKind::Other => {}
                };

                let paint_controls = |ui: &mut Ui| {
                    if !module.params.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for param in &module.params {
                                dial(
                                    ui,
                                    &param.name,
                                    param.value,
                                    param.minimum,
                                    param.maximum,
                                    param.unit,
                                    &|v| theme::format_compact_value(v, param.unit),
                                    mark(
                                        param.value,
                                        param_value(prev_inst, &param.key),
                                        param_value(other_inst, &param.key),
                                    ),
                                    view.other_word,
                                    accent,
                                );
                            }
                        });
                    } else {
                        draw_module_context_pills(ui, inst, module.kind);
                    }
                };

                let avail_w = ui.available_width();
                if module.kind != SynthModuleKind::Other && avail_w >= 520.0 {
                    let scope_w = (avail_w * 0.42).clamp(220.0, 320.0);
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(scope_w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                paint_scope_and_summary(ui, scope_w);
                            },
                        );
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            paint_controls(ui);
                        });
                    });
                } else {
                    let scope_w = avail_w.max(120.0);
                    paint_scope_and_summary(ui, scope_w);
                    ui.add_space(6.0);
                    paint_controls(ui);
                }
            });
        });
}

/// Shows compact hardware context badges when a synth module has a scope/nodes but no macro dials.
fn draw_module_context_pills(ui: &mut Ui, inst: &Instrument, kind: SynthModuleKind) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(5.0, 4.0);
        match kind {
            SynthModuleKind::Oscillator => {
                let mut shown = 0;
                for node in all_graph_nodes(inst).filter(|n| is_source_node(n)) {
                    let wave = node_setting_text(node, &["waveform", "wave", "shape"])
                        .unwrap_or_else(|| theme::node_kind_label(node.kind).to_owned());
                    let coarse = node_setting_text(node, &["coarse"])
                        .filter(|c| c != "0st" && c != "0")
                        .map(|c| format!(" {c}"))
                        .unwrap_or_default();
                    setting_chip(ui, &format!("{}: {wave}{coarse}", node.id), false, false);
                    shown += 1;
                    if shown >= 3 {
                        break;
                    }
                }
                if !inst.voicing.is_legacy_default() {
                    let mode = format!("{:?}", inst.voicing.mode).to_lowercase();
                    setting_chip(
                        ui,
                        &format!("{mode} · glide {}", format_ms(inst.voicing.glide_seconds)),
                        false,
                        false,
                    );
                }
            }
            SynthModuleKind::Envelope => {
                let active_routes = inst.modulation_routes.iter().filter(|r| !r.bypass).count();
                setting_chip(
                    ui,
                    &format!("{} sources", inst.modulation_sources.len()),
                    false,
                    false,
                );
                setting_chip(ui, &format!("{active_routes} active routes"), false, false);
                setting_chip(ui, &format!("{} voices", inst.polyphony), false, false);
            }
            SynthModuleKind::Filter => {
                if let Some(node) = all_graph_nodes(inst).find(|n| is_filter_node(n)) {
                    let model = node_setting_text(node, &["model"])
                        .unwrap_or_else(|| theme::node_kind_label(node.kind).to_owned());
                    setting_chip(ui, &model.replace('_', " "), false, false);
                }
            }
            SynthModuleKind::Other => {}
        }
    });
}

// ── Scope Data Extractors ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

#[derive(Clone, Debug)]
struct FilterState {
    mode: FilterMode,
    cutoff_hz: f64,
    resonance: f64,
}

impl FilterState {
    fn mode_tag(&self) -> &'static str {
        match self.mode {
            FilterMode::Lowpass => "LPF",
            FilterMode::Highpass => "HPF",
            FilterMode::Bandpass => "BPF",
            FilterMode::Notch => "NOTCH",
        }
    }

    fn differs_from(&self, other: &Self) -> bool {
        self.mode != other.mode
            || (self.cutoff_hz - other.cutoff_hz).abs() > 1e-3
            || (self.resonance - other.resonance).abs() > 1e-4
    }
}

/// Evaluates normalized filter response magnitude (in dB, clamped to -28..15 dB) at frequency `f`.
fn filter_response_db(state: &FilterState, freq_hz: f64) -> f64 {
    let q = 0.707 + state.resonance.clamp(0.0, 1.0) * 5.2;
    let r = (freq_hz / state.cutoff_hz.max(20.0)).max(1e-6);
    let r2 = r * r;
    let denom = ((1.0 - r2).powi(2) + (r / q).powi(2)).sqrt().max(1e-4);
    let mag = match state.mode {
        FilterMode::Lowpass => 1.0 / denom,
        FilterMode::Highpass => r2 / denom,
        FilterMode::Bandpass => (r / q) / denom,
        FilterMode::Notch => (1.0 - r2).abs() / denom,
    };
    (20.0 * mag.max(1e-4).log10()).clamp(-28.0, 15.0)
}

fn read_filter_state(inst: &Instrument) -> FilterState {
    let node = all_graph_nodes(inst).find(|n| is_filter_node(n));
    let mode = node
        .and_then(|n| node_setting_text(n, &["mode", "type"]))
        .map(|m| {
            let m = m.to_lowercase();
            if m.contains("high") || m.contains("hp") {
                FilterMode::Highpass
            } else if m.contains("band") || m.contains("bp") {
                FilterMode::Bandpass
            } else if m.contains("notch") {
                FilterMode::Notch
            } else {
                FilterMode::Lowpass
            }
        })
        .unwrap_or(FilterMode::Lowpass);

    let cutoff_from_node =
        node.and_then(|n| resolve_node_number(inst, n, &["cutoff", "frequency", "freq"]));
    let cutoff_from_param = inst
        .parameters
        .iter()
        .find(|p| {
            p.unit == Unit::Hertz
                && (p.key.contains("cut") || p.key.contains("freq") || p.key.contains("filter"))
        })
        .or_else(|| inst.parameters.iter().find(|p| p.unit == Unit::Hertz))
        .map(|p| p.value);
    let cutoff_hz = cutoff_from_node
        .or(cutoff_from_param)
        .unwrap_or(2400.0)
        .clamp(20.0, 20_000.0);

    let res_from_node = node.and_then(|n| resolve_node_number(inst, n, &["resonance", "res", "q"]));
    let res_from_param = inst
        .parameters
        .iter()
        .find(|p| p.key.contains("res") || p.key == "q")
        .map(|p| ((p.value - p.minimum) / (p.maximum - p.minimum).max(1e-6)).clamp(0.0, 1.0));
    let resonance = res_from_node
        .map(|v| {
            if v > 1.0 {
                ((v - 0.5) / 8.0).clamp(0.0, 1.0)
            } else {
                v.clamp(0.0, 1.0)
            }
        })
        .or(res_from_param)
        .unwrap_or(0.22);

    FilterState {
        mode,
        cutoff_hz,
        resonance,
    }
}

#[derive(Clone, Copy, Debug)]
struct AdsrState {
    attack_s: f64,
    decay_s: f64,
    sustain: f64,
    release_s: f64,
}

impl AdsrState {
    fn differs_from(&self, other: &Self) -> bool {
        (self.attack_s - other.attack_s).abs() > 1e-5
            || (self.decay_s - other.decay_s).abs() > 1e-5
            || (self.sustain - other.sustain).abs() > 1e-4
            || (self.release_s - other.release_s).abs() > 1e-5
    }
}

fn read_adsr_state(inst: &Instrument) -> AdsrState {
    let env_node = all_graph_nodes(inst).find(|n| is_envelope_node(n));
    let time_of = |key: &str, default_s: f64| -> f64 {
        env_node
            .and_then(|n| resolve_node_seconds(inst, n, key))
            .or_else(|| {
                inst.parameters
                    .iter()
                    .find(|p| p.key.contains(key))
                    .map(|p| match p.unit {
                        Unit::Seconds => p.value,
                        _ => {
                            0.005
                                + ((p.value - p.minimum) / (p.maximum - p.minimum).max(1e-6)) * 1.5
                        }
                    })
            })
            .unwrap_or(default_s)
            .clamp(0.001, 4.0)
    };
    let sustain = env_node
        .and_then(|n| resolve_node_number(inst, n, &["sustain"]))
        .or_else(|| {
            inst.parameters
                .iter()
                .find(|p| p.key.contains("sustain"))
                .map(|p| {
                    ((p.value - p.minimum) / (p.maximum - p.minimum).max(1e-6)).clamp(0.0, 1.0)
                })
        })
        .unwrap_or(0.58)
        .clamp(0.0, 1.0);

    AdsrState {
        attack_s: time_of("attack", 0.015),
        decay_s: time_of("decay", 0.22),
        sustain,
        release_s: time_of("release", 0.28),
    }
}

fn node_setting_text(node: &GraphNode, keys: &[&str]) -> Option<String> {
    node.settings.iter().find_map(|s| {
        if !keys.contains(&s.key.as_str()) {
            return None;
        }
        Some(match &s.value {
            SettingValue::Text(t) => t.clone(),
            SettingValue::Number(n) => format!("{n}"),
            SettingValue::Parameter(p) => format!("${p}"),
        })
    })
}

fn resolve_node_number(inst: &Instrument, node: &GraphNode, keys: &[&str]) -> Option<f64> {
    let setting = node
        .settings
        .iter()
        .find(|s| keys.contains(&s.key.as_str()))?;
    match &setting.value {
        SettingValue::Number(n) => Some(*n),
        SettingValue::Parameter(key) => inst
            .parameters
            .iter()
            .find(|p| p.key == *key)
            .map(|p| p.value),
        SettingValue::Text(t) => parse_numeric_literal(t),
    }
}

fn resolve_node_seconds(inst: &Instrument, node: &GraphNode, key: &str) -> Option<f64> {
    let setting = node.settings.iter().find(|s| s.key == key)?;
    match &setting.value {
        SettingValue::Number(n) => Some(*n),
        SettingValue::Parameter(p_key) => inst
            .parameters
            .iter()
            .find(|p| p.key == *p_key)
            .map(|p| p.value),
        SettingValue::Text(t) => crate::time::parse_seconds(t)
            .ok()
            .or_else(|| t.parse().ok()),
    }
}

fn parse_numeric_literal(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if let Ok(v) = trimmed.parse::<f64>() {
        return Some(v);
    }
    if let Ok(s) = crate::time::parse_seconds(trimmed) {
        return Some(s);
    }
    for (suffix, scale) in [
        ("khz", 1000.0),
        ("hz", 1.0),
        ("db", 1.0),
        ("st", 1.0),
        ("%", 0.01),
    ] {
        if let Some(num) = trimmed.strip_suffix(suffix)
            && let Ok(v) = num.parse::<f64>()
        {
            return Some(v * scale);
        }
    }
    None
}

fn format_ms(seconds: f64) -> String {
    if seconds >= 1.0 {
        format!("{seconds:.2}s")
    } else {
        format!("{:.0}ms", seconds * 1000.0)
    }
}

fn primary_waveform(inst: &Instrument) -> String {
    all_graph_nodes(inst)
        .find_map(|n| {
            n.settings
                .iter()
                .find(|s| s.key == "waveform" || s.key == "wave" || s.key == "shape")
                .and_then(|s| match &s.value {
                    SettingValue::Text(t) => Some(t.to_lowercase()),
                    _ => None,
                })
        })
        .unwrap_or_else(|| "saw".to_owned())
}

fn summarize_source(inst: &Instrument) -> String {
    match inst.kind {
        InstrumentKind::StockDrums => format!("Stock Drums · {} voices", inst.polyphony),
        InstrumentKind::Sampler => {
            let count = inst.sampler.as_ref().map_or(0, |s| s.mappings.len());
            format!("Sampler · {count} zones · {} voices", inst.polyphony)
        }
        InstrumentKind::Graph => {
            let sources: Vec<String> = all_graph_nodes(inst)
                .filter(|n| is_source_node(n))
                .take(2)
                .map(|n| {
                    let desc = node_setting_text(n, &["waveform", "wave", "shape"])
                        .unwrap_or_else(|| theme::node_kind_label(n.kind).to_owned());
                    format!("{}: {desc}", n.id)
                })
                .collect();
            if sources.is_empty() {
                format!("Graph · {} voices", inst.polyphony)
            } else {
                sources.join(" · ")
            }
        }
    }
}

// ── Module Scope Painters (Read-Only with Ghosting) ──────────────────────────

fn paint_osc_or_source_scope(
    ui: &mut Ui,
    inst: &Instrument,
    prev_inst: Option<&Instrument>,
    other_inst: Option<&Instrument>,
    width: f32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let inner = rect.shrink(6.0);

    // Specialised visual for Stock Drums (percussion transient contour modulated by tone/punch).
    if inst.kind == InstrumentKind::StockDrums {
        let tone = param_norm(inst, "tone").unwrap_or(0.5);
        let punch = param_norm(inst, "punch").unwrap_or(0.5);
        let prev_tp = prev_inst.map(|p| {
            (
                param_norm(p, "tone").unwrap_or(0.5),
                param_norm(p, "punch").unwrap_or(0.5),
            )
        });
        let other_tp = other_inst.map(|o| {
            (
                param_norm(o, "tone").unwrap_or(0.5),
                param_norm(o, "punch").unwrap_or(0.5),
            )
        });
        let edited =
            prev_tp.is_some_and(|(pt, pp)| (pt - tone).abs() > 1e-4 || (pp - punch).abs() > 1e-4);
        let differs = !edited
            && other_tp
                .is_some_and(|(ot, op)| (ot - tone).abs() > 1e-4 || (op - punch).abs() > 1e-4);
        paint_scope_frame(&painter, rect, edited, differs);
        painter.hline(
            inner.x_range(),
            inner.center().y,
            Stroke::new(1.0_f32, theme::GRID_BEAT),
        );
        if let Some((pt, pp)) = prev_tp
            && edited
        {
            paint_drum_transient(
                &painter,
                inner,
                pt,
                pp,
                with_alpha(theme::TEXT_SECONDARY, 0.55),
                false,
            );
        } else if let Some((ot, op)) = other_tp
            && differs
        {
            paint_drum_transient(
                &painter,
                inner,
                ot,
                op,
                with_alpha(theme::BORDER_STRONG, 0.7),
                false,
            );
        }
        let active_col = if edited {
            theme::SELECTION
        } else {
            Color32::from_rgb(138, 152, 174)
        };
        paint_drum_transient(&painter, inner, tone, punch, active_col, true);
        painter.text(
            Pos2::new(inner.left() + 2.0, inner.top()),
            Align2::LEFT_TOP,
            "TRANSIENT",
            FontId::monospace(8.5),
            with_alpha(active_col, 0.85),
        );
        return;
    }

    // Specialised visual for Sample / Multisample / Granular / Wavetable scan sources.
    let has_sample_or_scan = all_graph_nodes(inst).any(|n| {
        matches!(
            n.kind,
            NodeKind::SampleSource
                | NodeKind::Multisample
                | NodeKind::Granular
                | NodeKind::Spectral
                | NodeKind::Wavetable
        )
    });
    let scan_key = ["position", "scan", "start"]
        .into_iter()
        .find(|k| inst.parameters.iter().any(|p| p.key == *k));
    if has_sample_or_scan && let Some(key) = scan_key {
        let pos = param_norm(inst, key).unwrap_or(0.2);
        let prev_pos = prev_inst.and_then(|p| param_norm(p, key));
        let other_pos = other_inst.and_then(|o| param_norm(o, key));
        let edited = prev_pos.is_some_and(|p| (p - pos).abs() > 1e-4);
        let differs = !edited && other_pos.is_some_and(|o| (o - pos).abs() > 1e-4);
        paint_scope_frame(&painter, rect, edited, differs);
        paint_sample_scan_scope(
            &painter,
            inner,
            pos,
            prev_pos.filter(|_| edited),
            other_pos.filter(|_| differs),
            key,
        );
        return;
    }

    // Standard oscillator waveform scope with previous waveform ghosting.
    let wave = primary_waveform(inst);
    let prev_wave = prev_inst.map(primary_waveform);
    let other_wave = other_inst.map(primary_waveform);
    let edited = prev_wave.as_ref().is_some_and(|pw| pw != &wave);
    let differs = !edited && other_wave.as_ref().is_some_and(|ow| ow != &wave);
    paint_scope_frame(&painter, rect, edited, differs);

    painter.hline(
        inner.x_range(),
        inner.center().y,
        Stroke::new(1.0_f32, theme::GRID_BEAT),
    );
    painter.vline(
        inner.center().x,
        inner.y_range(),
        Stroke::new(1.0_f32, theme::GRID_SUB),
    );

    if let Some(pw) = &prev_wave
        && edited
    {
        let ghost_pts = sample_osc_waveform(inner, pw);
        painter.add(egui::Shape::line(
            ghost_pts,
            Stroke::new(1.5_f32, with_alpha(theme::TEXT_SECONDARY, 0.55)),
        ));
    } else if let Some(ow) = &other_wave
        && differs
    {
        let ghost_pts = sample_osc_waveform(inner, ow);
        painter.add(egui::Shape::line(
            ghost_pts,
            Stroke::new(1.2_f32, with_alpha(theme::BORDER_STRONG, 0.75)),
        ));
    }

    let wave_col = if edited {
        theme::SELECTION
    } else {
        Color32::from_rgb(138, 152, 174)
    };
    let pts = sample_osc_waveform(inner, &wave);
    for w in pts.windows(2) {
        painter.add(egui::Shape::convex_polygon(
            vec![
                w[0],
                w[1],
                Pos2::new(w[1].x, inner.center().y),
                Pos2::new(w[0].x, inner.center().y),
            ],
            with_alpha(wave_col, if edited { 0.14 } else { 0.08 }),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(pts, Stroke::new(2.0_f32, wave_col)));
    let tag = if let Some(pw) = prev_wave.filter(|_| edited) {
        format!("{} (prev {})", wave.to_uppercase(), pw.to_uppercase())
    } else {
        wave.to_uppercase()
    };
    painter.text(
        Pos2::new(inner.left() + 2.0, inner.top()),
        Align2::LEFT_TOP,
        tag,
        FontId::monospace(8.5),
        with_alpha(wave_col, 0.85),
    );
}

fn param_norm(inst: &Instrument, needle: &str) -> Option<f32> {
    inst.parameters
        .iter()
        .find(|p| p.key.contains(needle))
        .map(|p| theme::value_to_norm(p.value, p.minimum, p.maximum, p.unit))
}

fn paint_drum_transient(
    painter: &egui::Painter,
    inner: Rect,
    tone: f32,
    punch: f32,
    color: Color32,
    fill: bool,
) {
    let steps = 72usize;
    let freq = 2.5 + tone * 8.5;
    let decay = 3.2 + (1.0 - punch) * 3.5;
    let amp = 0.45 + punch * 0.48;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let env = (-t * decay).exp();
            let sweep = (t * freq * 2.0 * PI * (1.0 - 0.35 * t)).sin();
            let y = sweep * env * amp;
            Pos2::new(
                inner.left() + t * inner.width(),
                inner.center().y - y * (inner.height() * 0.45),
            )
        })
        .collect();
    if fill {
        for w in pts.windows(2) {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    w[0],
                    w[1],
                    Pos2::new(w[1].x, inner.center().y),
                    Pos2::new(w[0].x, inner.center().y),
                ],
                with_alpha(color, 0.14),
                Stroke::NONE,
            ));
        }
    }
    painter.add(egui::Shape::line(
        pts,
        Stroke::new(if fill { 2.0_f32 } else { 1.4_f32 }, color),
    ));
}

fn paint_sample_scan_scope(
    painter: &egui::Painter,
    inner: Rect,
    pos: f32,
    prev_pos: Option<f32>,
    other_pos: Option<f32>,
    key: &str,
) {
    painter.hline(
        inner.x_range(),
        inner.center().y,
        Stroke::new(1.0_f32, theme::GRID_BEAT),
    );
    // Deterministic stylized harmonic sample envelope so the scan/start hairline has clear context.
    let edited = prev_pos.is_some();
    let accent = if edited {
        theme::SELECTION
    } else {
        Color32::from_rgb(138, 152, 174)
    };
    let bars = 44usize;
    let step = inner.width() / bars as f32;
    for i in 0..bars {
        let t = i as f32 / bars as f32;
        let h = ((t * 5.3).sin().abs() * 0.55 + (t * 13.7 + 0.8).cos().abs() * 0.35)
            * (1.0 - t * 0.45)
            * (inner.height() * 0.42);
        let x = inner.left() + (i as f32 + 0.5) * step;
        let active = t >= pos;
        painter.line_segment(
            [
                Pos2::new(x, inner.center().y - h),
                Pos2::new(x, inner.center().y + h),
            ],
            Stroke::new(
                (step * 0.55).clamp(1.5, 3.0),
                if active {
                    with_alpha(accent, if edited { 0.7 } else { 0.55 })
                } else {
                    with_alpha(theme::TEXT_MUTED, 0.25)
                },
            ),
        );
    }
    if let Some(p) = prev_pos {
        let px = inner.left() + p.clamp(0.0, 1.0) * inner.width();
        painter.vline(
            px,
            inner.y_range(),
            Stroke::new(1.5_f32, with_alpha(theme::TEXT_SECONDARY, 0.7)),
        );
    } else if let Some(o) = other_pos {
        let ox = inner.left() + o.clamp(0.0, 1.0) * inner.width();
        painter.vline(
            ox,
            inner.y_range(),
            Stroke::new(1.2_f32, with_alpha(theme::BORDER_STRONG, 0.8)),
        );
    }
    let cx = inner.left() + pos.clamp(0.0, 1.0) * inner.width();
    painter.vline(cx, inner.y_range(), Stroke::new(2.0_f32, accent));
    painter.text(
        Pos2::new(inner.left() + 2.0, inner.top()),
        Align2::LEFT_TOP,
        key.to_uppercase(),
        FontId::monospace(8.5),
        with_alpha(accent, 0.9),
    );
}

fn sample_osc_waveform(inner: Rect, wave_kind: &str) -> Vec<Pos2> {
    let steps = 72usize;
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let phase = (t * 2.0) % 1.0;
            let y = if wave_kind.contains("sine") {
                (phase * 2.0 * PI).sin()
            } else if wave_kind.contains("square") || wave_kind.contains("pulse") {
                if phase < 0.5 { 0.85 } else { -0.85 }
            } else if wave_kind.contains("tri") {
                if phase < 0.25 {
                    phase * 4.0
                } else if phase < 0.75 {
                    2.0 - phase * 4.0
                } else {
                    phase * 4.0 - 4.0
                }
            } else {
                let mut s = 0.0_f32;
                for h in 1..=6 {
                    let hf = h as f32;
                    s += (phase * 2.0 * PI * hf).sin() / hf;
                }
                (s * 0.58).clamp(-0.92, 0.92)
            };
            Pos2::new(
                inner.left() + t * inner.width(),
                inner.center().y - y * (inner.height() * 0.42),
            )
        })
        .collect()
}

fn paint_scope_frame(painter: &egui::Painter, rect: Rect, edited: bool, differs: bool) {
    painter.rect_filled(rect, 4.0, theme::BG_WELL);
    // Keep inner scope bezel neutral — the enclosing module card already carries the amber edit border.
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(
            1.0_f32,
            if edited || differs {
                theme::BORDER_STRONG
            } else {
                theme::BORDER_SUBTLE
            },
        ),
        StrokeKind::Inside,
    );
}

fn paint_filter_bode_scope(
    ui: &mut Ui,
    inst: &Instrument,
    curr: &FilterState,
    prev: Option<&FilterState>,
    other: Option<&FilterState>,
    width: f32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let inner = rect.shrink(6.0);

    let edited = prev.is_some_and(|p| curr.differs_from(p));
    let differs = !edited && other.is_some_and(|o| curr.differs_from(o));
    paint_scope_frame(&painter, rect, edited, differs);

    let x_of_hz = |hz: f64| -> f32 {
        let nx = ((hz.clamp(20.0, 20_000.0) / 20.0).ln() / (20_000.0_f64 / 20.0).ln()) as f32;
        inner.left() + nx * inner.width()
    };

    for &(freq, lbl) in &[(100.0_f64, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        let x = x_of_hz(freq);
        painter.vline(x, inner.y_range(), Stroke::new(1.0_f32, theme::GRID_SUB));
        painter.text(
            Pos2::new(x + 2.0, inner.bottom() - 1.0),
            Align2::LEFT_BOTTOM,
            lbl,
            FontId::monospace(8.0),
            theme::TEXT_MUTED,
        );
    }
    let zero_db_y = inner.top() + inner.height() * 0.38;
    painter.hline(
        inner.x_range(),
        zero_db_y,
        Stroke::new(1.0_f32, theme::GRID_BEAT),
    );

    // Static modulation sweep band around cutoff, matching sound_design.rs.
    if let Some((lo, hi)) = cutoff_mod_sweep_range(inst, curr.cutoff_hz) {
        let x_lo = x_of_hz(lo);
        let x_hi = x_of_hz(hi);
        if (x_hi - x_lo).abs() > 1.0 {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x_lo.min(x_hi), inner.top()),
                    Pos2::new(x_lo.max(x_hi), inner.bottom()),
                ),
                0.0,
                with_alpha(theme::ACCENT_VIOLET, 0.10),
            );
        }
    }

    let sample_curve = |state: &FilterState| -> Vec<Pos2> {
        let steps = 64usize;
        (0..=steps)
            .map(|i| {
                let nx = i as f32 / steps as f32;
                let f = 20.0 * (20_000.0_f64 / 20.0).powf(f64::from(nx));
                let db = filter_response_db(state, f);
                let y_frac = ((15.0 - db) / 43.0).clamp(0.0, 1.0) as f32;
                Pos2::new(
                    inner.left() + nx * inner.width(),
                    inner.top() + y_frac * inner.height(),
                )
            })
            .collect()
    };

    let dot_pos = |state: &FilterState| -> Pos2 {
        let x = x_of_hz(state.cutoff_hz);
        let y = (zero_db_y - (state.resonance as f32) * inner.height() * 0.30)
            .clamp(inner.top() + 5.0, inner.bottom() - 5.0);
        Pos2::new(x, y)
    };

    // Ghost previous (or quiet other) curve behind the current curve.
    if let Some(p) = prev
        && edited
    {
        let ghost_pts = sample_curve(p);
        painter.add(egui::Shape::line(
            ghost_pts,
            Stroke::new(1.5_f32, with_alpha(theme::TEXT_SECONDARY, 0.65)),
        ));
        let p_dot = dot_pos(p);
        painter.vline(
            p_dot.x,
            inner.y_range(),
            Stroke::new(1.0_f32, with_alpha(theme::TEXT_SECONDARY, 0.35)),
        );
        painter.circle_stroke(
            p_dot,
            3.5,
            Stroke::new(1.3_f32, with_alpha(theme::TEXT_SECONDARY, 0.85)),
        );
    } else if let Some(o) = other
        && differs
    {
        let ghost_pts = sample_curve(o);
        painter.add(egui::Shape::line(
            ghost_pts,
            Stroke::new(1.2_f32, with_alpha(theme::BORDER_STRONG, 0.85)),
        ));
    }

    // Current curve + translucent area fill.
    let pts = sample_curve(curr);
    for w in pts.windows(2) {
        painter.add(egui::Shape::convex_polygon(
            vec![
                w[0],
                w[1],
                Pos2::new(w[1].x, inner.bottom()),
                Pos2::new(w[0].x, inner.bottom()),
            ],
            with_alpha(theme::ACCENT_CYAN, if edited { 0.18 } else { 0.13 }),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(
        pts,
        Stroke::new(2.0_f32, theme::ACCENT_CYAN),
    ));

    let c_dot = dot_pos(curr);
    if let Some(p) = prev
        && edited
    {
        let p_dot = dot_pos(p);
        if (c_dot - p_dot).length() > 6.0 {
            paint_arrow(&painter, p_dot, c_dot, with_alpha(theme::SELECTION, 0.8));
        }
    }
    painter.vline(
        c_dot.x,
        inner.y_range(),
        Stroke::new(
            1.0_f32,
            with_alpha(
                if edited {
                    theme::SELECTION
                } else {
                    theme::ACCENT_AMBER
                },
                0.65,
            ),
        ),
    );
    painter.circle_filled(
        c_dot,
        4.0,
        if edited {
            theme::SELECTION
        } else {
            theme::ACCENT_AMBER
        },
    );
    painter.circle_stroke(c_dot, 4.0, Stroke::new(1.2_f32, theme::TEXT_PRIMARY));
}

fn cutoff_mod_sweep_range(inst: &Instrument, base_hz: f64) -> Option<(f64, f64)> {
    let param = inst.parameters.iter().find(|p| {
        p.unit == Unit::Hertz
            && (p.key.contains("cut") || p.key.contains("freq") || p.key.contains("filter"))
    })?;
    let mut lo = base_hz;
    let mut hi = base_hz;
    let mut any = false;
    for route in &inst.modulation_routes {
        if route.bypass || route.target != param.key {
            continue;
        }
        let Ok(depth) = param.unit.parse_value(&route.depth) else {
            continue;
        };
        if depth.abs() < 1e-6 {
            continue;
        }
        any = true;
        match route.polarity {
            ControlPolarity::Unipolar => {
                lo = lo.min(base_hz + depth.min(0.0));
                hi = hi.max(base_hz + depth.max(0.0));
            }
            ControlPolarity::Bipolar => {
                lo = lo.min(base_hz - depth.abs());
                hi = hi.max(base_hz + depth.abs());
            }
        }
    }
    any.then(|| {
        (
            lo.clamp(param.minimum, param.maximum),
            hi.clamp(param.minimum, param.maximum),
        )
    })
}

fn paint_adsr_scope(
    ui: &mut Ui,
    inst: &Instrument,
    curr: &AdsrState,
    prev: Option<&AdsrState>,
    other: Option<&AdsrState>,
    width: f32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let inner = rect.shrink(7.0);

    let edited = prev.is_some_and(|p| curr.differs_from(p));
    let differs = !edited && other.is_some_and(|o| curr.differs_from(o));
    paint_scope_frame(&painter, rect, edited, differs);

    // Log-compressed stage weights with a SHARED scale between current and previous so moving
    // decay/release visibly shifts only the affected breakpoints instead of rescaling everything.
    let stage_w = |s: f64| -> f32 {
        let log_n = ((s.clamp(0.001, 4.0) / 0.001).ln() / (4.0_f64 / 0.001).ln()) as f32;
        0.04 + log_n * 0.34
    };
    let ref_state = prev.or(other).unwrap_or(curr);
    let hold = 0.22_f32;
    let total = (stage_w(curr.attack_s) + stage_w(curr.decay_s) + hold + stage_w(curr.release_s))
        .max(
            stage_w(ref_state.attack_s)
                + stage_w(ref_state.decay_s)
                + hold
                + stage_w(ref_state.release_s),
        )
        .max(0.5);

    let adsr_points = |st: &AdsrState| -> [Pos2; 5] {
        let y_bot = inner.bottom();
        let y_top = inner.top() + 3.0;
        let y_sus = y_bot - (st.sustain as f32) * (y_bot - y_top);
        let x0 = inner.left();
        let xa = x0 + (stage_w(st.attack_s) / total) * inner.width();
        let xd = xa + (stage_w(st.decay_s) / total) * inner.width();
        let xs = xd + (hold / total) * inner.width();
        let xr = (xs + (stage_w(st.release_s) / total) * inner.width()).min(inner.right());
        [
            Pos2::new(x0, y_bot),
            Pos2::new(xa, y_top),
            Pos2::new(xd, y_sus),
            Pos2::new(xs, y_sus),
            Pos2::new(xr, y_bot),
        ]
    };

    let y_bot = inner.bottom();

    // Ghost previous (or quiet other) ADSR shape.
    if let Some(p) = prev
        && edited
    {
        let gp = adsr_points(p);
        painter.add(egui::Shape::line(
            gp.to_vec(),
            Stroke::new(1.5_f32, with_alpha(theme::TEXT_SECONDARY, 0.65)),
        ));
        for pt in &gp[1..4] {
            painter.circle_stroke(
                *pt,
                2.8,
                Stroke::new(1.2_f32, with_alpha(theme::TEXT_SECONDARY, 0.8)),
            );
        }
    } else if let Some(o) = other
        && differs
    {
        let gp = adsr_points(o);
        painter.add(egui::Shape::line(
            gp.to_vec(),
            Stroke::new(1.2_f32, with_alpha(theme::BORDER_STRONG, 0.8)),
        ));
    }

    let pts = adsr_points(curr);
    for w in pts.windows(2) {
        painter.add(egui::Shape::convex_polygon(
            vec![
                w[0],
                w[1],
                Pos2::new(w[1].x, y_bot),
                Pos2::new(w[0].x, y_bot),
            ],
            with_alpha(theme::ACCENT_VIOLET, if edited { 0.22 } else { 0.16 }),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(
        pts.to_vec(),
        Stroke::new(2.0_f32, theme::ACCENT_VIOLET),
    ));

    // Highlight moved breakpoints with amber ring & subtle motion arrow.
    let moved_node = [
        false,
        prev.is_some_and(|p| (p.attack_s - curr.attack_s).abs() > 1e-5),
        prev.is_some_and(|p| {
            (p.decay_s - curr.decay_s).abs() > 1e-5 || (p.sustain - curr.sustain).abs() > 1e-4
        }),
        prev.is_some_and(|p| {
            (p.decay_s - curr.decay_s).abs() > 1e-5 || (p.sustain - curr.sustain).abs() > 1e-4
        }),
        prev.is_some_and(|p| (p.release_s - curr.release_s).abs() > 1e-5),
    ];
    if let Some(p) = prev
        && edited
    {
        let gp = adsr_points(p);
        for i in 1..4 {
            if moved_node[i] && (pts[i] - gp[i]).length() > 5.0 {
                paint_arrow(&painter, gp[i], pts[i], with_alpha(theme::SELECTION, 0.75));
            }
        }
    }
    for i in 1..4 {
        let ring = if moved_node[i] {
            theme::SELECTION
        } else {
            theme::ACCENT_VIOLET
        };
        painter.circle_filled(pts[i], 3.2, theme::TEXT_PRIMARY);
        painter.circle_stroke(pts[i], 3.2, Stroke::new(1.3_f32, ring));
    }

    // Subtle LFO overlay when the instrument has an LFO source.
    let has_lfo = inst
        .modulation_sources
        .iter()
        .any(|m| matches!(m.kind, InstrumentModulationSourceKind::Lfo { .. }));
    if has_lfo {
        let lfo_pts: Vec<Pos2> = (0..=44)
            .map(|i| {
                let t = i as f32 / 44.0;
                let y = inner.center().y - (t * 4.0 * PI).sin() * (inner.height() * 0.20);
                Pos2::new(inner.left() + t * inner.width(), y)
            })
            .collect();
        painter.add(egui::Shape::line(
            lfo_pts,
            Stroke::new(1.1_f32, with_alpha(theme::ACCENT_CYAN, 0.45)),
        ));
    }
}

/// Small directional arrow from `from` to `to`, kept short of both endpoints so dots stay clean.
pub(super) fn paint_arrow(painter: &egui::Painter, from: Pos2, to: Pos2, color: Color32) {
    let delta = to - from;
    let len = delta.length();
    if len < 4.0 {
        return;
    }
    let dir = delta / len;
    let start = from + dir * 4.0;
    let end = to - dir * 4.5;
    if (end - start).dot(dir) <= 1.0 {
        return;
    }
    painter.line_segment([start, end], Stroke::new(1.3_f32, color));
    let perp = Vec2::new(-dir.y, dir.x);
    let head = 4.0_f32;
    painter.line_segment(
        [end, end - dir * head + perp * (head * 0.65)],
        Stroke::new(1.3_f32, color),
    );
    painter.line_segment(
        [end, end - dir * head - perp * (head * 0.65)],
        Stroke::new(1.3_f32, color),
    );
}

// ── Modulation Matrix (Bipolar Depth Bars + Ghosting) ─────────────────────────

/// Semantic comparison for modulation routes, ignoring source-file byte offsets (`sites`/`location`).
fn mod_route_changed(a: &InstrumentModulationRoute, b: &InstrumentModulationRoute) -> bool {
    a.id != b.id
        || a.source != b.source
        || a.target != b.target
        || a.depth != b.depth
        || a.polarity != b.polarity
        || a.curve != b.curve
        || a.order != b.order
        || a.bypass != b.bypass
        || a.auxiliary != b.auxiliary
        || a.operators != b.operators
}

/// Semantic comparison for modulation sources, ignoring `location` byte offsets.
fn mod_source_changed(a: &InstrumentModulationSource, b: &InstrumentModulationSource) -> bool {
    a.id != b.id || a.scope != b.scope || a.polarity != b.polarity || a.kind != b.kind
}

fn draw_modulation_matrix(
    ui: &mut Ui,
    inst: &Instrument,
    prev_inst: Option<&Instrument>,
    other_inst: Option<&Instrument>,
    other_word: &str,
    show_sources: bool,
) {
    let any_route_edited = inst.modulation_routes.iter().any(|r| {
        prev_inst.is_some_and(|p| {
            p.modulation_routes
                .iter()
                .find(|pr| pr.id == r.id)
                .is_none_or(|pr| mod_route_changed(pr, r))
        })
    }) || prev_inst.is_some_and(|p| {
        p.modulation_routes
            .iter()
            .any(|pr| !inst.modulation_routes.iter().any(|r| r.id == pr.id))
    });
    let any_source_edited = show_sources
        && (inst.modulation_sources.iter().any(|s| {
            prev_inst.is_some_and(|p| {
                p.modulation_sources
                    .iter()
                    .find(|ps| ps.id == s.id)
                    .is_none_or(|ps| mod_source_changed(ps, s))
            })
        }) || prev_inst.is_some_and(|p| {
            p.modulation_sources
                .iter()
                .any(|ps| !inst.modulation_sources.iter().any(|s| s.id == ps.id))
        }));

    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(
            1.0_f32,
            if any_route_edited || any_source_edited {
                theme::BORDER_STRONG
            } else {
                theme::BORDER_SUBTLE
            },
        ))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                widgets::section_header(ui, "MODULATION MATRIX", theme::ACCENT_VIOLET);
                let active = inst.modulation_routes.iter().filter(|r| !r.bypass).count();
                ui.label(
                    RichText::new(format!("{active}/{} active", inst.modulation_routes.len()))
                        .monospace()
                        .size(9.5)
                        .color(theme::TEXT_MUTED),
                );
            });
            ui.add_space(6.0);

            if show_sources && !inst.modulation_sources.is_empty() {
                draw_mod_sources_strip(ui, inst, prev_inst, other_inst, other_word);
                ui.add_space(6.0);
            }

            for route in &inst.modulation_routes {
                let prev_route =
                    prev_inst.and_then(|p| p.modulation_routes.iter().find(|r| r.id == route.id));
                let other_route =
                    other_inst.and_then(|o| o.modulation_routes.iter().find(|r| r.id == route.id));
                let added = prev_inst.is_some() && prev_route.is_none();
                draw_mod_route_row(ui, inst, route, prev_route, other_route, other_word, added);
                ui.add_space(4.0);
            }

            // Ghost any routes that were removed in this version.
            if let Some(prev) = prev_inst {
                for removed in &prev.modulation_routes {
                    if inst.modulation_routes.iter().any(|r| r.id == removed.id) {
                        continue;
                    }
                    draw_removed_mod_route_row(ui, prev, removed);
                    ui.add_space(4.0);
                }
            }
        });
}

fn draw_mod_sources_strip(
    ui: &mut Ui,
    inst: &Instrument,
    prev_inst: Option<&Instrument>,
    other_inst: Option<&Instrument>,
    other_word: &str,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for src in &inst.modulation_sources {
            let prev = prev_inst.and_then(|p| p.modulation_sources.iter().find(|s| s.id == src.id));
            let other =
                other_inst.and_then(|o| o.modulation_sources.iter().find(|s| s.id == src.id));
            let edited = prev_inst.is_some() && prev.is_none_or(|p| mod_source_changed(p, src));
            let differs = !edited && other.is_some_and(|o| mod_source_changed(o, src));
            let summary = describe_mod_source(src);
            let text = if let Some(p) = prev
                && edited
            {
                format!("{}: {summary} (prev {})", src.id, describe_mod_source(p))
            } else if prev_inst.is_some() && prev.is_none() {
                format!("{}: {summary} (added)", src.id)
            } else if let Some(o) = other
                && differs
            {
                format!(
                    "{}: {summary} ({other_word} {})",
                    src.id,
                    describe_mod_source(o)
                )
            } else {
                format!("{}: {summary}", src.id)
            };
            setting_chip(ui, &text, edited, differs);
        }
    });
}

fn describe_mod_source(src: &InstrumentModulationSource) -> String {
    let scope = match src.scope {
        InstrumentModulationScope::Global => "global",
        InstrumentModulationScope::Instrument => "inst",
        InstrumentModulationScope::PerVoice => "voice",
    };
    match &src.kind {
        InstrumentModulationSourceKind::Lfo { waveform, rate, .. } => {
            let w = lfo_waveform_name(*waveform);
            let r = match rate {
                InstrumentModulationRate::FreeHertz(hz) => {
                    theme::format_compact_value(*hz, Unit::Hertz)
                }
                InstrumentModulationRate::Synchronized(s) => s.clone(),
            };
            format!("LFO {w} {r} · {scope}")
        }
        InstrumentModulationSourceKind::NoteVelocity => format!("velocity · {scope}"),
        InstrumentModulationSourceKind::NoteKey {
            reference,
            range_semitones,
        } => format!(
            "key {} ±{range_semitones:.0}st",
            crate::model::format_pitch(*reference)
        ),
        InstrumentModulationSourceKind::KeyCurve { points } => {
            format!("key curve ({} pts)", points.len())
        }
        InstrumentModulationSourceKind::NoteGate => "note gate".to_owned(),
        InstrumentModulationSourceKind::NoteAge { duration_seconds } => {
            format!("note age {}", format_ms(*duration_seconds))
        }
        InstrumentModulationSourceKind::RandomHold { rate, .. } => {
            let r = match rate {
                InstrumentModulationRate::FreeHertz(hz) => {
                    theme::format_compact_value(*hz, Unit::Hertz)
                }
                InstrumentModulationRate::Synchronized(s) => s.clone(),
            };
            format!("S&H {r} · {scope}")
        }
        InstrumentModulationSourceKind::GraphPort { node, port } => format!("{node}.{port}"),
        InstrumentModulationSourceKind::Parameter { parameter } => format!("${parameter}"),
    }
}

pub(super) fn lfo_waveform_name(w: LfoWaveform) -> &'static str {
    match w {
        LfoWaveform::Sine => "SINE",
        LfoWaveform::Triangle => "TRI",
        LfoWaveform::Saw => "SAW",
        LfoWaveform::Square => "SQR",
        LfoWaveform::SampleHold => "S&H",
    }
}

fn draw_mod_route_row(
    ui: &mut Ui,
    inst: &Instrument,
    route: &InstrumentModulationRoute,
    prev: Option<&InstrumentModulationRoute>,
    other: Option<&InstrumentModulationRoute>,
    other_word: &str,
    added: bool,
) {
    let target_param = inst.parameters.iter().find(|p| p.key == route.target);
    let unit = target_param.map_or(Unit::Ratio, |p| p.unit);
    let max_depth = target_param
        .map(|p| (p.maximum - p.minimum).abs().max(1.0))
        .unwrap_or(1.0);
    let curr_depth = unit.parse_value(&route.depth).unwrap_or(0.0);
    let prev_depth = prev.and_then(|r| unit.parse_value(&r.depth).ok());
    let other_depth = other.and_then(|r| unit.parse_value(&r.depth).ok());
    let depth_mark = mark(curr_depth, prev_depth, other_depth);

    let bypass_edited = prev.is_some_and(|p| p.bypass != route.bypass);
    let route_edited = added || prev.is_some_and(|p| mod_route_changed(p, route));
    let route_differs = !route_edited && other.is_some_and(|o| mod_route_changed(o, route));

    let border = if route_edited {
        theme::SELECTION
    } else if route_differs {
        theme::BORDER_STRONG
    } else {
        theme::BORDER_SUBTLE
    };

    egui::Frame::new()
        .fill(if route.bypass {
            theme::BG_WELL
        } else {
            theme::BG_SURFACE
        })
        .stroke(Stroke::new(
            if route_edited { 1.3_f32 } else { 1.0_f32 },
            border,
        ))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let narrow = ui.available_width() < 460.0;
            ui.horizontal(|ui| {
                // ON/OFF bypass pill.
                let state_text = if added {
                    "NEW"
                } else if route.bypass {
                    "OFF"
                } else {
                    "ON"
                };
                let pill_col = if added || bypass_edited {
                    theme::SELECTION
                } else if route.bypass {
                    theme::TEXT_MUTED
                } else {
                    theme::ACCENT_VIOLET
                };
                badge_cell(ui, state_text, pill_col, 34.0, bypass_edited || added);

                let chip_w = if narrow { 68.0 } else { 88.0 };
                badge_cell(
                    ui,
                    &route.source,
                    theme::ACCENT_VIOLET,
                    chip_w,
                    prev.is_some_and(|p| p.source != route.source),
                );
                paint_route_arrow_widget(ui);
                badge_cell(
                    ui,
                    target_param.map_or(&route.target, |p| &p.name),
                    theme::ACCENT_CYAN,
                    chip_w,
                    prev.is_some_and(|p| p.target != route.target),
                );

                // Bipolar depth bar takes remaining width minus polarity/curve/note tags.
                let pol_text = match route.polarity {
                    ControlPolarity::Unipolar => "UNI",
                    ControlPolarity::Bipolar => "BI",
                };
                let curve_tag = match route.curve {
                    ModulationCurve::Linear => None,
                    ModulationCurve::Smoothstep => Some("SMOOTH".to_owned()),
                    ModulationCurve::Power(e) => Some(format!("POW {e:.1}")),
                };
                let reserve = 34.0 + if curve_tag.is_some() { 54.0 } else { 0.0 };
                let bar_w = (ui.available_width() - reserve).clamp(90.0, 380.0);

                let depth_lit = added || matches!(depth_mark, Some(Mark::Edited(_)));
                paint_bipolar_depth_bar(
                    ui,
                    curr_depth,
                    max_depth,
                    target_param,
                    unit,
                    depth_mark,
                    if route.bypass {
                        theme::TEXT_MUTED
                    } else if depth_lit {
                        theme::SELECTION
                    } else {
                        theme::ACCENT_VIOLET
                    },
                    bar_w,
                    20.0,
                );

                let pol_edited = prev.is_some_and(|p| p.polarity != route.polarity);
                badge_cell(ui, pol_text, theme::TEXT_SECONDARY, 28.0, pol_edited);
                if let Some(tag) = curve_tag {
                    let curve_edited = prev.is_some_and(|p| p.curve != route.curve);
                    badge_cell(ui, &tag, theme::TEXT_MUTED, 48.0, curve_edited);
                }
            });

            // Secondary delta caption line when anything on this route moved or differs.
            let mut notes = Vec::new();
            match depth_mark {
                Some(Mark::Edited(prev_d)) => notes.push(format!(
                    "depth previous {}",
                    theme::format_compact_value(prev_d, unit)
                )),
                Some(Mark::Differs(other_d)) => notes.push(format!(
                    "depth {other_word} {}",
                    theme::format_compact_value(other_d, unit)
                )),
                None => {}
            }
            if bypass_edited && let Some(p) = prev {
                notes.push(format!(
                    "previous {}",
                    if p.bypass { "bypassed" } else { "on" }
                ));
            }
            if let Some(p) = prev {
                if p.polarity != route.polarity {
                    notes.push(format!("polarity previous {:?}", p.polarity));
                }
                if p.curve != route.curve {
                    notes.push(format!("curve previous {:?}", p.curve));
                }
            }
            if !notes.is_empty() {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(notes.join(" · "))
                        .monospace()
                        .size(9.5)
                        .color(if route_edited {
                            theme::SELECTION
                        } else {
                            theme::TEXT_MUTED
                        }),
                );
            }
        });
}

fn draw_removed_mod_route_row(
    ui: &mut Ui,
    prev_inst: &Instrument,
    route: &InstrumentModulationRoute,
) {
    let target_param = prev_inst.parameters.iter().find(|p| p.key == route.target);
    let unit = target_param.map_or(Unit::Ratio, |p| p.unit);
    let curr_depth = unit.parse_value(&route.depth).unwrap_or(0.0);
    egui::Frame::new()
        .fill(theme::BG_WELL)
        .stroke(Stroke::new(1.0_f32, with_alpha(theme::SIG_CLIP, 0.6)))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                badge_cell(ui, "DEL", theme::SIG_CLIP, 34.0, false);
                badge_cell(ui, &route.source, theme::TEXT_MUTED, 84.0, false);
                paint_route_arrow_widget(ui);
                badge_cell(
                    ui,
                    target_param.map_or(&route.target, |p| &p.name),
                    theme::TEXT_MUTED,
                    84.0,
                    false,
                );
                ui.label(
                    RichText::new(format!(
                        "removed (was {})",
                        theme::format_compact_value(curr_depth, unit)
                    ))
                    .monospace()
                    .size(10.0)
                    .color(theme::TEXT_MUTED),
                );
            });
        });
}

/// Computes perceptual bipolar bar fraction in `-1..=1` for a modulation route depth.
///
/// Uses a symmetric signed-log curve for Hertz depths so positive and negative Hz offsets always
/// produce a proportional bar even when the target parameter sits near its ceiling or floor.
pub(super) fn depth_fraction(
    value: f64,
    max_depth: f64,
    _target_param: Option<&Parameter>,
    unit: Unit,
) -> f32 {
    if unit == Unit::Hertz {
        if value.abs() < 1e-6 {
            return 0.0;
        }
        let sign = value.signum();
        let mag = (1.0 + value.abs() / 120.0).ln() / (1.0 + max_depth.abs() / 120.0).ln().max(1e-6);
        (sign * mag).clamp(-1.0, 1.0) as f32
    } else {
        ((value / max_depth.abs().max(1e-6)) as f32).clamp(-1.0, 1.0)
    }
}

/// Read-only bipolar depth bar with previous value ghosted as a translucent band + marker tick.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_bipolar_depth_bar(
    ui: &mut Ui,
    value: f64,
    max_depth: f64,
    target_param: Option<&Parameter>,
    unit: Unit,
    mark: Option<Mark>,
    accent: Color32,
    width: f32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::BG_WELL);
    let center_x = rect.center().x;
    let half_w = rect.width() * 0.48;

    let edited = matches!(mark, Some(Mark::Edited(_)));
    let differs = matches!(mark, Some(Mark::Differs(_)));

    // Ghost previous or other depth bar behind the current bar.
    if let Some(Mark::Edited(prev)) = mark {
        let pf = depth_fraction(prev, max_depth, target_param, unit);
        let px = center_x + pf * half_w;
        let (gx0, gx1) = if px >= center_x {
            (center_x, px)
        } else {
            (px, center_x)
        };
        if (gx1 - gx0).abs() > 1.0 {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(gx0, rect.top() + 3.0),
                    Pos2::new(gx1, rect.bottom() - 3.0),
                ),
                1.5,
                with_alpha(theme::TEXT_SECONDARY, 0.24),
            );
        }
        painter.vline(
            px,
            (rect.top() + 1.0)..=(rect.bottom() - 1.0),
            Stroke::new(2.0_f32, theme::TEXT_SECONDARY),
        );
    } else if let Some(Mark::Differs(other)) = mark {
        let of = depth_fraction(other, max_depth, target_param, unit);
        let ox = center_x + of * half_w;
        painter.vline(
            ox,
            (rect.top() + 2.0)..=(rect.bottom() - 2.0),
            Stroke::new(1.5_f32, theme::BORDER_STRONG),
        );
    }

    let frac = depth_fraction(value, max_depth, target_param, unit);
    let bar_end = center_x + frac * half_w;
    let (x0, x1) = if bar_end >= center_x {
        (center_x, bar_end)
    } else {
        (bar_end, center_x)
    };
    if (x1 - x0).abs() > 1.0 {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x0, rect.top() + 2.0),
                Pos2::new(x1, rect.bottom() - 2.0),
            ),
            2.0,
            with_alpha(accent, if edited { 0.72 } else { 0.52 }),
        );
    }

    painter.vline(
        center_x,
        (rect.top() + 2.0)..=(rect.bottom() - 2.0),
        Stroke::new(1.0_f32, theme::BORDER_STRONG),
    );
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(
            if edited { 1.3_f32 } else { 1.0_f32 },
            if edited {
                theme::SELECTION
            } else if differs {
                theme::BORDER_STRONG
            } else {
                theme::BORDER_SUBTLE
            },
        ),
        StrokeKind::Inside,
    );

    // Value label sits on the side the bar does not grow into so the edge never cuts through it.
    let (anchor, at) = if frac >= 0.0 {
        (
            Align2::LEFT_CENTER,
            Pos2::new(rect.left() + 6.0, rect.center().y),
        )
    } else {
        (
            Align2::RIGHT_CENTER,
            Pos2::new(rect.right() - 6.0, rect.center().y),
        )
    };
    let sign = if value > 0.0 { "+" } else { "" };
    painter.text(
        at,
        anchor,
        format!("{sign}{}", theme::format_compact_value(value, unit)),
        FontId::monospace(9.5),
        theme::TEXT_PRIMARY,
    );
}

fn badge_cell(ui: &mut Ui, text: &str, color: Color32, width: f32, lit: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, with_alpha(color, if lit { 0.22 } else { 0.13 }));
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(
            1.0_f32,
            if lit {
                theme::SELECTION
            } else {
                with_alpha(color, 0.35)
            },
        ),
        StrokeKind::Inside,
    );
    let shown = widgets::ellipsize(painter, text, &FontId::monospace(9.5), width - 8.0);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        shown,
        FontId::monospace(9.5),
        if lit { theme::TEXT_PRIMARY } else { color },
    );
}

fn paint_route_arrow_widget(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, 20.0), Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    let x0 = rect.left() + 2.0;
    let x1 = rect.right() - 2.0;
    let stroke = Stroke::new(1.3_f32, theme::TEXT_MUTED);
    painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], stroke);
    painter.line_segment([Pos2::new(x1 - 3.5, y - 3.0), Pos2::new(x1, y)], stroke);
    painter.line_segment([Pos2::new(x1 - 3.5, y + 3.0), Pos2::new(x1, y)], stroke);
}

// ── Voicing, Sampler, Graph Nodes, Faceplate ─────────────────────────────────

/// Semantic comparison for Voicing, ignoring source-file `ValueSite` byte offsets.
fn voicing_changed(a: &crate::model::Voicing, b: &crate::model::Voicing) -> bool {
    a.mode != b.mode
        || a.retrigger != b.retrigger
        || (a.glide_seconds - b.glide_seconds).abs() > 1e-9
        || a.glide_mode != b.glide_mode
        || a.note_priority != b.note_priority
        || a.steal_priority != b.steal_priority
        || a.seed != b.seed
}

fn draw_voicing_card(
    ui: &mut Ui,
    inst: &Instrument,
    prev_inst: Option<&Instrument>,
    other_inst: Option<&Instrument>,
    other_word: &str,
) {
    let v = &inst.voicing;
    let pv = prev_inst.map(|p| &p.voicing);
    let ov = other_inst.map(|o| &o.voicing);
    let edited = pv.is_some_and(|p| voicing_changed(p, v))
        || prev_inst.is_some_and(|p| p.polyphony != inst.polyphony);

    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(
            if edited { 1.5_f32 } else { 1.0_f32 },
            if edited {
                theme::SELECTION
            } else {
                theme::BORDER_SUBTLE
            },
        ))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_header(ui, "VOICING & POLYPHONY", theme::ACCENT_AMBER);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                // Glide dial with ghosting.
                let max_glide = v
                    .glide_seconds
                    .max(pv.map_or(0.0, |p| p.glide_seconds))
                    .max(0.5);
                dial(
                    ui,
                    "Glide",
                    v.glide_seconds,
                    0.0,
                    max_glide,
                    Unit::Seconds,
                    &|s| format_ms(s),
                    mark(
                        v.glide_seconds,
                        pv.map(|p| p.glide_seconds),
                        ov.map(|o| o.glide_seconds),
                    ),
                    other_word,
                    theme::ACCENT_AMBER,
                );
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                        voicing_chip(
                            ui,
                            "Mode",
                            &format!("{:?}", v.mode),
                            pv.map(|p| format!("{:?}", p.mode)),
                            ov.map(|o| format!("{:?}", o.mode)),
                            other_word,
                        );
                        voicing_chip(
                            ui,
                            "Voices",
                            &inst.polyphony.to_string(),
                            prev_inst.map(|p| p.polyphony.to_string()),
                            other_inst.map(|o| o.polyphony.to_string()),
                            other_word,
                        );
                        voicing_chip(
                            ui,
                            "Retrigger",
                            &format!("{:?}", v.retrigger),
                            pv.map(|p| format!("{:?}", p.retrigger)),
                            ov.map(|o| format!("{:?}", o.retrigger)),
                            other_word,
                        );
                        voicing_chip(
                            ui,
                            "Glide Mode",
                            &format!("{:?}", v.glide_mode),
                            pv.map(|p| format!("{:?}", p.glide_mode)),
                            ov.map(|o| format!("{:?}", o.glide_mode)),
                            other_word,
                        );
                        voicing_chip(
                            ui,
                            "Note Priority",
                            &format!("{:?}", v.note_priority),
                            pv.map(|p| format!("{:?}", p.note_priority)),
                            ov.map(|o| format!("{:?}", o.note_priority)),
                            other_word,
                        );
                        voicing_chip(
                            ui,
                            "Steal",
                            &format!("{:?}", v.steal_priority),
                            pv.map(|p| format!("{:?}", p.steal_priority)),
                            ov.map(|o| format!("{:?}", o.steal_priority)),
                            other_word,
                        );
                    });
                });
            });
        });
}

fn voicing_chip(
    ui: &mut Ui,
    label: &str,
    curr: &str,
    prev: Option<String>,
    other: Option<String>,
    other_word: &str,
) {
    let prev_diff = prev.filter(|p| p != curr);
    let other_diff = other.filter(|o| o != curr);
    let (text, lit, quiet) = match (prev_diff, other_diff) {
        (Some(p), _) => (format!("{label}: {curr} (prev {p})"), true, false),
        (None, Some(o)) => (format!("{label}: {curr} ({other_word} {o})"), false, true),
        _ => (format!("{label}: {curr}"), false, false),
    };
    setting_chip(ui, &text, lit, quiet);
}

fn draw_sampler_card(
    ui: &mut Ui,
    curr: Option<&SamplerDefinition>,
    prev: Option<&SamplerDefinition>,
    other: Option<&SamplerDefinition>,
    other_word: &str,
) {
    let curr_maps = curr.map_or(&[][..], |s| s.mappings.as_slice());
    let prev_maps = prev.map_or(&[][..], |s| s.mappings.as_slice());
    let other_maps = other.map_or(&[][..], |s| s.mappings.as_slice());

    let (min_key, max_key) = curr_maps
        .iter()
        .chain(prev_maps.iter())
        .fold((36u8, 84u8), |(lo, hi), m| {
            (lo.min(m.low_key), hi.max(m.high_key))
        });
    let span_keys = f32::from(max_key.saturating_sub(min_key).max(12)) + 1.0;

    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_header(ui, "SAMPLER KEYMAP", theme::ACCENT_AMBER);
            ui.add_space(6.0);

            let width = ui.available_width().max(160.0);
            let rows = curr_maps.len().clamp(1, 6);
            let row_h = 18.0_f32;
            let kb_h = 14.0_f32;
            let total_h = kb_h + rows as f32 * row_h + 6.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, total_h), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, theme::BG_WELL);

            let x_of = |key: u8| {
                rect.left()
                    + 4.0
                    + (f32::from(key.saturating_sub(min_key)) / span_keys) * (rect.width() - 8.0)
            };
            let key_w = (rect.width() - 8.0) / span_keys;

            // Mini keyboard ruler across the top.
            for k in min_key..=max_key {
                let x = x_of(k);
                let black = matches!(k % 12, 1 | 3 | 6 | 8 | 10);
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(x, rect.top() + 1.0),
                        Vec2::new(key_w.max(1.0), kb_h - 2.0),
                    ),
                    0.0,
                    if black {
                        Color32::from_rgb(20, 23, 30)
                    } else {
                        theme::BG_SURFACE
                    },
                );
                if k % 12 == 0 {
                    painter.vline(x, rect.y_range(), Stroke::new(1.0_f32, theme::GRID_BAR));
                    painter.text(
                        Pos2::new(x + 2.0, rect.top() + kb_h * 0.5),
                        Align2::LEFT_CENTER,
                        crate::model::format_pitch(k),
                        FontId::monospace(8.0),
                        theme::TEXT_MUTED,
                    );
                }
            }

            // Zone bars.
            for (i, mapping) in curr_maps.iter().enumerate() {
                let y = rect.top() + kb_h + 3.0 + (i % rows) as f32 * row_h;
                let pm = prev_maps.iter().find(|m| m.id == mapping.id);
                let om = other_maps.iter().find(|m| m.id == mapping.id);
                let col = theme::track_color(i);
                let edited = prev.is_some() && pm.is_none_or(|p| mapping_changed(p, mapping));
                let differs = !edited && om.is_some_and(|o| mapping_changed(o, mapping));

                if let Some(p) = pm
                    && (p.low_key != mapping.low_key || p.high_key != mapping.high_key)
                {
                    let gr = Rect::from_min_max(
                        Pos2::new(x_of(p.low_key), y + 2.0),
                        Pos2::new(x_of(p.high_key) + key_w, y + row_h - 2.0),
                    );
                    painter.rect_stroke(
                        gr,
                        3.0,
                        Stroke::new(1.0_f32, with_alpha(theme::TEXT_SECONDARY, 0.7)),
                        StrokeKind::Inside,
                    );
                }
                let zr = Rect::from_min_max(
                    Pos2::new(x_of(mapping.low_key), y + 2.0),
                    Pos2::new(x_of(mapping.high_key) + key_w, y + row_h - 2.0),
                );
                painter.rect_filled(zr, 3.0, with_alpha(col, 0.35));
                painter.rect_stroke(
                    zr,
                    3.0,
                    Stroke::new(
                        if edited { 1.5_f32 } else { 1.0_f32 },
                        if edited {
                            theme::SELECTION
                        } else if differs {
                            theme::BORDER_STRONG
                        } else {
                            with_alpha(col, 0.8)
                        },
                    ),
                    StrokeKind::Inside,
                );
                // Root key marker dot.
                painter.circle_filled(
                    Pos2::new(x_of(mapping.root_key) + key_w * 0.5, zr.center().y),
                    2.5,
                    theme::TEXT_PRIMARY,
                );
                painter.text(
                    Pos2::new(zr.left() + 4.0, zr.center().y),
                    Align2::LEFT_CENTER,
                    format!("{} ({})", mapping.id, mapping.asset),
                    FontId::monospace(8.5),
                    theme::TEXT_PRIMARY,
                );
            }

            // Zone detail chips for edited mappings.
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                for m in curr_maps {
                    let pm = prev_maps.iter().find(|p| p.id == m.id);
                    let om = other_maps.iter().find(|o| o.id == m.id);
                    let edited = prev.is_some() && pm.is_none_or(|p| mapping_changed(p, m));
                    let differs = !edited && om.is_some_and(|o| mapping_changed(o, m));
                    let dir = match m.direction {
                        SampleDirection::Forward => "fwd",
                        SampleDirection::Reverse => "rev",
                    };
                    let pb = match m.playback {
                        SamplePlayback::OneShot => "1shot",
                        SamplePlayback::Gated => "gated",
                    };
                    let mut text = format!(
                        "{}: {}-{} root {} · {:.1} dB · {pb} {dir}",
                        m.id,
                        crate::model::format_pitch(m.low_key),
                        crate::model::format_pitch(m.high_key),
                        crate::model::format_pitch(m.root_key),
                        m.gain_db
                    );
                    if let Some(p) = pm
                        && (p.gain_db - m.gain_db).abs() > 1e-3
                    {
                        text.push_str(&format!(" (prev {:.1} dB)", p.gain_db));
                    } else if let Some(o) = om
                        && differs
                        && (o.gain_db - m.gain_db).abs() > 1e-3
                    {
                        text.push_str(&format!(" ({other_word} {:.1} dB)", o.gain_db));
                    }
                    setting_chip(ui, &text, edited, differs);
                }
            });
        });
}

fn mapping_changed(a: &SampleMapping, b: &SampleMapping) -> bool {
    a.asset != b.asset
        || a.low_key != b.low_key
        || a.high_key != b.high_key
        || a.root_key != b.root_key
        || a.start_frame != b.start_frame
        || a.end_frame != b.end_frame
        || a.playback != b.playback
        || a.direction != b.direction
        || a.loop_region != b.loop_region
        || (a.gain_db - b.gain_db).abs() > 1e-6
        || (a.pan - b.pan).abs() > 1e-6
        || (a.attack_seconds - b.attack_seconds).abs() > 1e-6
        || (a.release_seconds - b.release_seconds).abs() > 1e-6
}

/// Draws visual node cards for any graph / shared_graph nodes whose settings were edited.
/// Returns `true` if node setting changes were rendered visually.
fn draw_edited_graph_nodes(
    ui: &mut Ui,
    inst: &Instrument,
    prev_inst: Option<&Instrument>,
    other_inst: Option<&Instrument>,
    graph_changes: &[&ValueChange],
    other_word: &str,
) -> bool {
    let mut touched: Vec<(&str, &str)> = Vec::new();
    for change in graph_changes {
        if is_node_setting_path(&change.path) {
            let pair = (change.path[0].as_str(), change.path[2].as_str());
            if !touched.contains(&pair) {
                touched.push(pair);
            }
        }
    }
    if touched.is_empty() {
        return false;
    }

    ui.add_space(10.0);
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(1.5_f32, theme::SELECTION))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_header(ui, "GRAPH NODES", theme::ACCENT_CYAN);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                for &(domain, node_id) in &touched {
                    let Some(node) = graph_node_by_id(inst, domain, node_id) else {
                        continue;
                    };
                    let prev_node = prev_inst.and_then(|p| graph_node_by_id(p, domain, node_id));
                    let other_node = other_inst.and_then(|o| graph_node_by_id(o, domain, node_id));
                    draw_graph_node_card(ui, node, prev_node, other_node, other_word);
                }
            });
        });
    true
}

fn graph_node_by_id<'a>(inst: &'a Instrument, domain: &str, id: &str) -> Option<&'a GraphNode> {
    let graph = if domain == "shared_graph" {
        inst.shared_graph.as_ref()
    } else {
        inst.graph.as_ref()
    };
    graph.and_then(|g| g.nodes.iter().find(|n| n.id == id))
}

fn draw_graph_node_card(
    ui: &mut Ui,
    node: &GraphNode,
    prev: Option<&GraphNode>,
    other: Option<&GraphNode>,
    other_word: &str,
) {
    let accent = if is_filter_node(node) {
        theme::ACCENT_CYAN
    } else if is_envelope_node(node) {
        theme::ACCENT_VIOLET
    } else if is_source_node(node) {
        theme::ACCENT_AMBER
    } else {
        theme::TEXT_SECONDARY
    };
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&node.id)
                            .strong()
                            .size(11.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(theme::node_kind_label(node.kind))
                            .monospace()
                            .size(9.5)
                            .color(accent),
                    );
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
                    for s in &node.settings {
                        let fmt_val = |v: &SettingValue| match v {
                            SettingValue::Number(n) => {
                                let text = format!("{n:.3}");
                                text.trim_end_matches('0').trim_end_matches('.').to_owned()
                            }
                            SettingValue::Parameter(p) => format!("${p}"),
                            SettingValue::Text(t) => readable_literal(t),
                        };
                        let curr_str = fmt_val(&s.value);
                        let prev_val = prev
                            .and_then(|p| p.setting(&s.key))
                            .filter(|pv| *pv != &s.value)
                            .map(fmt_val);
                        let other_val = other
                            .and_then(|o| o.setting(&s.key))
                            .filter(|ov| *ov != &s.value)
                            .map(fmt_val);
                        let (text, lit, quiet) = match (prev_val, other_val) {
                            (Some(pv), _) => {
                                (format!("{} {curr_str} (prev {pv})", s.key), true, false)
                            }
                            (None, Some(ov)) => (
                                format!("{} {curr_str} ({other_word} {ov})", s.key),
                                false,
                                true,
                            ),
                            _ => (format!("{} {curr_str}", s.key), false, false),
                        };
                        setting_chip(ui, &text, lit, quiet);
                    }
                });
            });
        });
}

fn draw_faceplate_card(
    ui: &mut Ui,
    controls: &[FaceplateControl],
    prev: Option<&[FaceplateControl]>,
    other: Option<&[FaceplateControl]>,
    other_word: &str,
) {
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(1.5_f32, theme::SELECTION))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_header(ui, "FACEPLATE", Kind::Instrument.color());
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for ctrl in controls {
                    let pc = prev.and_then(|list| list.iter().find(|c| c.key == ctrl.key));
                    let oc = other.and_then(|list| list.iter().find(|c| c.key == ctrl.key));
                    let edited = prev.is_some()
                        && pc.is_none_or(|p| {
                            p.label != ctrl.label
                                || p.parameter != ctrl.parameter
                                || p.explanation != ctrl.explanation
                        });
                    let differs = !edited
                        && oc.is_some_and(|o| {
                            o.label != ctrl.label
                                || o.parameter != ctrl.parameter
                                || o.explanation != ctrl.explanation
                        });
                    let mut text = format!("{} → ${}", ctrl.label, ctrl.parameter);
                    if let Some(p) = pc
                        && edited
                        && (p.label != ctrl.label || p.parameter != ctrl.parameter)
                    {
                        text.push_str(&format!(" (prev {} → ${})", p.label, p.parameter));
                    } else if let Some(o) = oc
                        && differs
                        && (o.label != ctrl.label || o.parameter != ctrl.parameter)
                    {
                        text.push_str(&format!(" ({other_word} {} → ${})", o.label, o.parameter));
                    }
                    setting_chip(ui, &text, edited, differs);
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_deep_house() -> Project {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/deep-house-reference/project.crump");
        crate::project::load_project(&path)
            .expect("load reference project")
            .project
            .expect("parsed project")
    }

    #[test]
    fn every_instrument_parameter_is_assigned_once() {
        let project = load_deep_house();
        for inst in &project.song.instruments {
            let modules = build_synth_modules(inst);
            let grouped: Vec<&str> = modules
                .iter()
                .flat_map(|m| m.params.iter().map(|p| p.key.as_str()))
                .collect();
            assert_eq!(
                grouped.len(),
                inst.parameters.len(),
                "instrument {} dropped or duplicated params",
                inst.id
            );
            for p in &inst.parameters {
                assert!(
                    grouped.contains(&p.key.as_str()),
                    "missing {} in {}",
                    p.key,
                    inst.id
                );
            }
        }
    }

    #[test]
    fn dh_bass_groups_cutoff_and_drive_under_filter_and_reads_lowpass() {
        let project = load_deep_house();
        let bass = project
            .song
            .instruments
            .iter()
            .find(|i| i.id == "dh-bass")
            .expect("dh-bass");
        let modules = build_synth_modules(bass);
        let filter_mod = modules
            .iter()
            .find(|m| m.kind == SynthModuleKind::Filter)
            .expect("filter module");
        let keys: Vec<&str> = filter_mod.params.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, vec!["cutoff", "drive"]);

        let state = read_filter_state(bass);
        assert_eq!(state.mode, FilterMode::Lowpass);
        assert!(state.cutoff_hz >= 70.0 && state.cutoff_hz <= 2400.0);
        // Lowpass passes low frequencies near 0 dB and attenuates high frequencies.
        assert!(filter_response_db(&state, 40.0) > -3.0);
        assert!(filter_response_db(&state, 16_000.0) < -15.0);
    }

    #[test]
    fn dh_transition_reads_highpass_filter_mode() {
        let project = load_deep_house();
        let tr = project
            .song
            .instruments
            .iter()
            .find(|i| i.id == "dh-transition")
            .expect("dh-transition");
        let state = read_filter_state(tr);
        assert_eq!(state.mode, FilterMode::Highpass);
        assert!(filter_response_db(&state, 40.0) < -15.0);
        assert!(filter_response_db(&state, 16_000.0) > -3.0);
    }

    #[test]
    fn dh_keys_reads_decay_parameter_into_adsr() {
        let project = load_deep_house();
        let keys = project
            .song
            .instruments
            .iter()
            .find(|i| i.id == "dh-keys")
            .expect("dh-keys");
        let adsr = read_adsr_state(keys);
        let decay_param = keys
            .parameters
            .iter()
            .find(|p| p.key == "decay")
            .expect("decay param");
        assert!((adsr.decay_s - decay_param.value).abs() < 1e-6);
        assert!((adsr.sustain - 0.28).abs() < 1e-4);
    }

    #[test]
    fn bipolar_depth_fraction_preserves_sign_and_bounds() {
        let project = load_deep_house();
        let bass = project
            .song
            .instruments
            .iter()
            .find(|i| i.id == "dh-bass")
            .expect("dh-bass");
        let param = bass.parameters.iter().find(|p| p.key == "cutoff");
        let pos = depth_fraction(500.0, 2000.0, param, Unit::Hertz);
        let neg = depth_fraction(-500.0, 2000.0, param, Unit::Hertz);
        assert!(pos > 0.0 && pos <= 1.0);
        assert!((-1.0..0.0).contains(&neg));
    }

    #[test]
    fn modulation_diff_ignores_source_locations() {
        let project = load_deep_house();
        let bass = project
            .song
            .instruments
            .iter()
            .find(|i| i.id == "dh-bass")
            .expect("dh-bass");
        let mut shifted_route = bass.modulation_routes[0].clone();
        shifted_route.location.line += 5;
        shifted_route.location.column += 3;
        shifted_route.sites.depth.start += 12;
        shifted_route.sites.depth.end += 12;
        assert!(
            !mod_route_changed(&bass.modulation_routes[0], &shifted_route),
            "shifting source offsets must not mark an unchanged route as edited"
        );

        shifted_route.depth = "999hz".to_owned();
        assert!(mod_route_changed(
            &bass.modulation_routes[0],
            &shifted_route
        ));

        let mut shifted_src = bass.modulation_sources[0].clone();
        shifted_src.location.line += 2;
        assert!(!mod_source_changed(
            &bass.modulation_sources[0],
            &shifted_src
        ));
    }
}
