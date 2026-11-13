use std::f64::consts::PI;

use crate::engine::{control::mix_modulation_identity, sample::CompiledSampleSource};

use super::Scalar;

pub(super) const MAX_SAMPLE_SLICES: usize = crate::validate::MAX_SAMPLE_SLICES;
pub(super) const MAX_MULTISAMPLE_ZONES: usize = crate::validate::MAX_MULTISAMPLE_ZONES;
const SOURCE_RELEASE_FRAMES: u32 = 64;

pub(super) const fn sample_source_state_bytes() -> u64 {
    std::mem::size_of::<SampleSourceState>() as u64
}

pub(super) const fn multisample_state_bytes(zone_count: usize) -> u64 {
    (std::mem::size_of::<MultisampleState>() as u64).saturating_add(
        (zone_count as u64).saturating_mul(std::mem::size_of::<SampleCursor>() as u64),
    )
}

pub(super) const fn granular_state_bytes(max_grains: usize) -> u64 {
    (std::mem::size_of::<GranularState>() as u64)
        .saturating_add((max_grains as u64).saturating_mul(std::mem::size_of::<Grain>() as u64))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SampleLoopMode {
    Off,
    Forward,
    PingPong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SampleDirection {
    Forward,
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SampleGateMode {
    Gated,
    OneShot,
}

#[derive(Debug, Clone)]
pub(super) struct SampleSourceConfig {
    pub(super) source: CompiledSampleSource,
    pub(super) slice_id: Option<String>,
    pub(super) slice_start: f64,
    pub(super) slice_end: f64,
    pub(super) start: Scalar,
    pub(super) end: Scalar,
    pub(super) loop_mode: SampleLoopMode,
    pub(super) loop_start: Scalar,
    pub(super) loop_end: Scalar,
    pub(super) loop_crossfade: f64,
    pub(super) direction: SampleDirection,
    pub(super) gate_mode: SampleGateMode,
    pub(super) root_key: u8,
    pub(super) tune: Scalar,
    pub(super) rate: Scalar,
    pub(super) velocity_amount: Scalar,
    pub(super) level: Scalar,
    pub(super) pan: Scalar,
}

#[derive(Debug, Clone)]
pub(super) struct SampleSourceState {
    cursor: SampleCursor,
}

impl SampleSourceState {
    pub(super) const fn new() -> Self {
        Self {
            cursor: SampleCursor::new(),
        }
    }

    pub(super) fn retrigger(&mut self) {
        self.cursor = SampleCursor::new();
    }
}

impl SampleSourceConfig {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &self,
        state: &mut SampleSourceState,
        pitch: f32,
        gate: f32,
        gain: f32,
        velocity: f32,
        has_gain: bool,
        has_velocity: bool,
        controls: &[f64],
    ) -> [f32; 2] {
        if !state.cursor.initialized {
            let slice_span = (self.slice_end - self.slice_start).max(0.0);
            let start = self.slice_start + self.start.value(controls).clamp(0.0, 1.0) * slice_span;
            let end = self.slice_start + self.end.value(controls).clamp(0.0, 1.0) * slice_span;
            let region_start = start.floor().clamp(self.slice_start, self.slice_end);
            let region_end = end.ceil().clamp(self.slice_start, self.slice_end);
            let region_span = (region_end - region_start).max(0.0);
            let loop_start =
                region_start + self.loop_start.value(controls).clamp(0.0, 1.0) * region_span;
            let loop_end =
                region_start + self.loop_end.value(controls).clamp(0.0, 1.0) * region_span;
            state.cursor.activate(
                region_start,
                region_end,
                loop_start,
                loop_end,
                self.direction,
                gate > 0.0,
            );
        }
        let root_hz = midi_frequency(self.root_key);
        let pitch_ratio = f64::from(pitch.max(1.0)) / root_hz;
        let tune_ratio = 2.0_f64.powf(self.tune.value(controls) / 12.0);
        let rate = self.rate.value(controls).clamp(0.125, 8.0);
        let step = (pitch_ratio * tune_ratio * rate).clamp(1.0 / 64.0, 64.0);
        let mut output = state.cursor.process(
            &self.source,
            step,
            self.loop_mode,
            self.loop_crossfade,
            gate > 0.0,
            self.gate_mode,
        );
        let velocity = if has_velocity {
            f64::from(velocity).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let velocity_amount = self.velocity_amount.value(controls).clamp(0.0, 1.0);
        let amplitude = (if has_gain { f64::from(gain) } else { 1.0 })
            * ((1.0 - velocity_amount) + velocity_amount * velocity)
            * self.level.value(controls);
        apply_level_and_pan(&mut output, amplitude, self.pan.value(controls));
        output
    }

    pub(super) fn transition_compatible(&self, replacement: &Self) -> bool {
        self.source.cache_identity() == replacement.source.cache_identity()
            && self.slice_id == replacement.slice_id
            && self.slice_start.to_bits() == replacement.slice_start.to_bits()
            && self.slice_end.to_bits() == replacement.slice_end.to_bits()
            && self.loop_mode == replacement.loop_mode
            && self.loop_crossfade.to_bits() == replacement.loop_crossfade.to_bits()
            && self.direction == replacement.direction
            && self.gate_mode == replacement.gate_mode
            && self.root_key == replacement.root_key
    }

    pub(super) fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(self.source.asset_id().len() as u64)
            .saturating_add(self.source.source_sha256().len() as u64)
            .saturating_add(
                self.slice_id
                    .as_ref()
                    .map_or(0, |slice| slice.capacity() as u64),
            )
    }
}

#[derive(Debug, Clone)]
pub(super) struct MultisampleZone {
    pub(super) id: String,
    pub(super) source: CompiledSampleSource,
    pub(super) low_key: u8,
    pub(super) high_key: u8,
    pub(super) low_velocity: f64,
    pub(super) high_velocity: f64,
    pub(super) root_key: u8,
    pub(super) layer: String,
    pub(super) articulation: String,
    pub(super) round_robin: String,
    pub(super) round_robin_index: u16,
    pub(super) region_start: f64,
    pub(super) region_end: f64,
    /// Project-rate `(start, end, crossfade)` frames repeated while the note is held.
    pub(super) loop_region: Option<(f64, f64, f64)>,
    pub(super) gain: f64,
    pub(super) pan: f64,
}

impl MultisampleZone {
    fn matches(&self, key: u8, velocity: f64) -> bool {
        (self.low_key..=self.high_key).contains(&key)
            && velocity >= self.low_velocity
            && velocity <= self.high_velocity
            && self.articulation == "default"
    }

    fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(self.id.capacity() as u64)
            .saturating_add(self.source.asset_id().len() as u64)
            .saturating_add(self.source.source_sha256().len() as u64)
            .saturating_add(self.layer.capacity() as u64)
            .saturating_add(self.articulation.capacity() as u64)
            .saturating_add(self.round_robin.capacity() as u64)
    }
}

#[derive(Debug, Clone)]
pub(super) struct MultisampleRoundRobinGroup {
    pub(super) identity: u64,
    pub(super) zone_indexes: Vec<u16>,
}

#[derive(Debug, Clone)]
pub(super) struct MultisampleConfig {
    pub(super) zones: Vec<MultisampleZone>,
    pub(super) groups: Vec<MultisampleRoundRobinGroup>,
    pub(super) gate_mode: SampleGateMode,
    pub(super) tune: Scalar,
    pub(super) rate: Scalar,
    pub(super) velocity_amount: Scalar,
    pub(super) level: Scalar,
    pub(super) pan: Scalar,
}

#[derive(Debug, Clone)]
pub(super) struct MultisampleState {
    cursors: Vec<SampleCursor>,
    seed: u64,
    needs_selection: bool,
}

impl MultisampleState {
    pub(super) fn new(zone_count: usize, seed: u64) -> Self {
        Self {
            cursors: vec![SampleCursor::new(); zone_count],
            seed,
            needs_selection: true,
        }
    }

    pub(super) fn retrigger(&mut self, seed: u64) {
        self.seed = seed;
        self.needs_selection = true;
        self.cursors.fill(SampleCursor::new());
    }

    #[cfg(test)]
    pub(super) fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64).saturating_add(
            (self.cursors.capacity() as u64)
                .saturating_mul(std::mem::size_of::<SampleCursor>() as u64),
        )
    }
}

