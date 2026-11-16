#[derive(Debug, Clone)]
pub(super) struct StutterState {
    pub(super) buffer: Vec<[f32; 2]>,
    write_cursor: usize,
    valid_frames: usize,
    playback_start: usize,
    playback_cursor: usize,
    remaining_frames: u64,
    prior_trigger: bool,
    ignored_triggers: u64,
}

impl StutterState {
    pub(super) fn new(slice_frames: usize) -> Self {
        Self {
            buffer: vec![[0.0; 2]; slice_frames.max(1)],
            write_cursor: 0,
            valid_frames: 0,
            playback_start: 0,
            playback_cursor: 0,
            remaining_frames: 0,
            prior_trigger: false,
            ignored_triggers: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &mut self,
        input: [f32; 2],
        trigger: f64,
        chance: f64,
        crossfade_ratio: f64,
        mix: f64,
        absolute_frame: u64,
        repeats: u32,
        interval_frames: u64,
        seed: u64,
    ) -> [f32; 2] {
        let trigger_high = trigger >= 0.5;
        let manual = trigger_high && !self.prior_trigger;
        self.prior_trigger = trigger_high;
        let cycle = absolute_frame / interval_frames.max(1);
        let automatic = absolute_frame.is_multiple_of(interval_frames.max(1))
            && unit_random(splitmix64(seed ^ cycle)) < chance.clamp(0.0, 1.0);
        let requested = manual || automatic;
        if requested {
            if self.remaining_frames > 0 || self.valid_frames < self.buffer.len() {
                self.ignored_triggers = self.ignored_triggers.saturating_add(1);
            } else {
                self.playback_start = self.write_cursor;
                self.playback_cursor = 0;
                self.remaining_frames =
                    (self.buffer.len() as u64).saturating_mul(u64::from(repeats));
            }
        }

        let wet = if self.remaining_frames > 0 {
            let slice_frames = self.buffer.len();
            let current = self.buffer[(self.playback_start + self.playback_cursor) % slice_frames];
            let crossfade_frames =
                (crossfade_ratio.clamp(0.0, 0.5) * slice_frames as f64).round() as usize;
            let output = if crossfade_frames > 0
                && self.playback_cursor >= slice_frames - crossfade_frames
            {
                let offset = self.playback_cursor - (slice_frames - crossfade_frames);
                let alternate = self.buffer[(self.playback_start + offset) % slice_frames];
                let blend = (offset + 1) as f32 / crossfade_frames as f32;
                [
                    current[0] + (alternate[0] - current[0]) * blend,
                    current[1] + (alternate[1] - current[1]) * blend,
                ]
            } else {
                current
            };
            self.playback_cursor = (self.playback_cursor + 1) % slice_frames;
            self.remaining_frames -= 1;
            if self.remaining_frames == 0 {
                self.write_cursor = 0;
                self.valid_frames = 0;
                self.playback_cursor = 0;
            }
            output
        } else {
            self.buffer[self.write_cursor] = input;
            self.write_cursor = (self.write_cursor + 1) % self.buffer.len();
            self.valid_frames = self.valid_frames.saturating_add(1).min(self.buffer.len());
            input
        };
        let mix = mix.clamp(0.0, 1.0) as f32;
        [
            input[0] + (wet[0] - input[0]) * mix,
            input[1] + (wet[1] - input[1]) * mix,
        ]
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.write_cursor = 0;
        self.valid_frames = 0;
        self.playback_start = 0;
        self.playback_cursor = 0;
        self.remaining_frames = 0;
        self.prior_trigger = false;
        self.ignored_triggers = 0;
    }

    pub(super) const fn ignored_triggers(&self) -> u64 {
        self.ignored_triggers
    }

    pub(super) const fn repeating(&self) -> bool {
        self.remaining_frames > 0
    }
}

pub(super) fn stable_stutter_seed(authored: u32, processor_id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ u64::from(authored);
    for byte in processor_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    splitmix64(hash)
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn unit_random(value: u64) -> f64 {
    (value >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preceding_slice_repeats_exactly_and_held_trigger_is_bounded() {
        let mut state = StutterState::new(4);
        let process = |state: &mut StutterState, frame, input, trigger| {
            state.process([input, -input], trigger, 0.0, 0.0, 1.0, frame, 2, 16, 7)[0]
        };
        assert_eq!(process(&mut state, 0, 1.0, 0.0), 1.0);
        assert_eq!(process(&mut state, 1, 2.0, 1.0), 2.0);
        assert_eq!(process(&mut state, 2, 3.0, 0.0), 3.0);
        assert_eq!(process(&mut state, 3, 4.0, 0.0), 4.0);
        let repeated = (4..12)
            .map(|frame| process(&mut state, frame, 9.0, 1.0))
            .collect::<Vec<_>>();
        assert_eq!(repeated, vec![1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0]);
        assert!(!state.repeating());
        assert_eq!(process(&mut state, 12, 10.0, 1.0), 10.0);
        assert_eq!(state.ignored_triggers(), 1);
    }

    #[test]
    fn crossfade_and_seeded_grid_trigger_stay_inside_frozen_slice() {
        let mut state = StutterState::new(4);
        for (frame, input) in [1.0, 2.0, 3.0, 4.0].into_iter().enumerate() {
            state.process([input; 2], 0.0, 1.0, 0.0, 1.0, frame as u64, 1, 4, 11);
        }
        let output = (4..8)
            .map(|frame| state.process([9.0; 2], 0.0, 1.0, 0.5, 1.0, frame, 1, 4, 11)[0])
            .collect::<Vec<_>>();
        assert_eq!(output, vec![1.0, 2.0, 2.0, 2.0]);
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_eq!(
            state.ignored_triggers(),
            1,
            "frame zero auto-trigger warms up"
        );
    }

    #[test]
    fn stable_seed_mixer_is_identity_based() {
        assert_eq!(
            stable_stutter_seed(41, "repeat"),
            12_422_853_647_353_421_410
        );
        assert_ne!(
            stable_stutter_seed(41, "repeat"),
            stable_stutter_seed(42, "repeat")
        );
        assert_ne!(
            stable_stutter_seed(41, "repeat"),
            stable_stutter_seed(41, "other")
        );
    }
}
