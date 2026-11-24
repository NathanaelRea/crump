//! Song modules of the History time machine: tempo, patterns, a track's clips on the timeline,
//! automation lanes and anything else at Song level, as they were in one version.

use super::*;
use crate::model::Unit;

/// Draws `area` (Tempo, a Pattern, a track's Arrangement, or any Other Song-level collection
/// such as automations, modulations, locators or sidechains) as it is in `view.shown`.
/// `changes` are the measured changes in that area.
pub(super) fn draw(ui: &mut Ui, view: &SoundView<'_>, area: &Area, changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);
    match area {
        Area::Tempo => section(ui, Kind::Song, "Song", |ui| {
            dial(
                ui,
                "Tempo",
                song.tempo_bpm,
                60.0,
                180.0,
                Unit::Ratio,
                &|bpm| format!("{bpm:.1} BPM"),
                mark(
                    song.tempo_bpm,
                    prev_song.map(|s| s.tempo_bpm),
                    other_song.map(|s| s.tempo_bpm),
                ),
                view.other_word,
                theme::TEXT_PRIMARY,
            );
        }),
        Area::Pattern(id) => {
            let Some(pattern) = song.patterns.iter().find(|p| p.id == *id) else {
                return;
            };
            section(ui, Kind::Pattern, &pattern.id, |ui| {
                let width = ui.available_width().min(420.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 96.0), Sense::hover());
                pattern_thumb(ui.painter(), rect, pattern, Kind::Pattern.color());
                change_chips(ui, changes);
            });
        }
        Area::Arrangement(id) => {
            let name = song
                .tracks
                .iter()
                .find(|t| t.id == *id)
                .map_or(id.as_str(), |t| t.name.as_str());
            section(ui, Kind::Arrangement, name, |ui| change_chips(ui, changes));
        }
        _ => {
            let owner = changes.first().map_or("Project", |c| c.owner.as_str());
            section(ui, Kind::Other, owner, |ui| change_chips(ui, changes));
        }
    }
}
