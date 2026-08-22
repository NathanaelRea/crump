use super::*;

#[derive(Debug)]
pub(super) struct CompiledMixProgram {
    pub(super) channels: Vec<CompiledChannel>,
    pub(super) routes: Vec<CompiledRoute>,
    pub(super) track_channels: Vec<usize>,
    pub(super) master_channel: usize,
    pub(super) auxiliary_slots: usize,
    pub(super) solo_masks: Vec<Vec<bool>>,
}

pub(super) fn vec_allocation_bytes<T>(capacity: usize) -> u64 {
    (capacity as u64).saturating_mul(std::mem::size_of::<T>() as u64)
}

pub(super) fn vec_bool_allocation_bytes(capacity_bits: usize) -> u64 {
    (capacity_bits.div_ceil(usize::BITS as usize) as u64)
        .saturating_mul(std::mem::size_of::<usize>() as u64)
}

impl CompiledMixProgram {
    pub(super) fn allocation_bytes(&self) -> u64 {
        let arc_allocation = (std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + std::mem::align_of::<Self>()) as u64;
        let channel_allocations = self
            .channels
            .iter()
            .map(|channel| {
                let processor_allocations = channel
                    .source_processors
                    .iter()
                    .chain(&channel.inserts)
                    .map(|processor| {
                        processor.program.heap_bytes().saturating_add(
                            vec_allocation_bytes::<usize>(processor.auxiliary_slots.capacity()),
                        )
                    })
                    .fold(0_u64, u64::saturating_add);
                (channel.identity.capacity() as u64)
                    .saturating_add(vec_allocation_bytes::<CompiledProcessorInstance>(
                        channel.source_processors.capacity(),
                    ))
                    .saturating_add(vec_allocation_bytes::<CompiledProcessorInstance>(
                        channel.inserts.capacity(),
                    ))
                    .saturating_add(vec_allocation_bytes::<usize>(
                        channel.outgoing_routes.capacity(),
                    ))
                    .saturating_add(processor_allocations)
            })
            .fold(0_u64, u64::saturating_add);
        let route_allocations = self
            .routes
            .iter()
            .map(|route| {
                (route.id.capacity() as u64)
                    .saturating_add(route.destination_display.capacity() as u64)
            })
            .fold(0_u64, u64::saturating_add);
        let solo_allocations = vec_allocation_bytes::<Vec<bool>>(self.solo_masks.capacity())
            .saturating_add(
                self.solo_masks
                    .iter()
                    .map(|mask| vec_bool_allocation_bytes(mask.capacity()))
                    .fold(0_u64, u64::saturating_add),
            );
        arc_allocation
            .saturating_add(vec_allocation_bytes::<CompiledChannel>(
                self.channels.capacity(),
            ))
            .saturating_add(channel_allocations)
            .saturating_add(vec_allocation_bytes::<CompiledRoute>(
                self.routes.capacity(),
            ))
            .saturating_add(route_allocations)
            .saturating_add(vec_allocation_bytes::<usize>(
                self.track_channels.capacity(),
            ))
            .saturating_add(solo_allocations)
    }

    pub(super) fn runtime_structure_bytes(&self) -> u64 {
        let processor_allocations = self
            .channels
            .iter()
            .flat_map(|channel| channel.source_processors.iter().chain(&channel.inserts))
            .map(|processor| {
                processor
                    .program
                    .heap_bytes()
                    .saturating_add(vec_allocation_bytes::<usize>(
                        processor.auxiliary_slots.capacity(),
                    ))
            })
            .fold(0_u64, u64::saturating_add);
        let processor_vector_overhead = self
            .channels
            .iter()
            .map(|channel| {
                vec_allocation_bytes::<ProcessorInstanceRuntime>(
                    channel.source_processors.capacity(),
                )
                .saturating_add(vec_allocation_bytes::<ProcessorInstanceRuntime>(
                    channel.inserts.capacity(),
                ))
                .saturating_sub(
                    ((channel.source_processors.capacity() + channel.inserts.capacity()) as u64)
                        .saturating_mul(std::mem::size_of::<ProcessorRuntime>() as u64),
                )
            })
            .fold(0_u64, u64::saturating_add);
        (std::mem::size_of::<MixGraphRuntime>() as u64)
            .saturating_add(vec_allocation_bytes::<ChannelRuntime>(
                self.channels.capacity(),
            ))
            .saturating_add(processor_vector_overhead)
            .saturating_add(processor_allocations)
            .saturating_add(vec_allocation_bytes::<CompensationDelay>(
                self.routes.capacity(),
            ))
            .saturating_add(vec_allocation_bytes::<Option<GainRuntime>>(
                self.routes.capacity(),
            ))
            .saturating_add(
                (self.auxiliary_slots as u64)
                    .saturating_mul(std::mem::size_of::<[f32; 2]>() as u64),
            )
            .saturating_add(vec_bool_allocation_bytes(self.channels.len()))
    }

