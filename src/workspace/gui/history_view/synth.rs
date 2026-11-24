//! Synth module of the History time machine: one instrument as it was in one version.

use super::*;
use crate::model::{Instrument, Song};

fn old_inst<'a>(song: Option<&'a Song>, id: &str) -> Option<&'a Instrument> {
    song.and_then(|s| s.instruments.iter().find(|i| i.id == id))
}

/// Draws instrument `id` as it is in `view.shown`, marking what this edit moved (against
/// `view.previous`) and what differs from the other side of Then / Now (`view.other`).
/// `changes` are the measured changes inside this instrument.
pub(super) fn draw(ui: &mut Ui, view: &SoundView<'_>, id: &str, changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);
    let word = view.other_word;
    let Some(inst) = song.instruments.iter().find(|i| i.id == id) else {
        return;
    };
    let param_of = |s: Option<&Song>, key: &str| {
        old_inst(s, id)
            .and_then(|i| i.parameters.iter().find(|p| p.key == key))
            .map(|p| p.value)
    };
    let name = match song.tracks.iter().find(|t| t.instrument == inst.id) {
        Some(track) => format!("{} on {}", inst.name, track.name),
        None => inst.name.clone(),
    };
    section(ui, Kind::Instrument, &name, |ui| {
        ui.horizontal_wrapped(|ui| {
            for param in &inst.parameters {
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
                        param_of(prev_song, &param.key),
                        param_of(other_song, &param.key),
                    ),
                    word,
                    Kind::Instrument.color(),
                );
            }
        });
        let fx = changes
            .iter()
            .filter(|c| c.path.first().is_some_and(|p| p == "post_processors"))
            .copied()
            .collect::<Vec<_>>();
        fx_rack::chain(
            ui,
            view,
            &inst.post_processors,
            old_inst(prev_song, id).map(|i| i.post_processors.as_slice()),
            old_inst(other_song, id).map(|i| i.post_processors.as_slice()),
            &fx,
        );
        // Inside the synth: oscillator, filter and envelope settings, routes, and so on.
        let inner = changes
            .iter()
            .filter(|c| {
                c.path
                    .first()
                    .is_none_or(|p| p != "parameters" && p != "post_processors")
            })
            .copied()
            .collect::<Vec<_>>();
        change_chips(ui, &inner);
    });
}
