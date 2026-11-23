//! Studio visual design system: Obsidian Slate surfaces, warm hardware accents,
//! deterministic track palette, perceptual audio scaling, and crisp vector icons.

use eframe::egui::{
    self, Color32, CornerRadius, Margin, Painter, Pos2, Rect, Stroke, Vec2,
    style::{ScrollStyle, WidgetVisuals, Widgets},
};

use crate::model::{NodeKind, ProcessorKind, Unit};

// ── 5-Tier Dark Studio Surface Elevation ────────────────────────────────────

/// Level 0 — Deep recessed well: meter troughs, LCD readouts, fader tracks, code gutter.
pub(super) const BG_WELL: Color32 = Color32::from_rgb(14, 16, 21);
/// Level 1 — Primary work canvas: timeline, piano roll, automation, signal graph.
pub(super) const BG_CANVAS: Color32 = Color32::from_rgb(21, 24, 30);
/// Subtle alternating row / lane surface.
pub(super) const BG_LANE_ALT: Color32 = Color32::from_rgb(25, 29, 36);
/// Level 2 — Structural studio chrome: transport bar, track headers, console chassis.
pub(super) const BG_SURFACE: Color32 = Color32::from_rgb(30, 34, 42);
/// Level 3 — Interactive cards, device modules, knob caps, pill groups.
pub(super) const BG_CARD: Color32 = Color32::from_rgb(39, 44, 54);
/// Hover surface for interactive controls.
pub(super) const BG_HOVER: Color32 = Color32::from_rgb(49, 56, 69);
/// Pressed / active recessed button fill.
pub(super) const BG_ACTIVE: Color32 = Color32::from_rgb(56, 65, 80);

// ── Borders & Gridlines ─────────────────────────────────────────────────────

/// Dark structural seam separating docked studio panels.
pub(super) const SEAM_DARK: Color32 = Color32::from_rgb(10, 12, 16);
/// Subtle 1px divider hairline.
pub(super) const BORDER_SUBTLE: Color32 = Color32::from_rgb(44, 50, 62);
/// Prominent card rim / active frame border.
pub(super) const BORDER_STRONG: Color32 = Color32::from_rgb(66, 75, 92);
/// Bar gridline on Timeline / Piano Roll / Automation.
pub(super) const GRID_BAR: Color32 = Color32::from_rgb(52, 59, 73);
/// Beat gridline.
pub(super) const GRID_BEAT: Color32 = Color32::from_rgb(34, 39, 49);
/// Sub-beat (1/16th) gridline.
pub(super) const GRID_SUB: Color32 = Color32::from_rgb(27, 31, 39);

// ── Typography Hierarchy ────────────────────────────────────────────────────

/// High-contrast crisp studio primary text (~13.5:1 on BG_SURFACE).
pub(super) const TEXT_PRIMARY: Color32 = Color32::from_rgb(232, 236, 244);
/// Secondary labels, units, inactive tabs.
pub(super) const TEXT_SECONDARY: Color32 = Color32::from_rgb(154, 164, 182);
/// Muted annotations, grid numbers, hints.
pub(super) const TEXT_MUTED: Color32 = Color32::from_rgb(118, 128, 147);

// ── Semantic Studio Accents ─────────────────────────────────────────────────

/// Signature warm studio amber-orange: knob arcs, active focus, loop, primary CTA.
pub(super) const ACCENT_AMBER: Color32 = Color32::from_rgb(255, 142, 48);
/// Precision cyan: modulation halos, automation curves, cue/selection highlights.
pub(super) const ACCENT_CYAN: Color32 = Color32::from_rgb(56, 198, 217);
/// Modulation / envelope / LFO violet.
pub(super) const ACCENT_VIOLET: Color32 = Color32::from_rgb(169, 124, 248);
/// The one selection outline used by every selectable card, strip, row and list item.
pub(super) const SELECTION: Color32 = ACCENT_AMBER;
/// Nominal signal / playing LED / Active Revision badge.
pub(super) const SIG_NOMINAL: Color32 = Color32::from_rgb(52, 199, 123);
/// Hot signal (-12..-3 dBFS) / Solo LED / GR / Candidate modified.
pub(super) const SIG_HOT: Color32 = Color32::from_rgb(240, 180, 41);
/// Clipping (>-3 dBFS) / Mute / Playhead / Error.
pub(super) const SIG_CLIP: Color32 = Color32::from_rgb(240, 82, 82);
/// Lit Mute: a neutral grey, since a muted channel is a choice, not a problem.
pub(super) const MUTE_LIT: Color32 = Color32::from_rgb(170, 180, 198);