    pub(super) fn topology_compatible(&self, other: &Self) -> bool {
        self.track_channels == other.track_channels
            && self.master_channel == other.master_channel
            && self.auxiliary_slots == other.auxiliary_slots
            && self.channels.len() == other.channels.len()
            && self.routes.len() == other.routes.len()
            && self
                .channels
                .iter()
                .zip(&other.channels)
                .all(|(left, right)| {
                    left.identity == right.identity
                        && left.role == right.role
                        && left.source_index == right.source_index
                        && left.level == right.level
                        && left.panner == right.panner
                        && left.mute == right.mute
                        && left.outgoing_routes == right.outgoing_routes
                        && left.source_compensation_frames == right.source_compensation_frames
                        && chains_compatible(&left.source_processors, &right.source_processors)
                        && chains_compatible(&left.inserts, &right.inserts)
                })
            && self.routes.iter().zip(&other.routes).all(|(left, right)| {
                left.id == right.id
                    && left.role == right.role
                    && left.from == right.from
                    && left.owner == right.owner
                    && left.destination == right.destination
                    && left.tap == right.tap
                    && left.gain == right.gain
                    && left.processor_index == right.processor_index
                    && left.compensation_frames == right.compensation_frames
            })
    }
}

#[derive(Debug, Clone)]
pub(super) struct CompiledChannel {
    pub(super) identity: String,
    pub(super) role: ChannelRole,
    pub(super) source_index: Option<usize>,
    pub(super) source_processors: Vec<CompiledProcessorInstance>,
    pub(super) inserts: Vec<CompiledProcessorInstance>,
    pub(super) level: MixScalar,
    pub(super) panner: Option<CompiledPanner>,
    pub(super) mute: MixScalar,
    pub(super) outgoing_routes: Vec<usize>,
    pub(super) source_compensation_frames: u32,
    pub(super) pre_latency_frames: u32,
    pub(super) post_latency_frames: u32,
}

impl CompiledChannel {
    pub(super) fn runtime(&self) -> ChannelRuntime {
        ChannelRuntime {
            source_delay: CompensationDelay::new(self.source_compensation_frames as usize),
            source_processors: self
                .source_processors
                .iter()
                .map(CompiledProcessorInstance::runtime)
                .collect(),
            inserts: self
                .inserts
                .iter()
                .map(CompiledProcessorInstance::runtime)
                .collect(),
            fader: GainRuntime::new(self.level),
            panner: self.panner.map(CompiledPanner::runtime),
        }
    }

    pub(super) fn processor_input_offset(&self, insert_index: usize) -> u32 {
        self.source_processors
            .iter()
            .chain(self.inserts.iter().take(insert_index))
            .map(|processor| processor.program.output_latency_frames())
            .fold(0_u32, u32::saturating_add)
    }

    pub(super) fn total_processor_latency(&self) -> u32 {
        self.source_processors
            .iter()
            .chain(&self.inserts)
            .map(|processor| processor.program.output_latency_frames())
            .fold(0_u32, u32::saturating_add)
    }
}

#[derive(Debug, Clone)]
pub(super) struct CompiledProcessorInstance {
    pub(super) program: ProcessorProgram,
    pub(super) auxiliary_slots: Vec<usize>,
    pub(super) telemetry_index: usize,
}

