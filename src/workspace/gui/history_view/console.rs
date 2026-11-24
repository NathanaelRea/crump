//! Console module of the History time machine: the mix channels (tracks, groups, returns and
//! the Master) as they were in one version, each with its FX rack.

use super::*;
use crate::model::Unit;

/// Draws the console for `areas` (every touched Track, Group, Return and Master) as they are in
/// `view.shown`. `changes` are the measured changes in those areas.
pub(super) fn draw(ui: &mut Ui, view: &SoundView<'_>, areas: &[Area], changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);
    let word = view.other_word;
    for area in areas {
        let in_area = changes
            .iter()
            .filter(|c| c.area == *area)
            .copied()
            .collect::<Vec<_>>();
        let fx = in_area
            .iter()
            .filter(|c| c.path.get(1).is_some_and(|p| p == "inserts"))
            .copied()
            .collect::<Vec<_>>();
        if *area == Area::Master {
            let Some(master) = &song.master else {
                continue;
            };
            let prev = prev_song.and_then(|s| s.master.as_ref());
            let other = other_song.and_then(|s| s.master.as_ref());
            section(ui, Kind::Mix, "Master", |ui| {
                fader(
                    ui,
                    master.level_db,
                    mark(
                        master.level_db,
                        prev.map(|m| m.level_db),
                        other.map(|m| m.level_db),
                    ),
                    word,
                );
                fx_rack::chain(
                    ui,
                    view,
                    &master.inserts,
                    prev.map(|m| m.inserts.as_slice()),
                    other.map(|m| m.inserts.as_slice()),
                    &fx,
                );
            });
            ui.add_space(12.0);
            continue;
        }
        let Some((name, channel)) = channel_strip(song, area) else {
            continue;
        };
        let prev = prev_song
            .and_then(|s| channel_strip(s, area))
            .map(|(_, c)| c);
        let other = other_song
            .and_then(|s| channel_strip(s, area))
            .map(|(_, c)| c);
        section(ui, Kind::Mix, &name, |ui| {
            ui.horizontal_top(|ui| {
                fader(
                    ui,
                    channel.level_db,
                    mark(
                        channel.level_db,
                        prev.map(|c| c.level_db),
                        other.map(|c| c.level_db),
                    ),
                    word,
                );
                // A stereo panner places each side, so it gets a dial per side.
                for (side, (label, pan)) in pan_dials(&channel.panner).into_iter().enumerate() {
                    let at = |c: &crate::model::ChannelStrip| {
                        pan_dials(&c.panner).get(side).map(|(_, v)| *v)
                    };
                    dial(
                        ui,
                        label,
                        pan,
                        -1.0,
                        1.0,
                        Unit::Pan,
                        &|v| theme::format_compact_value(v, Unit::Pan),
                        mark(pan, prev.and_then(at), other.and_then(at)),
                        word,
                        Kind::Mix.color(),
                    );
                }
            });
            fx_rack::chain(
                ui,
                view,
                &channel.inserts,
                prev.map(|c| c.inserts.as_slice()),
                other.map(|c| c.inserts.as_slice()),
                &fx,
            );
            // Sends, mute, routing and anything else on this channel.
            let inner = in_area
                .iter()
                .filter(|c| {
                    !matches!(c.label.as_str(), "Level" | "Pan" | "Pan left" | "Pan right")
                        && c.path.get(1).is_none_or(|p| p != "inserts")
                })
                .copied()
                .collect::<Vec<_>>();
            change_chips(ui, &inner);
        });
        ui.add_space(12.0);
    }
}