// ── Deterministic 8-Color Track Palette ─────────────────────────────────────

const TRACK_PALETTE: [Color32; 8] = [
    Color32::from_rgb(236, 122, 160), // Rose (Drums); kept clear of the error red
    Color32::from_rgb(255, 158, 68),  // Amber (Bass)
    Color32::from_rgb(229, 192, 123), // Gold (Keys)
    Color32::from_rgb(86, 182, 139),  // Emerald (Lead)
    Color32::from_rgb(56, 198, 217),  // Cyan (Pad)
    Color32::from_rgb(91, 155, 248),  // Cobalt (Arp)
    Color32::from_rgb(169, 124, 248), // Violet (FX)
    Color32::from_rgb(232, 121, 184), // Magenta (Vox)
];

/// Deterministic studio track accent color for a track index.
pub(super) const fn track_color(index: usize) -> Color32 {
    TRACK_PALETTE[index % TRACK_PALETTE.len()]
}

/// Semantic accent color for a processor family.
/// One fixed color per drum voice, matching the drum pad bank order (Kick, Snare, Hat, Clap) so a
/// voice keeps its color in the grid, velocity lane and pads regardless of row order.
pub(super) const fn drum_voice_color(voice: crate::model::DrumVoice) -> Color32 {
    use crate::model::DrumVoice;
    track_color(match voice {
        DrumVoice::Kick => 0,
        DrumVoice::Snare => 1,
        DrumVoice::Hat => 2,
        DrumVoice::Clap => 3,
    })
}

pub(super) const fn processor_color(kind: ProcessorKind) -> Color32 {
    match kind {
        ProcessorKind::Eq => ACCENT_CYAN,
        ProcessorKind::Compressor | ProcessorKind::Gate | ProcessorKind::Limiter => ACCENT_AMBER,
        ProcessorKind::Delay | ProcessorKind::Reverb | ProcessorKind::ModulationDelay => {
            ACCENT_VIOLET
        }
        ProcessorKind::Saturation | ProcessorKind::Crusher | ProcessorKind::Stutter => {
            Color32::from_rgb(236, 122, 160)
        }
        ProcessorKind::Utility | ProcessorKind::Latency => SIG_NOMINAL,
    }
}

/// Human-friendly display title for a processor kind.
pub(super) const fn processor_title(kind: ProcessorKind) -> &'static str {
    match kind {
        ProcessorKind::Utility => "Utility",
        ProcessorKind::Eq => "Equalizer",
        ProcessorKind::Compressor => "Compressor",
        ProcessorKind::Gate => "Gate",
        ProcessorKind::Delay => "Delay",
        ProcessorKind::Reverb => "Reverb",
        ProcessorKind::Saturation => "Saturator",
        ProcessorKind::ModulationDelay => "Mod Delay",
        ProcessorKind::Crusher => "Bit Crusher",
        ProcessorKind::Stutter => "Stutter",
        ProcessorKind::Limiter => "Limiter",
        ProcessorKind::Latency => "Latency",
    }
}

/// Semantic accent color for a synthesis graph node kind.
pub(super) const fn node_color(kind: NodeKind) -> Color32 {
    match kind {
        NodeKind::Oscillator
        | NodeKind::Unison
        | NodeKind::AnalogOscillator
        | NodeKind::Wavetable
        | NodeKind::Noise
        | NodeKind::Exciter
        | NodeKind::ModalBank
        | NodeKind::SampleSource
        | NodeKind::Multisample
        | NodeKind::Granular
        | NodeKind::Spectral => ACCENT_AMBER,
        NodeKind::Lowpass
        | NodeKind::StereoFilter
        | NodeKind::DcBlock
        | NodeKind::LinearPhaseLowpass
        | NodeKind::Biquad
        | NodeKind::OnePole
        | NodeKind::BiquadChain => ACCENT_CYAN,
        NodeKind::Envelope | NodeKind::Lfo | NodeKind::Constant => ACCENT_VIOLET,
        NodeKind::Delay
        | NodeKind::Reverb
        | NodeKind::DelayLine
        | NodeKind::Saturation
        | NodeKind::Shaper => Color32::from_rgb(236, 122, 160),
        NodeKind::Output => SIG_NOMINAL,
        NodeKind::Mixer
        | NodeKind::StereoMixer
        | NodeKind::Sum
        | NodeKind::StereoSum
        | NodeKind::Amplifier
        | NodeKind::Pan
        | NodeKind::MonoToStereo
        | NodeKind::StereoToMono
        | NodeKind::Math
        | NodeKind::StereoMath
        | NodeKind::ToAudio
        | NodeKind::ToControl => Color32::from_rgb(86, 182, 139),
    }
}