impl CompiledProcessorInstance {
    pub(super) fn runtime(&self) -> ProcessorInstanceRuntime {
        ProcessorInstanceRuntime {
            processor: self.program.runtime(),
            auxiliary_slots: self.auxiliary_slots.clone(),
            telemetry_index: self.telemetry_index,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ProcessorInstanceRuntime {
    pub(super) processor: ProcessorRuntime,
    pub(super) auxiliary_slots: Vec<usize>,
    pub(super) telemetry_index: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum MixScalar {
    Dynamic { index: ControlIndex, initial: f64 },
    Constant(f64),
}

impl PartialEq for MixScalar {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Dynamic { index: left, .. }, Self::Dynamic { index: right, .. }) => {
                left == right
            }
            (Self::Constant(left), Self::Constant(right)) => left.to_bits() == right.to_bits(),
            _ => false,
        }
    }
}

impl MixScalar {
    pub(super) fn from_target(
        controls: &CompiledControlProgram,
        target: &ParameterTarget,
    ) -> Result<Self, EngineError> {
        let index = controls.index(target).ok_or_else(|| {
            EngineError::Compile(format!("missing compiled Parameter `{target}`"))
        })?;
        let initial = controls.initial_value(index).ok_or_else(|| {
            EngineError::Compile(format!("missing initial Parameter value `{target}`"))
        })?;
        Ok(Self::Dynamic { index, initial })
    }

    pub(super) fn value(self, controls: &[f64]) -> f64 {
        match self {
            Self::Dynamic { index, initial } => controls.get(index.0).copied().unwrap_or(initial),
            Self::Constant(value) => value,
        }
    }