impl MultisampleConfig {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &self,
        state: &mut MultisampleState,
        pitch: f32,
        gate: f32,
        gain: f32,
        velocity: f32,
        has_gain: bool,
        controls: &[f64],
    ) -> [f32; 2] {
        let key = frequency_to_midi(f64::from(pitch.max(1.0)));
        let velocity = f64::from(velocity).clamp(0.0, 1.0);
        if state.needs_selection {
            state.cursors.fill(SampleCursor::new());
            for group in &self.groups {
                let matching = group
                    .zone_indexes
                    .iter()
                    .filter(|index| self.zones[usize::from(**index)].matches(key, velocity))
                    .count();
                if matching == 0 {
                    continue;
                }
                let selected = (mix_modulation_identity(state.seed, group.identity)
                    % matching as u64) as usize;
                let Some(zone_index) = group
                    .zone_indexes
                    .iter()
                    .filter(|index| self.zones[usize::from(**index)].matches(key, velocity))
                    .nth(selected)
                    .copied()
                else {
                    continue;
                };
                let zone = &self.zones[usize::from(zone_index)];
                let (loop_start, loop_end, _) =
                    zone.loop_region
                        .unwrap_or((zone.region_start, zone.region_end, 0.0));
                state.cursors[usize::from(zone_index)].activate(
                    zone.region_start,
                    zone.region_end,
                    loop_start,
                    loop_end,
                    SampleDirection::Forward,
                    gate > 0.0,
                );
            }
            state.needs_selection = false;
        }

        let tune_ratio = 2.0_f64.powf(self.tune.value(controls) / 12.0);
        let rate = self.rate.value(controls).clamp(0.125, 8.0);
        let mut output = [0.0_f32; 2];
        for (zone, cursor) in self.zones.iter().zip(&mut state.cursors) {
            if !cursor.active {
                continue;
            }
            let pitch_ratio = f64::from(pitch.max(1.0)) / midi_frequency(zone.root_key);
            let step = (pitch_ratio * tune_ratio * rate).clamp(1.0 / 64.0, 64.0);
            // A zone loop repeats only while the note is held. Afterwards a one-shot
            // zone plays on through the rest of its region (its natural release),
            // while a gated zone takes the fixed source release.
            let (loop_mode, crossfade) = zone
                .loop_region
                .map_or((SampleLoopMode::Off, 0.0), |(_, _, crossfade)| {
                    (SampleLoopMode::Forward, crossfade)
                });
            let mut frame = cursor.process(
                &zone.source,
                step,
                loop_mode,
                crossfade,
                gate > 0.0,
                self.gate_mode,
            );
            apply_level_and_pan(&mut frame, zone.gain, zone.pan);
            output[0] += frame[0];
            output[1] += frame[1];
        }
        let velocity_amount = self.velocity_amount.value(controls).clamp(0.0, 1.0);
        let amplitude = (if has_gain { f64::from(gain) } else { 1.0 })
            * ((1.0 - velocity_amount) + velocity_amount * velocity)
            * self.level.value(controls);
        apply_level_and_pan(&mut output, amplitude, self.pan.value(controls));
        output
    }

    pub(super) fn transition_compatible(&self, replacement: &Self) -> bool {
        if self.gate_mode != replacement.gate_mode
            || self.zones.len() != replacement.zones.len()
            || self.groups.len() != replacement.groups.len()
        {
            return false;
        }
        self.zones
            .iter()
            .zip(&replacement.zones)
            .all(|(left, right)| {
                left.id == right.id
                    && left.source.cache_identity() == right.source.cache_identity()
                    && left.low_key == right.low_key
                    && left.high_key == right.high_key
                    && left.low_velocity.to_bits() == right.low_velocity.to_bits()
                    && left.high_velocity.to_bits() == right.high_velocity.to_bits()
                    && left.root_key == right.root_key
                    && left.layer == right.layer
                    && left.articulation == right.articulation
                    && left.round_robin == right.round_robin
                    && left.round_robin_index == right.round_robin_index
                    && left.region_start.to_bits() == right.region_start.to_bits()
                    && left.region_end.to_bits() == right.region_end.to_bits()
                    && left
                        .loop_region
                        .map(|(a, b, c)| [a.to_bits(), b.to_bits(), c.to_bits()])
                        == right
                            .loop_region
                            .map(|(a, b, c)| [a.to_bits(), b.to_bits(), c.to_bits()])
                    && left.gain.to_bits() == right.gain.to_bits()
                    && left.pan.to_bits() == right.pan.to_bits()
            })
            && self
                .groups
                .iter()
                .zip(&replacement.groups)
                .all(|(left, right)| {
                    left.identity == right.identity && left.zone_indexes == right.zone_indexes
                })
    }

    pub(super) fn allocation_bytes(&self) -> u64 {
        let zones = self
            .zones
            .iter()
            .map(MultisampleZone::allocation_bytes)
            .fold(0_u64, u64::saturating_add);
        let groups = self
            .groups
            .iter()
            .map(|group| {
                (std::mem::size_of::<MultisampleRoundRobinGroup>() as u64).saturating_add(
                    (group.zone_indexes.capacity() as u64)
                        .saturating_mul(std::mem::size_of::<u16>() as u64),
                )
            })
            .fold(0_u64, u64::saturating_add);
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(zones)
            .saturating_add(groups)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GrainWindow {
    Hann,
    Triangle,
    Tukey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GrainEdge {
    Wrap,
    Reflect,
    Clamp,
}

#[derive(Debug, Clone)]
pub(super) struct GranularConfig {
    pub(super) source: CompiledSampleSource,
    pub(super) max_grains: usize,
    pub(super) position: Scalar,
    pub(super) scan: Scalar,
    pub(super) size: Scalar,
    pub(super) density: Scalar,
    pub(super) root_key: u8,
    pub(super) tune: Scalar,
    pub(super) spray: Scalar,
    pub(super) window: GrainWindow,
    pub(super) edge: GrainEdge,
    pub(super) direction_probability: Scalar,
    pub(super) stereo_spread: Scalar,
    pub(super) freeze: Scalar,
    pub(super) level: Scalar,
    pub(super) seed: u64,
    pub(super) node_identity: u64,
}

#[derive(Debug, Clone)]
pub(super) struct GranularState {
    grains: Vec<Grain>,
    density_phase: f64,
    scan_phase: f64,
    spawn_index: u64,
    runtime_seed: u64,
}

impl GranularState {
    pub(super) fn new(max_grains: usize, runtime_seed: u64) -> Self {
        Self {
            grains: vec![Grain::default(); max_grains],
            density_phase: 0.0,
            scan_phase: 0.0,
            spawn_index: 0,
            runtime_seed,
        }
    }

    pub(super) fn retrigger(&mut self, runtime_seed: u64) {
        self.grains.fill(Grain::default());
        self.density_phase = 0.0;
        self.scan_phase = 0.0;
        self.spawn_index = 0;
        self.runtime_seed = runtime_seed;
    }

    #[cfg(test)]
    pub(super) fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64).saturating_add(
            (self.grains.capacity() as u64).saturating_mul(std::mem::size_of::<Grain>() as u64),
        )
    }
}