/// Human-friendly category tag for a synthesis graph node kind.
pub(super) const fn node_kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Exciter => "Exciter",
        NodeKind::ModalBank => "Modal Bank",
        NodeKind::Oscillator => "Oscillator",
        NodeKind::Unison => "Unison Osc",
        NodeKind::Noise => "Noise",
        NodeKind::Envelope => "Envelope",
        NodeKind::Mixer => "Mixer",
        NodeKind::Lowpass => "Lowpass",
        NodeKind::StereoFilter => "Stereo Filter",
        NodeKind::Amplifier => "Amp / VCA",
        NodeKind::Saturation => "Saturator",
        NodeKind::Pan => "Panner",
        NodeKind::Delay => "Delay",
        NodeKind::Reverb => "Reverb",
        NodeKind::Output => "Output",
        NodeKind::Lfo => "LFO",
        NodeKind::MonoToStereo => "Mono -> Stereo",
        NodeKind::StereoToMono => "Stereo -> Mono",
        NodeKind::StereoMixer => "Stereo Mix",
        NodeKind::StereoSum => "Stereo Sum",
        NodeKind::Sum => "Sum",
        NodeKind::AnalogOscillator => "Analog Osc",
        NodeKind::Wavetable => "Wavetable",
        NodeKind::SampleSource => "Sampler",
        NodeKind::Multisample => "Multisample",
        NodeKind::Granular => "Granular",
        NodeKind::Spectral => "Spectral",
        NodeKind::DelayLine => "Delay Line",
        NodeKind::Math => "Math",
        NodeKind::StereoMath => "Stereo Math",
        NodeKind::Shaper => "Shaper",
        NodeKind::ToAudio => "To Audio",
        NodeKind::ToControl => "To Control",
        NodeKind::DcBlock => "DC Block",
        NodeKind::LinearPhaseLowpass => "LP Lowpass",
        NodeKind::Biquad => "Biquad",
        NodeKind::OnePole => "One-Pole",
        NodeKind::BiquadChain => "Biquad Chain",
        NodeKind::Constant => "Constant",
    }
}

/// A gain in decibels with one decimal, a space before the unit, and no negative zero.
pub(super) fn format_db(db: f64) -> String {
    let rounded = (db * 10.0).round() / 10.0;
    format!("{:.1} dB", if rounded == 0.0 { 0.0 } else { rounded })
}

/// A musical position as `bar.beat.sixteenth`, 1-based (`0qn` → `1.1.1`). Beats follow the
/// meter's denominator, so a 6/8 bar counts six eighth-note beats of two sixteenths each.
pub(super) fn format_bar_beat(quarters: f64, meter: crate::time::Meter) -> String {
    let beat_q = 4.0 / f64::from(meter.denominator.max(1));
    let bar_q = (beat_q * f64::from(meter.numerator.max(1))).max(0.25);
    let q = quarters.max(0.0) + 1e-6;
    let bar = (q / bar_q).floor();
    let in_bar = q - bar * bar_q;
    let beat = (in_bar / beat_q).floor();
    let sixteenth = ((in_bar - beat * beat_q) / 0.25).floor();
    format!(
        "{}.{}.{}",
        bar as u64 + 1,
        beat as u64 + 1,
        sixteenth as u64 + 1
    )
}

/// The bar-label stride for rulers: the smallest musical power of two (1, 2, 4, 8, 16, …) that is
/// at least `min_bars`, so labels land on phrase boundaries (1, 5, 9, 17 …) in every view.
pub(super) fn bar_label_interval(min_bars: f32) -> u32 {
    let mut stride = 1_u32;
    while (stride as f32) < min_bars && stride < 1024 {
        stride *= 2;
    }
    stride
}

/// Converts a quarter-note float position into an exact 1/32nd-snapped `MusicalTime`.
pub(super) fn quarters_to_musical_time(quarters: f64) -> crate::time::MusicalTime {
    let ticks = (quarters.max(0.0) * 32.0).round() as i64;
    crate::time::MusicalTime::new(ticks, 32).unwrap_or(crate::time::MusicalTime::ZERO)
}

