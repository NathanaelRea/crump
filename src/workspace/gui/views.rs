//! Dispatcher mapping each [`View`] to its studio workspace module.

use super::*;

pub(super) fn view(ui: &mut Ui, cx: &mut Cx<'_>) {
    match cx.app.view {
        View::Timeline => arrangement::timeline(ui, cx),
        View::Mixer => mixer::mixer(ui, cx),
        View::Processing => processing::processing(ui, cx),
        View::EventGrid => sequencer::event_grid(ui, cx),
        View::Faceplate => sound_design::faceplate(ui, cx),
        View::Instrument => sound_design::instrument_sections(ui, cx),
        View::Parameters => sound_design::parameters(ui, cx),
        View::Automation => sequencer::automation(ui, cx),
        View::Graph => sound_design::graph(ui, cx),
        View::Source => utility_views::source(ui, cx),
        View::Diagnostics => utility_views::diagnostics(ui, cx),
        View::History => utility_views::history(ui, cx),
        View::Help => utility_views::help(ui, cx),
    }
}