impl GranularConfig {
    pub(super) fn process(
        &self,
        state: &mut GranularState,
        pitch: f32,
        gate: f32,
        controls: &[f64],
        sample_rate: f64,
    ) -> [f32; 2] {
        let freeze = self.freeze.value(controls) >= 0.5;
        if !freeze {
            state.scan_phase = (state.scan_phase
                + self.scan.value(controls).clamp(-2.0, 2.0) / sample_rate)
                .rem_euclid(1.0);
        }
        if gate > 0.0 {
            state.density_phase += self.density.value(controls).clamp(0.1, 1_000.0) / sample_rate;
            if state.density_phase >= 1.0 {
                state.density_phase -= 1.0;
                self.spawn(state, pitch, controls, sample_rate);
            }
        }

        let mut output = [0.0_f64; 2];
        let mut active = 0_u32;
        for grain in &mut state.grains {
            if !grain.active {
                continue;
            }
            active += 1;
            let frame = interpolated_frame(
                &self.source,
                grain.position,
                0.0,
                self.source.stored_len() as f64,
            );
            let phase = f64::from(grain.age) / f64::from(grain.duration.max(1));
            let window = grain_window(self.window, phase);
            let angle = (grain.pan + 1.0) * PI * 0.25;
            let mono = (f64::from(frame[0]) + f64::from(frame[1])) * 0.5 * window;
            output[0] += mono * angle.cos();
            output[1] += mono * angle.sin();
            grain.age += 1;
            if grain.age >= grain.duration {
                grain.active = false;
                continue;
            }
            grain.position += grain.step;
            let end = self.source.stored_len() as f64;
            if grain.position < 0.0 || grain.position >= end {
                match self.edge {
                    GrainEdge::Wrap => grain.position = grain.position.rem_euclid(end.max(1.0)),
                    GrainEdge::Reflect => {
                        (grain.position, grain.step) =
                            reflected_position_and_step(grain.position, grain.step, end);
                    }
                    GrainEdge::Clamp => grain.active = false,
                }
            }
        }
        let normalization = 1.0 / f64::from(active.max(1)).sqrt();
        let level = self.level.value(controls) * normalization;
        [(output[0] * level) as f32, (output[1] * level) as f32]
    }