/// Applies the Obsidian Slate studio DAW theme to the egui context.
pub(super) fn apply_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.interaction.selectable_labels = false;
        style.spacing.item_spacing = Vec2::new(6.0, 5.0);
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
        style.spacing.window_margin = Margin::same(10);
        style.spacing.combo_width = 148.0;
        style.spacing.scroll = ScrollStyle {
            bar_width: 8.0,
            // Floating bars overlay the content instead of taking width from it, so content
            // never reflows as a bar appears; a slim handle stays visible and widens on hover.
            floating: true,
            floating_width: 4.0,
            floating_allocated_width: 0.0,
            dormant_background_opacity: 0.0,
            dormant_handle_opacity: 0.6,
            // Slate handle from the widget fill (brightening on hover/drag) instead of the
            // near-white text color, so scrollbars never outshine the content they scroll.
            foreground_color: false,
            ..ScrollStyle::floating()
        };

        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT_PRIMARY);
        visuals.panel_fill = BG_CANVAS;
        visuals.window_fill = BG_SURFACE;
        visuals.window_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
        visuals.window_corner_radius = CornerRadius::same(6);
        visuals.faint_bg_color = BG_LANE_ALT;
        visuals.extreme_bg_color = BG_WELL;
        visuals.code_bg_color = BG_WELL;
        visuals.hyperlink_color = ACCENT_CYAN;
        visuals.warn_fg_color = SIG_HOT;
        visuals.error_fg_color = SIG_CLIP;
        visuals.selection.bg_fill = Color32::from_rgb(28, 66, 80);
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT_CYAN);

        let r = CornerRadius::same(4);
        visuals.widgets = Widgets {
            noninteractive: WidgetVisuals {
                bg_fill: BG_SURFACE,
                weak_bg_fill: BG_SURFACE,
                bg_stroke: Stroke::new(1.0_f32, BORDER_SUBTLE),
                corner_radius: r,
                fg_stroke: Stroke::new(1.0_f32, TEXT_SECONDARY),
                expansion: 0.0,
            },
            inactive: WidgetVisuals {
                bg_fill: BG_CARD,
                weak_bg_fill: BG_CARD,
                bg_stroke: Stroke::new(1.0_f32, BORDER_SUBTLE),
                corner_radius: r,
                fg_stroke: Stroke::new(1.0_f32, TEXT_PRIMARY),
                expansion: 0.0,
            },
            hovered: WidgetVisuals {
                bg_fill: BG_HOVER,
                weak_bg_fill: BG_HOVER,
                bg_stroke: Stroke::new(1.0_f32, BORDER_STRONG),
                corner_radius: r,
                fg_stroke: Stroke::new(1.0_f32, TEXT_PRIMARY),
                expansion: 0.0,
            },
            active: WidgetVisuals {
                bg_fill: BG_ACTIVE,
                weak_bg_fill: BG_ACTIVE,
                bg_stroke: Stroke::new(1.0_f32, ACCENT_AMBER),
                corner_radius: r,
                fg_stroke: Stroke::new(1.0_f32, TEXT_PRIMARY),
                expansion: 0.0,
            },
            open: WidgetVisuals {
                bg_fill: BG_HOVER,
                weak_bg_fill: BG_HOVER,
                bg_stroke: Stroke::new(1.0_f32, ACCENT_CYAN),
                corner_radius: r,
                fg_stroke: Stroke::new(1.0_f32, TEXT_PRIMARY),
                expansion: 0.0,
            },
        };
        style.visuals = visuals;
    });
}

/// Blends `color` with alpha `0.0..=1.0` cleanly without premultiplication artifacts.
pub(super) fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a)
}

// ── Perceptual Audio Scaling & Formatting ───────────────────────────────────

/// Converts linear amplitude (`0.0..`) to decibels full scale, floored at `-60.0 dBFS`.
pub(super) fn amp_to_dbfs(amp: f32) -> f32 {
    if amp <= 1e-5 {
        -60.0
    } else {
        (20.0 * amp.log10()).clamp(-60.0, 6.0)
    }
}

