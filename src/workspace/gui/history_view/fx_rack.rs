//! FX rack module of the History time machine: an effect chain (a channel's inserts or an
//! instrument's post processors) as it was in one version.

use super::*;
use crate::model::Processor;

/// Draws `processors` as they are in `view.shown`. `previous` is the same chain one version
/// earlier (what this edit moved), `other` the chain on the other side of Then / Now, and
/// `changes` the measured changes that landed in this chain's owner.
pub(super) fn chain(
    ui: &mut Ui,
    view: &SoundView<'_>,
    processors: &[Processor],
    previous: Option<&[Processor]>,
    other: Option<&[Processor]>,
    _changes: &[&ValueChange],
) {
    for processor in processors {
        effect_settings(
            ui,
            processor,
            previous.and_then(|c| c.iter().find(|p| p.id == processor.id)),
            other.and_then(|c| c.iter().find(|p| p.id == processor.id)),
            view.other_word,
        );
    }
}