    fn spawn(&self, state: &mut GranularState, pitch: f32, controls: &[f64], sample_rate: f64) {
        let Some(grain) = state.grains.iter_mut().find(|grain| !grain.active) else {
            state.spawn_index = state.spawn_index.wrapping_add(1);
            return;
        };
        let identity = mix_modulation_identity(
            self.seed,
            mix_modulation_identity(
                self.node_identity,
                mix_modulation_identity(state.runtime_seed, state.spawn_index),
            ),
        );
        let random_a = identity_to_unit(identity);
        let random_b = identity_to_unit(mix_modulation_identity(identity, 0x9e37_79b9));
        let random_c = identity_to_unit(mix_modulation_identity(identity, 0x243f_6a88));
        state.spawn_index = state.spawn_index.wrapping_add(1);
        let position = (self.position.value(controls).clamp(0.0, 1.0)
            + state.scan_phase
            + (random_a * 2.0 - 1.0) * self.spray.value(controls).clamp(0.0, 1.0))
        .rem_euclid(1.0);
        let root = midi_frequency(self.root_key);
        let ratio =
            (f64::from(pitch.max(1.0)) / root) * 2.0_f64.powf(self.tune.value(controls) / 12.0);
        let reverse = random_b < self.direction_probability.value(controls).clamp(0.0, 1.0);
        grain.active = true;
        grain.position = position * self.source.stored_len() as f64;
        grain.step = if reverse { -ratio } else { ratio };
        grain.age = 0;
        grain.duration = (self.size.value(controls).clamp(0.005, 1.0) * sample_rate)
            .round()
            .max(1.0) as u32;
        grain.pan = (random_c * 2.0 - 1.0) * self.stereo_spread.value(controls).clamp(0.0, 1.0);
    }