/// Maps a dBFS reading (`-60.0..=+6.0`) onto normalized meter fill `0.0..=1.0`
/// with expanded resolution in the musical `-24..+6 dB` zone.
pub(super) fn dbfs_to_norm(dbfs: f32) -> f32 {
    let clamped = dbfs.clamp(-60.0, 6.0);
    // Piece-wise studio meter ballistics: -60..-24 gets 30%, -24..0 gets 58%, 0..+6 gets 12%
    if clamped <= -24.0 {
        ((clamped + 60.0) / 36.0) * 0.30
    } else if clamped <= 0.0 {
        0.30 + ((clamped + 24.0) / 24.0) * 0.58
    } else {
        0.88 + (clamped / 6.0) * 0.12
    }
}

/// Formats a linear peak amplitude as a compact dBFS string (`-4.2 dB` or `-inf`).
pub(super) fn format_dbfs(amp: f32) -> String {
    if amp <= 1e-4 {
        "-inf".to_owned()
    } else {
        let db = (20.0 * amp.log10()).clamp(-60.0, 12.0);
        if db >= 0.0 {
            format!("+{db:.1}")
        } else {
            format!("{db:.1}")
        }
    }
}

/// Formats a pan value (`-1.0..=1.0`) as studio hardware notation (`L45`, `C`, `R22`).
pub(super) fn format_pan(pan: f64) -> String {
    let pct = (pan.clamp(-1.0, 1.0) * 100.0).round() as i32;
    if pct.abs() <= 1 {
        "C".to_owned()
    } else if pct < 0 {
        format!("L{}", -pct)
    } else {
        format!("R{pct}")
    }
}

/// Maps a physical value in `[min, max]` to normalized `[0, 1]` using perceptual scaling for Hz/Seconds.
pub(super) fn value_to_norm(value: f64, min: f64, max: f64, unit: Unit) -> f32 {
    if (max - min).abs() <= f64::EPSILON {
        return 0.5;
    }
    let v = value.clamp(min, max);
    match unit {
        Unit::Hertz if min > 0.0 && max / min > 4.0 => {
            ((v / min).ln() / (max / min).ln()).clamp(0.0, 1.0) as f32
        }
        Unit::Seconds if max > min && max > 0.05 => {
            let floor = min.max(0.001);
            let shifted = v.max(floor);
            ((shifted / floor).ln() / (max / floor).ln()).clamp(0.0, 1.0) as f32
        }
        _ => ((v - min) / (max - min)).clamp(0.0, 1.0) as f32,
    }
}

/// Maps normalized `[0, 1]` back to a physical value in `[min, max]` using perceptual scaling.
pub(super) fn norm_to_value(norm: f32, min: f64, max: f64, unit: Unit) -> f64 {
    let n = f64::from(norm.clamp(0.0, 1.0));
    match unit {
        Unit::Hertz if min > 0.0 && max / min > 4.0 => min * (max / min).powf(n),
        Unit::Seconds if max > min && max > 0.05 => {
            if n <= 0.001 && min == 0.0 {
                0.0
            } else {
                let floor = min.max(0.001);
                floor * (max / floor).powf(n)
            }
        }
        _ => min + n * (max - min),
    }
}

/// Formats a physical value concisely for studio knob/fader readouts.
pub(super) fn format_compact_value(value: f64, unit: Unit) -> String {
    match unit {
        Unit::Hertz => {
            if value >= 1000.0 {
                format!("{:.2} kHz", value / 1000.0)
            } else if value >= 100.0 {
                format!("{value:.0} Hz")
            } else {
                format!("{value:.1} Hz")
            }
        }
        Unit::Seconds => {
            if value < 1.0 {
                format!("{:.0} ms", value * 1000.0)
            } else {
                format!("{value:.2} s")
            }
        }
        Unit::Decibels => {
            if value <= -59.5 {
                "-inf dB".to_owned()
            } else if value > 0.05 {
                format!("+{value:.1} dB")
            } else {
                format!("{value:.1} dB")
            }
        }
        Unit::Pan => format_pan(value),
        Unit::Ratio => {
            if (0.0..=1.0).contains(&value) {
                format!("{:.0}%", value * 100.0)
            } else {
                format!("{value:.2}")
            }
        }
        Unit::Semitones => format!("{value:+.1} st"),
        Unit::Bits => format!("{value:.0} bit"),
        Unit::Boolean => {
            if value >= 0.5 {
                "ON".to_owned()
            } else {
                "OFF".to_owned()
            }
        }
    }
}

/// Formats a physical value into canonical Crump literal syntax for `SetProcessorSetting` / Automation.
pub(super) fn format_crump_literal(value: f64, unit: Unit) -> String {
    unit.format_value(value)
}

