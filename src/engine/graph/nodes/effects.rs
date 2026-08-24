use super::Scalar;

pub(super) const MAX_DELAY_SECONDS: f64 = 2.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct DelayConfig {
    pub(super) delay_frames: usize,
    pub(super) feedback: Scalar,
    pub(super) mix: Scalar,
}

#[derive(Debug, Clone)]
pub(super) struct DelayState {
    buffer: Vec<[f32; 2]>,
    index: usize,
}

impl DelayState {
    pub(super) fn new(delay_frames: usize) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; delay_frames.max(1)],
            index: 0,
        }
    }

    pub(super) fn process(&mut self, input: [f32; 2], feedback: f32, mix: f32) -> [f32; 2] {
        let delayed = self.buffer[self.index];
        self.buffer[self.index] = [
            input[0] + delayed[0] * feedback,
            input[1] + delayed[1] * feedback,
        ];
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0;
        }
        let dry = 1.0 - mix;
        [
            input[0] * dry + delayed[0] * mix,
            input[1] * dry + delayed[1] * mix,
        ]
    }

    #[cfg(test)]
    pub(super) fn heap_bytes(&self) -> u64 {
        (self.buffer.capacity() as u64).saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
    }
}

impl DelayConfig {
    pub(super) fn process(
        self,
        state: &mut DelayState,
        audio: [f32; 2],
        controls: &[f64],
    ) -> [f32; 2] {
        state.process(
            audio,
            self.feedback.value(controls).clamp(0.0, 0.95) as f32,
            self.mix.value(controls).clamp(0.0, 1.0) as f32,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReverbConfig {
    pub(super) room: Scalar,
    pub(super) damping: Scalar,
    pub(super) mix: Scalar,
}

#[derive(Debug, Clone)]
pub(super) struct ReverbState {
    left: ReverbChannel,
    right: ReverbChannel,
}

impl ReverbState {
    pub(super) fn new(sample_rate: u32) -> Self {
        Self {
            left: ReverbChannel::new(sample_rate, 0),
            right: ReverbChannel::new(sample_rate, 23),
        }
    }

    fn process(&mut self, input: [f32; 2], feedback: f32, damping: f32, mix: f32) -> [f32; 2] {
        let wet_left = self
            .left
            .process(input[0] + input[1] * 0.15, feedback, damping);
        let wet_right = self
            .right
            .process(input[1] + input[0] * 0.15, feedback, damping);
        let dry = 1.0 - mix;
        [
            input[0] * dry + wet_left * mix,
            input[1] * dry + wet_right * mix,
        ]
    }

    #[cfg(test)]
    pub(super) fn heap_bytes(&self) -> u64 {
        reverb_channel_heap_bytes(&self.left).saturating_add(reverb_channel_heap_bytes(&self.right))
    }
}

impl ReverbConfig {
    pub(super) fn process(
        self,
        state: &mut ReverbState,
        audio: [f32; 2],
        controls: &[f64],
    ) -> [f32; 2] {
        let room = self.room.value(controls).clamp(0.0, 1.0);
        state.process(
            audio,
            (0.68 + room * 0.28) as f32,
            self.damping.value(controls).clamp(0.0, 0.95) as f32,
            self.mix.value(controls).clamp(0.0, 1.0) as f32,
        )
    }
}

#[derive(Debug, Clone)]
struct ReverbChannel {
    combs: Vec<CombFilter>,
    allpasses: Vec<AllpassFilter>,
}

impl ReverbChannel {
    fn new(sample_rate: u32, spread: usize) -> Self {
        let scale = sample_rate as f64 / 48_000.0;
        let combs = [1557, 1617, 1491, 1422]
            .into_iter()
            .map(|length| CombFilter::new(((length + spread) as f64 * scale).round() as usize))
            .collect();
        let allpasses = [225, 556]
            .into_iter()
            .map(|length| AllpassFilter::new(((length + spread) as f64 * scale).round() as usize))
            .collect();
        Self { combs, allpasses }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let mut output = self
            .combs
            .iter_mut()
            .map(|comb| comb.process(input, feedback, damping))
            .sum::<f32>()
            * 0.25;
        for allpass in &mut self.allpasses {
            output = allpass.process(output);
        }
        output
    }
}

#[derive(Debug, Clone)]
struct CombFilter {
    buffer: Vec<f32>,
    index: usize,
    filtered: f32,
}

impl CombFilter {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            index: 0,
            filtered: 0.0,
        }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let output = self.buffer[self.index];
        self.filtered = output * (1.0 - damping) + self.filtered * damping;
        self.buffer[self.index] = input + self.filtered * feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug, Clone)]
struct AllpassFilter {
    buffer: Vec<f32>,
    index: usize,
}

impl AllpassFilter {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            index: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.index];
        let output = -input + buffered;
        self.buffer[self.index] = input + buffered * 0.5;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

#[cfg(test)]
fn reverb_channel_heap_bytes(channel: &ReverbChannel) -> u64 {
    let filter_objects = channel
        .combs
        .capacity()
        .saturating_mul(std::mem::size_of::<CombFilter>())
        .saturating_add(
            channel
                .allpasses
                .capacity()
                .saturating_mul(std::mem::size_of::<AllpassFilter>()),
        ) as u64;
    channel
        .combs
        .iter()
        .map(|filter| filter.buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64)
        .chain(
            channel
                .allpasses
                .iter()
                .map(|filter| filter.buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64),
        )
        .fold(filter_objects, u64::saturating_add)
}

pub(super) fn graph_reverb_heap_bytes(sample_rate: u32) -> u64 {
    let scale = f64::from(sample_rate) / 48_000.0;
    let mut samples = 0_u64;
    for spread in [0_usize, 23] {
        for length in [1557_usize, 1617, 1491, 1422] {
            samples =
                samples.saturating_add(
                    (((length + spread) as f64 * scale).round() as usize).max(1) as u64,
                );
        }
        for length in [225_usize, 556] {
            samples =
                samples.saturating_add(
                    (((length + spread) as f64 * scale).round() as usize).max(1) as u64,
                );
        }
    }
    samples
        .saturating_mul(std::mem::size_of::<f32>() as u64)
        .saturating_add(8_u64.saturating_mul(std::mem::size_of::<CombFilter>() as u64))
        .saturating_add(4_u64.saturating_mul(std::mem::size_of::<AllpassFilter>() as u64))
}