    pub(super) fn transition_compatible(&self, replacement: &Self) -> bool {
        self.source.cache_identity() == replacement.source.cache_identity()
            && self.max_grains == replacement.max_grains
            && self.window == replacement.window
            && self.edge == replacement.edge
            && self.root_key == replacement.root_key
            && self.seed == replacement.seed
            && self.node_identity == replacement.node_identity
    }

    pub(super) fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(self.source.asset_id().len() as u64)
            .saturating_add(self.source.source_sha256().len() as u64)
    }
}

#[derive(Debug, Clone, Copy)]
struct Grain {
    position: f64,
    step: f64,
    pan: f64,
    age: u32,
    duration: u32,
    active: bool,
}

impl Default for Grain {
    fn default() -> Self {
        Self {
            position: 0.0,
            step: 0.0,
            pan: 0.0,
            age: 0,
            duration: 0,
            active: false,
        }
    }
}

fn reflected_position_and_step(position: f64, step: f64, end: f64) -> (f64, f64) {
    let maximum = (end - 1.0).max(0.0);
    if maximum == 0.0 {
        return (0.0, 0.0);
    }
    let period = maximum * 2.0;
    // Recover an increasing unwrapped coordinate from the current physical
    // direction, then fold an arbitrary number of boundary crossings through
    // one triangular period. This remains bounded even for very short sources
    // and the maximum admitted pitch ratio.
    let unwrapped = if step >= 0.0 { position } else { -position };
    let folded = unwrapped.rem_euclid(period);
    let speed = step.abs();
    if folded < maximum {
        (folded, speed)
    } else {
        (period - folded, -speed)
    }
}

fn grain_window(window: GrainWindow, phase: f64) -> f64 {
    let phase = phase.clamp(0.0, 1.0);
    match window {
        GrainWindow::Hann => 0.5 - 0.5 * (std::f64::consts::TAU * phase).cos(),
        GrainWindow::Triangle => 1.0 - (2.0 * phase - 1.0).abs(),
        GrainWindow::Tukey => {
            if phase < 0.25 {
                0.5 - 0.5 * (std::f64::consts::PI * phase / 0.25).cos()
            } else if phase > 0.75 {
                0.5 - 0.5 * (std::f64::consts::PI * (1.0 - phase) / 0.25).cos()
            } else {
                1.0
            }
        }
    }
}