// ── Crisp Vector Studio Icons (Zero Missing Glyphs) ─────────────────────────

/// Paints a crisp Play triangle centered in `rect`.
pub(super) fn paint_play_icon(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.34;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(c.x - r * 0.65, c.y - r),
            Pos2::new(c.x + r * 0.95, c.y),
            Pos2::new(c.x - r * 0.65, c.y + r),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Paints two crisp Pause bars centered in `rect`.
pub(super) fn paint_pause_icon(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.34;
    let w = (r * 0.52).max(2.0);
    let gap = (r * 0.38).max(1.5);
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(c.x - gap - w, c.y - r),
            Pos2::new(c.x - gap, c.y + r),
        ),
        1.0,
        color,
    );
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(c.x + gap, c.y - r),
            Pos2::new(c.x + gap + w, c.y + r),
        ),
        1.0,
        color,
    );
}

/// Paints a crisp Stop square centered in `rect`.
pub(super) fn paint_stop_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.width().min(rect.height()) * 0.30;
    painter.rect_filled(
        Rect::from_center_size(rect.center(), Vec2::splat(r * 2.0)),
        1.5,
        color,
    );
}

/// Paints a Return-to-Zero (`|<`) icon centered in `rect`.
pub(super) fn paint_rtz_icon(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.32;
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(c.x - r * 0.9, c.y - r),
            Pos2::new(c.x - r * 0.48, c.y + r),
        ),
        0.5,
        color,
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(c.x + r * 0.9, c.y - r),
            Pos2::new(c.x - r * 0.35, c.y),
            Pos2::new(c.x + r * 0.9, c.y + r),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Paints a studio Loop cycle icon centered in `rect`.
pub(super) fn paint_loop_icon(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let rx = rect.width() * 0.32;
    let ry = rect.height() * 0.24;
    let stroke = Stroke::new(1.6_f32, color);
    // Top bar + right bend
    painter.line_segment(
        [
            Pos2::new(c.x - rx * 0.8, c.y - ry),
            Pos2::new(c.x + rx * 0.65, c.y - ry),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(c.x + rx * 0.65, c.y - ry),
            Pos2::new(c.x + rx * 0.65, c.y - ry * 0.1),
        ],
        stroke,
    );
    // Arrowhead top-right
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(c.x + rx * 0.35, c.y - ry - 3.0),
            Pos2::new(c.x + rx * 0.95, c.y - ry),
            Pos2::new(c.x + rx * 0.35, c.y - ry + 3.0),
        ],
        color,
        Stroke::NONE,
    ));
    // Bottom bar + left bend
    painter.line_segment(
        [
            Pos2::new(c.x + rx * 0.8, c.y + ry),
            Pos2::new(c.x - rx * 0.65, c.y + ry),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(c.x - rx * 0.65, c.y + ry),
            Pos2::new(c.x - rx * 0.65, c.y + ry * 0.1),
        ],
        stroke,
    );
    // Arrowhead bottom-left
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(c.x - rx * 0.35, c.y + ry - 3.0),
            Pos2::new(c.x - rx * 0.95, c.y + ry),
            Pos2::new(c.x - rx * 0.35, c.y + ry + 3.0),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Paints a circular hardware Power/Bypass LED icon centered in `rect`.
pub(super) fn paint_power_icon(painter: &Painter, rect: Rect, color: Color32, lit: bool) {
    let c = rect.center();
    let r = (rect.width().min(rect.height()) * 0.34).max(4.0);
    if lit {
        painter.circle_filled(c, r + 2.0, with_alpha(color, 0.22));
    }
    let stroke = Stroke::new(1.6_f32, color);
    // Open arc from -55 deg to +235 deg (gap at top 12 o'clock)
    let segments = 16;
    let start = -std::f32::consts::FRAC_PI_2 + 0.52;
    let end = std::f32::consts::TAU - std::f32::consts::FRAC_PI_2 - 0.52;
    let points = (0..=segments)
        .map(|i| {
            let t = i as f32 / segments as f32;
            let a = start + t * (end - start);
            Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect::<Vec<_>>();
    painter.add(egui::Shape::line(points, stroke));
    painter.line_segment(
        [
            Pos2::new(c.x, c.y - r * 1.15),
            Pos2::new(c.x, c.y - r * 0.1),
        ],
        stroke,
    );
}