    pub(super) const fn initial(self) -> f64 {
        match self {
            Self::Dynamic { initial, .. } | Self::Constant(initial) => initial,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum CompiledPanner {
    MonoPan(MixScalar),
    Balance(MixScalar),
    StereoPan { left: MixScalar, right: MixScalar },
}

impl CompiledPanner {
    pub(super) fn compile(
        panner: &Panner,
        controls: &CompiledControlProgram,
        target: impl Fn(TrackParameter) -> ParameterTarget,
    ) -> Result<Self, EngineError> {
        match panner {
            Panner::MonoPan { .. } => Ok(Self::MonoPan(MixScalar::from_target(
                controls,
                &target(TrackParameter::Pan),
            )?)),
            Panner::Balance { .. } => Ok(Self::Balance(MixScalar::from_target(
                controls,
                &target(TrackParameter::Balance),
            )?)),
            Panner::StereoPan { .. } => Ok(Self::StereoPan {
                left: MixScalar::from_target(controls, &target(TrackParameter::LeftPan))?,
                right: MixScalar::from_target(controls, &target(TrackParameter::RightPan))?,
            }),
        }
    }

    pub(super) const fn mode(self) -> PannerMode {
        match self {
            Self::MonoPan(_) => PannerMode::MonoPan,
            Self::Balance(_) => PannerMode::Balance,
            Self::StereoPan { .. } => PannerMode::StereoPan,
        }
    }

    pub(super) fn initial_positions(self) -> [f64; 2] {
        match self {
            Self::MonoPan(position) | Self::Balance(position) => [position.initial(), 0.0],
            Self::StereoPan { left, right } => [left.initial(), right.initial()],
        }
    }

    pub(super) fn positions(self, controls: &[f64]) -> [f64; 2] {
        match self {
            Self::MonoPan(position) | Self::Balance(position) => [position.value(controls), 0.0],
            Self::StereoPan { left, right } => [left.value(controls), right.value(controls)],
        }
    }

    pub(super) fn runtime(self) -> PannerRuntime {
        PannerRuntime::new(self)
    }

    #[cfg(test)]
    pub(super) fn process(self, sample: [f32; 2], controls: &[f64]) -> [f32; 2] {
        self.runtime().process(sample, controls)
    }
}

#[derive(Debug, Clone)]
pub(super) struct GainRuntime {
    pub(super) scalar: MixScalar,
    pub(super) last_value: f64,
    pub(super) coefficient: f32,
    #[cfg(test)]
    pub(super) coefficient_updates: usize,
}

impl GainRuntime {
    pub(super) fn new(scalar: MixScalar) -> Self {
        let initial = scalar.initial();
        Self {
            scalar,
            last_value: initial,
            coefficient: db_gain(initial),
            #[cfg(test)]
            coefficient_updates: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.last_value = self.scalar.initial();
        self.coefficient = db_gain(self.last_value);
        #[cfg(test)]
        {
            self.coefficient_updates = 0;
        }
    }

    pub(super) fn coefficient(&mut self, controls: &[f64]) -> f32 {
        let value = self.scalar.value(controls);
        if value.to_bits() != self.last_value.to_bits() {
            self.last_value = value;
            self.coefficient = db_gain(value);
            #[cfg(test)]
            {
                self.coefficient_updates = self.coefficient_updates.saturating_add(1);
            }
        }
        self.coefficient
    }
}

#[derive(Debug, Clone)]
pub(super) struct PannerRuntime {
    pub(super) program: CompiledPanner,
    pub(super) last_positions: [f64; 2],
    pub(super) matrix: [[f32; 2]; 2],
    #[cfg(test)]
    pub(super) coefficient_updates: usize,
}

impl PannerRuntime {
    pub(super) fn new(program: CompiledPanner) -> Self {
        let last_positions = program.initial_positions();
        Self {
            program,
            last_positions,
            matrix: panner_matrix(program.mode(), last_positions),
            #[cfg(test)]
            coefficient_updates: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.last_positions = self.program.initial_positions();
        self.matrix = panner_matrix(self.program.mode(), self.last_positions);
        #[cfg(test)]
        {
            self.coefficient_updates = 0;
        }
    }

    pub(super) fn process(&mut self, sample: [f32; 2], controls: &[f64]) -> [f32; 2] {
        let positions = self.program.positions(controls);
        if positions
            .iter()
            .zip(self.last_positions)
            .any(|(current, previous)| current.to_bits() != previous.to_bits())
        {
            self.last_positions = positions;
            self.matrix = panner_matrix(self.program.mode(), positions);
            #[cfg(test)]
            {
                self.coefficient_updates = self.coefficient_updates.saturating_add(1);
            }
        }
        [
            sample[0] * self.matrix[0][0] + sample[1] * self.matrix[0][1],
            sample[0] * self.matrix[1][0] + sample[1] * self.matrix[1][1],
        ]
    }
}

pub(super) fn panner_matrix(mode: PannerMode, positions: [f64; 2]) -> [[f32; 2]; 2] {
    match mode {
        PannerMode::MonoPan => {
            let angle = (positions[0].clamp(-1.0, 1.0) + 1.0) * std::f64::consts::FRAC_PI_4;
            let left = angle.cos() as f32 * 0.5;
            let right = angle.sin() as f32 * 0.5;
            [[left, left], [right, right]]
        }
        PannerMode::Balance => {
            let position = positions[0].clamp(-1.0, 1.0);
            let left = if position > 0.0 {
                (position * std::f64::consts::FRAC_PI_2).cos() as f32
            } else {
                1.0
            };
            let right = if position < 0.0 {
                (-position * std::f64::consts::FRAC_PI_2).cos() as f32
            } else {
                1.0
            };
            [[left, 0.0], [0.0, right]]
        }
        PannerMode::StereoPan => {
            let left_angle = (positions[0].clamp(-1.0, 1.0) + 1.0) * std::f64::consts::FRAC_PI_4;
            let right_angle = (positions[1].clamp(-1.0, 1.0) + 1.0) * std::f64::consts::FRAC_PI_4;
            [
                [left_angle.cos() as f32, right_angle.cos() as f32],
                [left_angle.sin() as f32, right_angle.sin() as f32],
            ]
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CompiledRoute {
    pub(super) id: String,
    pub(super) role: RouteRole,
    pub(super) from: usize,
    pub(super) owner: usize,
    pub(super) destination: RouteDestination,
    pub(super) destination_display: String,
    pub(super) tap: TapPoint,
    pub(super) gain: Option<MixScalar>,
    pub(super) processor_index: Option<usize>,
    pub(super) compensation_frames: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RouteDestination {
    Bus(usize),
    Auxiliary(usize),
}

/// Preallocated mutable processing state shared by monitoring and rendering.
#[derive(Debug, Clone)]
pub(crate) struct MixGraphRuntime {
    pub(super) program: Arc<CompiledMixProgram>,
    pub(super) channels: Vec<ChannelRuntime>,
    pub(super) route_delays: Vec<CompensationDelay>,
    pub(super) route_gains: Vec<Option<GainRuntime>>,
    pub(super) bus_inputs: Vec<[f32; 2]>,
    pub(super) auxiliary_inputs: Vec<[f32; 2]>,
    pub(super) meters: Vec<ChannelMeter>,
    pub(super) processor_meters: Vec<ProcessorMeter>,
    pub(super) channel_taps: [ChannelTapSamples; MAX_MIX_CHANNELS],
    pub(super) soloed: Vec<bool>,
    pub(super) cue: Option<(usize, TapPoint)>,
    #[cfg(test)]
    pub(super) reset_calls: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ChannelTapSamples {
    pub(super) source: [f32; 2],
    pub(super) pre_fader: [f32; 2],
    pub(super) post_fader: [f32; 2],
}

#[derive(Debug, Clone)]
pub(super) struct ChannelRuntime {
    pub(super) source_delay: CompensationDelay,
    pub(super) source_processors: Vec<ProcessorInstanceRuntime>,
    pub(super) inserts: Vec<ProcessorInstanceRuntime>,
    pub(super) fader: GainRuntime,
    pub(super) panner: Option<PannerRuntime>,
}

impl MixGraphRuntime {
    /// Reports whether mutable processing state can transfer without changing semantics.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn topology_compatible(&self, other: &Self) -> bool {
        self.program.topology_compatible(&other.program)
    }

    /// Executes one sample through every processing scope in topological order.
    pub(crate) fn process(
        &mut self,
        sources: &[[f32; 2]],
        controls: &[f64],
        audition: bool,
    ) -> [f32; 2] {
        debug_assert_eq!(sources.len(), self.program.track_channels.len());
        self.bus_inputs.fill([0.0, 0.0]);
        self.auxiliary_inputs.fill([0.0, 0.0]);
        let any_solo = audition && self.soloed.iter().any(|selected| *selected);
        let mut master = [0.0_f32, 0.0_f32];
        let mut cue_output = None;
        let program = &self.program;
        let channel_states = &mut self.channels;
        let route_delays = &mut self.route_delays;
        let route_gains = &mut self.route_gains;
        let bus_inputs = &mut self.bus_inputs;
        let auxiliary_inputs = &mut self.auxiliary_inputs;
        let meters = &mut self.meters;
        let processor_meters = &mut self.processor_meters;
        let channel_taps = &mut self.channel_taps;
        for index in 0..program.channels.len() {
            let channel = &program.channels[index];
            let state = &mut channel_states[index];
            let mut sample = bus_inputs[index];
            if let Some(source_index) = channel.source_index {
                let source = sources.get(source_index).copied().unwrap_or([0.0, 0.0]);
                let source = state.source_delay.process(source);
                sample[0] += source[0];
                sample[1] += source[1];
            }
            for processor in &mut state.source_processors {
                sample = process_processor(
                    processor,
                    sample,
                    auxiliary_inputs,
                    controls,
                    processor_meters,
                    audition,
                );
            }
            let source = sample;
            for processor in &mut state.inserts {
                sample = process_processor(
                    processor,
                    sample,
                    auxiliary_inputs,
                    controls,
                    processor_meters,
                    audition,
                );
            }
            let pre_fader = sample;
            let gain = state.fader.coefficient(controls);
            sample[0] *= gain;
            sample[1] *= gain;
            if let Some(panner) = &mut state.panner {
                sample = panner.process(sample, controls);
            }
            if channel.mute.value(controls) >= 0.5 {
                sample = [0.0, 0.0];
            }
            let post_fader = sample;
            channel_taps[index] = ChannelTapSamples {
                source,
                pre_fader,
                post_fader,
            };
            meters[index].observe(post_fader);
            if audition && self.cue == Some((index, TapPoint::PreFader)) {
                cue_output = Some(pre_fader);
            } else if audition && self.cue == Some((index, TapPoint::PostFader)) {
                cue_output = Some(post_fader);
            }
            let audible = !any_solo
                || self
                    .soloed
                    .iter()
                    .enumerate()
                    .any(|(selected, enabled)| *enabled && program.solo_masks[selected][index]);
            for route_index in &channel.outgoing_routes {
                let route = &program.routes[*route_index];
                let mut routed = match route.tap {
                    TapPoint::PreFader => pre_fader,
                    TapPoint::PostFader => post_fader,
                };
                if !audible {
                    routed = [0.0, 0.0];
                }
                if let Some(gain) = &mut route_gains[*route_index] {
                    let gain = gain.coefficient(controls);
                    routed[0] *= gain;
                    routed[1] *= gain;
                }
                routed = route_delays[*route_index].process(routed);
                match route.destination {
                    RouteDestination::Bus(destination) => {
                        bus_inputs[destination][0] += routed[0];
                        bus_inputs[destination][1] += routed[1];
                    }
                    RouteDestination::Auxiliary(slot) => auxiliary_inputs[slot] = routed,
                }
            }
            if index == program.master_channel {
                master = post_fader;
            }
        }
        cue_output.unwrap_or(master)
    }

    /// Resets all time-dependent state after an explicit transport seek.
    ///
    /// Realtime publication preparation calls this on the controller thread; it
    /// clears existing storage and never replaces heap-owning state.
    pub(crate) fn reset(&mut self) {
        #[cfg(test)]
        {
            self.reset_calls = self.reset_calls.saturating_add(1);
        }
        for runtime in &mut self.channels {
            runtime.source_delay.reset();
            for processor in runtime
                .source_processors
                .iter_mut()
                .chain(&mut runtime.inserts)
            {
                processor.processor.reset();
            }
            runtime.fader.reset();
            if let Some(panner) = &mut runtime.panner {
                panner.reset();
            }
        }
        for delay in &mut self.route_delays {
            delay.reset();
        }
        for gain in self.route_gains.iter_mut().flatten() {
            gain.reset();
        }
        self.bus_inputs.fill([0.0, 0.0]);
        self.auxiliary_inputs.fill([0.0, 0.0]);
        self.meters.fill(ChannelMeter::default());
        self.processor_meters.fill(ProcessorMeter::default());
        self.channel_taps.fill(ChannelTapSamples::default());
    }

    #[cfg(test)]
    pub(crate) const fn reset_calls(&self) -> usize {
        self.reset_calls
    }

    /// Enables or disables one transient solo without changing source or render state.
    pub(crate) fn set_solo(&mut self, identity: &str, enabled: bool) -> bool {
        let Some(index) = self
            .program
            .channels
            .iter()
            .position(|channel| channel.identity == identity)
        else {
            return false;
        };
        self.soloed[index] = enabled;
        true
    }

    /// Selects one transient cue tap, or clears cue when `identity` is `None`.
    pub(crate) fn set_cue(&mut self, identity: Option<&str>, tap: TapPoint) -> bool {
        let Some(identity) = identity else {
            self.cue = None;
            return true;
        };
        let Some(index) = self
            .program
            .channels
            .iter()
            .position(|channel| channel.identity == identity)
        else {
            return false;
        };
        self.cue = Some((index, tap));
        true
    }

    /// Sets or clears a monitoring-only bypass override on every runtime instance
    /// of one song-wide durable processor ID.
    pub(crate) fn set_processor_bypass(
        &mut self,
        id: &str,
        bypass: Option<bool>,
        controls: &[f64],
    ) -> usize {
        let mut changed = 0;
        let meters = &mut self.processor_meters;
        for processor in self.channels.iter_mut().flat_map(|channel| {
            channel
                .source_processors
                .iter_mut()
                .chain(&mut channel.inserts)
        }) {
            if processor.processor.id() == id {
                processor.processor.set_transient_bypass(bypass);
                let (authored, transient, effective, mix) =
                    processor.processor.bypass_state(controls, true);
                if let Some(meter) = meters.get_mut(processor.telemetry_index) {
                    meter.authored_bypass = authored;
                    meter.transient_bypass = transient;
                    meter.effective_bypass = effective;
                    meter.bypass_mix = mix;
                }
                changed += 1;
            }
        }
        changed
    }

    /// Moves compatible shared DSP, compensation, meter, and audition state.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn adopt_state_from(&mut self, current: &mut Self) {
        if !self.program.topology_compatible(&current.program) {
            return;
        }
        std::mem::swap(&mut self.channels, &mut current.channels);
        std::mem::swap(&mut self.route_delays, &mut current.route_delays);
        std::mem::swap(&mut self.route_gains, &mut current.route_gains);
        std::mem::swap(&mut self.meters, &mut current.meters);
        std::mem::swap(&mut self.processor_meters, &mut current.processor_meters);
        std::mem::swap(&mut self.channel_taps, &mut current.channel_taps);
        std::mem::swap(&mut self.soloed, &mut current.soloed);
        std::mem::swap(&mut self.cue, &mut current.cue);
    }

    pub(crate) fn channel_index(&self, identity: &str) -> Option<usize> {
        self.program
            .channels
            .iter()
            .position(|channel| channel.identity == identity)
    }

    pub(crate) fn channel_tap(&self, index: usize, tap: TapPoint) -> Option<[f32; 2]> {
        self.channel_taps.get(index).map(|samples| match tap {
            TapPoint::PreFader => samples.pre_fader,
            TapPoint::PostFader => samples.post_fader,
        })
    }

    pub(crate) fn source_tap(&self, index: usize) -> Option<[f32; 2]> {
        self.channel_taps.get(index).map(|samples| samples.source)
    }

    pub(crate) fn channel_activity(&self) -> ([ChannelMeter; MAX_MIX_CHANNELS], usize) {
        let mut activity = [ChannelMeter::default(); MAX_MIX_CHANNELS];
        let count = self.meters.len().min(MAX_MIX_CHANNELS);
        activity[..count].copy_from_slice(&self.meters[..count]);
        (activity, count)
    }

    pub(crate) fn copy_processor_activity(
        &self,
        activity: &mut [ProcessorMeter; MAX_PROCESSOR_INSTANCES],
    ) -> usize {
        let count = self.processor_meters.len().min(MAX_PROCESSOR_INSTANCES);
        activity[..count].copy_from_slice(&self.processor_meters[..count]);
        count
    }

    pub(crate) fn clear_current_activity(&mut self) {
        for meter in &mut self.meters {
            meter.current_peak = 0.0;
        }
        for meter in &mut self.processor_meters {
            meter.gain_reduction_db = 0.0;
        }
    }

    #[cfg(test)]
    pub(crate) fn heap_storage_fingerprint(&self) -> u64 {
        fn include(seed: u64, pointer: *const (), capacity: usize) -> u64 {
            seed.rotate_left(7) ^ (pointer as usize as u64) ^ (capacity as u64).rotate_left(31)
        }
        let mut fingerprint = include(0xCBF2_9CE4_8422_2325, Arc::as_ptr(&self.program).cast(), 1);
        fingerprint = include(
            fingerprint,
            self.channels.as_ptr().cast(),
            self.channels.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.route_delays.as_ptr().cast(),
            self.route_delays.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.route_gains.as_ptr().cast(),
            self.route_gains.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.bus_inputs.as_ptr().cast(),
            self.bus_inputs.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.auxiliary_inputs.as_ptr().cast(),
            self.auxiliary_inputs.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.meters.as_ptr().cast(),
            self.meters.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.processor_meters.as_ptr().cast(),
            self.processor_meters.capacity(),
        );
        for channel in &self.channels {
            fingerprint = include(
                fingerprint,
                channel.source_delay.buffer.as_ptr().cast(),
                channel.source_delay.buffer.capacity(),
            );
            fingerprint = include(
                fingerprint,
                channel.source_processors.as_ptr().cast(),
                channel.source_processors.capacity(),
            );
            fingerprint = include(
                fingerprint,
                channel.inserts.as_ptr().cast(),
                channel.inserts.capacity(),
            );
            for processor in channel.source_processors.iter().chain(&channel.inserts) {
                fingerprint ^= processor
                    .processor
                    .heap_storage_fingerprint()
                    .rotate_left(13);
                fingerprint = include(
                    fingerprint,
                    processor.auxiliary_slots.as_ptr().cast(),
                    processor.auxiliary_slots.capacity(),
                );
            }
        }
        for delay in &self.route_delays {
            fingerprint = include(
                fingerprint,
                delay.buffer.as_ptr().cast(),
                delay.buffer.capacity(),
            );
        }
        fingerprint
    }

    #[cfg(test)]
    pub(crate) fn poison_track_level_for_test(&mut self, index: usize) {
        if let Some(channel_index) = self.program.track_channels.get(index).copied() {
            self.channels[channel_index].fader.scalar = MixScalar::Constant(f64::NAN);
        }
    }
}

pub(super) fn process_processor(
    processor: &mut ProcessorInstanceRuntime,
    sample: [f32; 2],
    auxiliary_inputs: &[[f32; 2]],
    controls: &[f64],
    processor_meters: &mut [ProcessorMeter],
    audition: bool,
) -> [f32; 2] {
    let mut sidechain: Option<[f32; 2]> = None;
    for slot in &processor.auxiliary_slots {
        let input = auxiliary_inputs.get(*slot).copied().unwrap_or([0.0, 0.0]);
        if let Some(sum) = &mut sidechain {
            sum[0] += input[0];
            sum[1] += input[1];
        } else {
            sidechain = Some(input);
        }
    }
    let output = processor
        .processor
        .process_with_audition(sample, sidechain, controls, audition);
    if let Some(meter) = processor_meters.get_mut(processor.telemetry_index) {
        if let Some(gain_reduction_db) = processor.processor.gain_reduction_db() {
            meter.observe_gain_reduction(gain_reduction_db);
        }
        let (authored, transient, effective, mix) =
            processor.processor.bypass_state(controls, audition);
        meter.observe_processing(sample, output, authored, transient, effective, mix);
    }
    output
}

#[derive(Debug, Clone)]
pub(super) struct CompensationDelay {
    pub(super) buffer: Vec<[f32; 2]>,
    pub(super) cursor: usize,
    pub(super) frames: usize,
}

impl CompensationDelay {
    pub(super) fn new(frames: usize) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; frames.max(1)],
            cursor: 0,
            frames,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
    }

    pub(super) fn process(&mut self, sample: [f32; 2]) -> [f32; 2] {
        if self.frames == 0 {
            return sample;
        }
        let output = self.buffer[self.cursor];
        self.buffer[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }
}

/// Callback-cheap processor activity accumulator aligned to processor inspection indexes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct ProcessorMeter {
    /// Current compressor gain reduction in decibels; zero for other processor kinds.
    pub gain_reduction_db: f32,
    /// Maximum compressor gain reduction observed since runtime creation.
    pub maximum_gain_reduction_db: f32,
    /// Current absolute stereo input peak.
    pub input_peak: f32,
    /// Current absolute stereo output peak.
    pub output_peak: f32,
    /// Authored bypass value at the current control frame.
    pub authored_bypass: bool,
    /// Monitoring-only override, when active.
    pub transient_bypass: Option<bool>,
    /// Effective bypass target for the current processing mode.
    pub effective_bypass: bool,
    /// Click-guard blend, where zero is wet and one is latency-aligned dry.
    pub bypass_mix: f32,
}

impl ProcessorMeter {
    pub(super) fn observe_gain_reduction(&mut self, gain_reduction_db: f32) {
        self.gain_reduction_db = gain_reduction_db.max(0.0);
        self.maximum_gain_reduction_db = self.maximum_gain_reduction_db.max(self.gain_reduction_db);
    }

    pub(super) fn observe_processing(
        &mut self,
        input: [f32; 2],
        output: [f32; 2],
        authored: bool,
        transient: Option<bool>,
        effective: bool,
        mix: f32,
    ) {
        self.input_peak = input[0].abs().max(input[1].abs());
        self.output_peak = output[0].abs().max(output[1].abs());
        self.authored_bypass = authored;
        self.transient_bypass = transient;
        self.effective_bypass = effective;
        self.bypass_mix = mix;
    }
}

/// Callback-cheap channel activity accumulator.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelMeter {
    /// Current absolute stereo peak.
    pub current_peak: f32,
    /// Maximum absolute peak since runtime creation.
    pub peak: f32,
    /// Exponential moving average of stereo absolute activity.
    pub average: f32,
}

impl ChannelMeter {
    pub(super) fn observe(&mut self, sample: [f32; 2]) {
        let peak = sample[0].abs().max(sample[1].abs());
        self.current_peak = peak;
        self.peak = self.peak.max(peak);
        self.average += (peak - self.average) * 0.001;
    }
}