fn identity_to_unit(identity: u64) -> f64 {
    (identity >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64))
}

#[derive(Debug, Clone, Copy)]
struct SampleCursor {
    position: f64,
    region_start: f64,
    region_end: f64,
    loop_start: f64,
    loop_end: f64,
    direction: f64,
    release_remaining: u32,
    initialized: bool,
    active: bool,
    last_gate: bool,
}

impl SampleCursor {
    const fn new() -> Self {
        Self {
            position: 0.0,
            region_start: 0.0,
            region_end: 0.0,
            loop_start: 0.0,
            loop_end: 0.0,
            direction: 1.0,
            release_remaining: 0,
            initialized: false,
            active: false,
            last_gate: false,
        }
    }

    fn activate(
        &mut self,
        region_start: f64,
        region_end: f64,
        loop_start: f64,
        loop_end: f64,
        direction: SampleDirection,
        gate: bool,
    ) {
        self.region_start = region_start;
        self.region_end = region_end;
        self.loop_start = loop_start.clamp(region_start, region_end);
        self.loop_end = loop_end.clamp(region_start, region_end);
        self.direction = match direction {
            SampleDirection::Forward => 1.0,
            SampleDirection::Reverse => -1.0,
        };
        self.position = if self.direction > 0.0 {
            region_start
        } else {
            (region_end - 1.0).max(region_start)
        };
        self.release_remaining = 0;
        self.initialized = true;
        self.active = region_end - region_start >= 1.0;
        self.last_gate = gate;
    }

    fn process(
        &mut self,
        source: &CompiledSampleSource,
        step: f64,
        loop_mode: SampleLoopMode,
        loop_crossfade: f64,
        gate: bool,
        gate_mode: SampleGateMode,
    ) -> [f32; 2] {
        if !self.active {
            return [0.0, 0.0];
        }
        if gate_mode == SampleGateMode::Gated && self.last_gate && !gate {
            self.release_remaining = SOURCE_RELEASE_FRAMES;
        }
        if gate_mode == SampleGateMode::Gated && !gate && self.release_remaining == 0 {
            self.active = false;
            return [0.0, 0.0];
        }
        self.last_gate = gate;

        let looping = gate && self.release_remaining == 0 && loop_mode != SampleLoopMode::Off;
        let mut output = self.read_with_loop_crossfade(source, looping, loop_crossfade);
        if self.release_remaining > 0 {
            let release = f64::from(self.release_remaining) / f64::from(SOURCE_RELEASE_FRAMES);
            output[0] = (f64::from(output[0]) * release) as f32;
            output[1] = (f64::from(output[1]) * release) as f32;
            self.release_remaining -= 1;
            if self.release_remaining == 0 {
                self.active = false;
            }
        }
        if self.active {
            self.advance(step, loop_mode, looping, loop_crossfade);
        }
        output
    }

    fn read_with_loop_crossfade(
        &self,
        source: &CompiledSampleSource,
        looping: bool,
        crossfade: f64,
    ) -> [f32; 2] {
        let primary = interpolated_frame(source, self.position, self.region_start, self.region_end);
        if !looping || crossfade <= 0.0 || self.loop_end - self.loop_start < 2.0 {
            return primary;
        }
        let crossfade = crossfade.min((self.loop_end - self.loop_start) * 0.5);
        let (alternate_position, amount) = if self.direction > 0.0
            && self.position >= self.loop_end - crossfade
            && self.position < self.loop_end
        {
            (
                self.loop_start + (self.position - (self.loop_end - crossfade)),
                (self.position - (self.loop_end - crossfade)) / crossfade,
            )
        } else if self.direction < 0.0
            && self.position >= self.loop_start
            && self.position < self.loop_start + crossfade
        {
            (
                self.loop_end - crossfade + (self.position - self.loop_start),
                (self.loop_start + crossfade - self.position) / crossfade,
            )
        } else {
            return primary;
        };
        let alternate = interpolated_frame(
            source,
            alternate_position,
            self.region_start,
            self.region_end,
        );
        let amount = amount.clamp(0.0, 1.0) as f32;
        [
            primary[0] + (alternate[0] - primary[0]) * amount,
            primary[1] + (alternate[1] - primary[1]) * amount,
        ]
    }

    fn advance(&mut self, step: f64, loop_mode: SampleLoopMode, looping: bool, crossfade: f64) {
        self.position += self.direction * step;
        if looping && self.loop_end - self.loop_start >= 1.0 {
            match loop_mode {
                SampleLoopMode::Forward => {
                    // The crossfade has already played the loop's first `crossfade`
                    // frames, so the wrap resumes after them rather than repeating them.
                    let crossfade = if crossfade > 0.0 && self.loop_end - self.loop_start >= 2.0 {
                        crossfade.min((self.loop_end - self.loop_start) * 0.5)
                    } else {
                        0.0
                    };
                    let cycle = self.loop_end - self.loop_start - crossfade;
                    if self.direction > 0.0 && self.position >= self.loop_end {
                        self.position =
                            self.loop_start + crossfade + (self.position - self.loop_end) % cycle;
                    } else if self.direction < 0.0 && self.position < self.loop_start {
                        self.position =
                            self.loop_end - crossfade - (self.loop_start - self.position) % cycle;
                    }
                }
                SampleLoopMode::PingPong => {
                    if self.direction > 0.0 && self.position >= self.loop_end {
                        self.position =
                            (self.loop_end - (self.position - self.loop_end)).max(self.loop_start);
                        self.direction = -1.0;
                    } else if self.direction < 0.0 && self.position < self.loop_start {
                        self.position = (self.loop_start + (self.loop_start - self.position))
                            .min(self.loop_end - 1.0);
                        self.direction = 1.0;
                    }
                }
                SampleLoopMode::Off => {}
            }
            return;
        }
        if self.position < self.region_start || self.position >= self.region_end {
            self.active = false;
        }
    }
}

fn interpolated_frame(
    source: &CompiledSampleSource,
    position: f64,
    region_start: f64,
    region_end: f64,
) -> [f32; 2] {
    if source.stored_len() == 0 || region_end - region_start < 1.0 {
        return [0.0, 0.0];
    }
    let maximum = (region_end - 1.0)
        .min(source.stored_len().saturating_sub(1) as f64)
        .max(region_start);
    let position = position.clamp(region_start, maximum);
    let first = position.floor() as usize;
    let second = (first + 1).min(maximum.floor() as usize);
    let fraction = (position - first as f64) as f32;
    let first = source.frame(first).unwrap_or([0.0, 0.0]);
    let second = source.frame(second).unwrap_or(first);
    [
        first[0] + (second[0] - first[0]) * fraction,
        first[1] + (second[1] - first[1]) * fraction,
    ]
}

fn apply_level_and_pan(output: &mut [f32; 2], level: f64, pan: f64) {
    let level = level as f32;
    output[0] *= level;
    output[1] *= level;
    let pan = pan.clamp(-1.0, 1.0);
    if pan > 0.0 {
        output[0] *= (pan * PI * 0.5).cos() as f32;
    } else if pan < 0.0 {
        output[1] *= (-pan * PI * 0.5).cos() as f32;
    }
}

fn frequency_to_midi(frequency: f64) -> u8 {
    (69.0 + 12.0 * (frequency.max(1.0) / 440.0).log2())
        .round()
        .clamp(0.0, 127.0) as u8
}

fn midi_frequency(pitch: u8) -> f64 {
    440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::reflected_position_and_step;

    #[test]
    fn reflection_folds_arbitrary_forward_and_reverse_overshoot() {
        let (forward_position, forward_step) = reflected_position_and_step(10.0, 10.0, 4.0);
        assert_eq!(forward_position.to_bits(), 2.0_f64.to_bits());
        assert_eq!(forward_step.to_bits(), (-10.0_f64).to_bits());

        let (reverse_position, reverse_step) = reflected_position_and_step(-10.0, -10.0, 4.0);
        assert_eq!(reverse_position.to_bits(), 2.0_f64.to_bits());
        assert_eq!(reverse_step.to_bits(), (-10.0_f64).to_bits());

        for (position, step) in [
            reflected_position_and_step(1_003.25, 1_002.25, 4.0),
            reflected_position_and_step(-1_003.25, -1_002.25, 4.0),
        ] {
            assert!((0.0..=3.0).contains(&position));
            assert_eq!(step.abs().to_bits(), 1_002.25_f64.to_bits());
        }
    }
}
