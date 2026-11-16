use super::*;

/// Mutable preallocated state for one shared stereo processor instance.
#[derive(Debug, Clone)]
pub(crate) struct ProcessorRuntime {
    pub(super) program: ProcessorProgram,
    pub(super) state: ProcessorState,
    pub(super) declared_delay: StereoDelay,
    pub(super) bypass_delay: StereoDelay,
    pub(super) transient_bypass: Option<bool>,
    pub(super) transient_bypass_guard_active: bool,
    pub(super) bypass_mix: f32,
    pub(super) transport_frame: u64,
}

// Large fixed FIR histories stay inline so callback state has no hidden heap owner.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub(super) enum ProcessorState {
    Utility,
    Eq(Vec<StereoBiquad>),
    Compressor(CompressorState),
    Gate(GateState),
    Delay(DelayState),
    Reverb(ReverbState),
    Saturation(SaturationState),
    ModulationDelay(ModulationDelayState),
    Crusher(CrusherState),
    Stutter(StutterState),
    Limiter(LimiterState),
    Latency,
}

impl ProcessorRuntime {
    pub(super) fn new(program: ProcessorProgram) -> Self {
        let state = match &program.config {
            ProcessorConfig::Utility { .. } => ProcessorState::Utility,
            ProcessorConfig::Eq { bands } => {
                ProcessorState::Eq(vec![StereoBiquad::default(); bands.len()])
            }
            ProcessorConfig::Compressor { .. } => {
                ProcessorState::Compressor(CompressorState::new())
            }
            ProcessorConfig::Gate { .. } => ProcessorState::Gate(GateState::new()),
            ProcessorConfig::Delay { maximum_frames, .. } => {
                ProcessorState::Delay(DelayState::new(*maximum_frames))
            }
            ProcessorConfig::Reverb { .. } => {
                ProcessorState::Reverb(ReverbState::new(program.sample_rate))
            }
            ProcessorConfig::Saturation { .. } => {
                ProcessorState::Saturation(SaturationState::new())
            }
            ProcessorConfig::ModulationDelay { maximum_frames, .. } => {
                ProcessorState::ModulationDelay(ModulationDelayState::new(*maximum_frames))
            }
            ProcessorConfig::Crusher { .. } => ProcessorState::Crusher(CrusherState::new()),
            ProcessorConfig::Stutter { slice_frames, .. } => {
                ProcessorState::Stutter(StutterState::new(*slice_frames))
            }
            ProcessorConfig::Limiter {
                lookahead_frames, ..
            } => ProcessorState::Limiter(LimiterState::new(*lookahead_frames)),
            ProcessorConfig::Latency => ProcessorState::Latency,
        };
        let declared_delay = StereoDelay::new(program.declared_latency_frames as usize);
        let bypass_delay = StereoDelay::new(program.output_latency_frames() as usize);
        let bypass_mix = if program.authored_bypass { 1.0 } else { 0.0 };
        Self {
            program,
            state,
            declared_delay,
            bypass_delay,
            transient_bypass: None,
            transient_bypass_guard_active: false,
            bypass_mix,
            transport_frame: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn process(
        &mut self,
        sample: [f32; 2],
        sidechain: Option<[f32; 2]>,
        controls: &[f64],
    ) -> [f32; 2] {
        self.process_with_audition(sample, sidechain, controls, false)
    }

    pub(crate) fn process_with_audition(
        &mut self,
        sample: [f32; 2],
        sidechain: Option<[f32; 2]>,
        controls: &[f64],
        audition: bool,
    ) -> [f32; 2] {
        let dry = self.bypass_delay.process(sample);
        let processed = match (&self.program.config, &mut self.state) {
            (
                ProcessorConfig::Utility {
                    gain,
                    width,
                    invert,
                    mono,
                },
                ProcessorState::Utility,
            ) => process_utility(
                sample,
                gain.value(controls),
                width.value(controls),
                *invert,
                *mono,
            ),
            (ProcessorConfig::Eq { bands }, ProcessorState::Eq(states)) => {
                let mut output = sample;
                for (band, state) in bands.iter().zip(states) {
                    let coefficients = BiquadCoefficients::for_band(
                        band.kind,
                        band.frequency.value(controls),
                        band.gain.value(controls),
                        band.q.value(controls),
                        self.program.sample_rate,
                    );
                    output = state.process(output, coefficients);
                }
                output
            }
            (
                ProcessorConfig::Compressor {
                    threshold,
                    ratio,
                    attack,
                    release,
                    knee,
                    makeup,
                    mix,
                    rms,
                },
                ProcessorState::Compressor(state),
            ) => state.process(
                sample,
                sidechain,
                threshold.value(controls),
                ratio.value(controls),
                attack.value(controls),
                release.value(controls),
                knee.value(controls),
                makeup.value(controls),
                mix.value(controls),
                *rms,
                self.program.sample_rate,
            ),
            (
                ProcessorConfig::Gate {
                    mode,
                    threshold,
                    ratio,
                    attack,
                    hold,
                    release,
                    range,
                    hysteresis,
                    rms,
                    mix,
                },
                ProcessorState::Gate(state),
            ) => state.process(
                sample,
                sidechain,
                *mode,
                threshold.value(controls),
                ratio.value(controls),
                attack.value(controls),
                hold.value(controls),
                release.value(controls),
                range.value(controls),
                hysteresis.value(controls),
                *rms,
                mix.value(controls),
                self.program.sample_rate,
            ),
            (
                ProcessorConfig::Delay {
                    time,
                    feedback,
                    mix,
                    low_pass,
                    high_pass,
                    time_crossfade,
                    mode,
                    maximum_frames,
                },
                ProcessorState::Delay(state),
            ) => state.process(
                sample,
                time.value(controls),
                feedback.value(controls),
                mix.value(controls),
                low_pass.value(controls),
                high_pass.value(controls),
                time_crossfade.value(controls),
                *mode,
                *maximum_frames,
                self.program.sample_rate,
            ),
            (
                ProcessorConfig::Reverb {
                    pre_delay,
                    diffusion,
                    decay,
                    damping,
                    modulation,
                    width,
                    mix,
                },
                ProcessorState::Reverb(state),
            ) => state.process(
                sample,
                pre_delay.value(controls),
                diffusion.value(controls),
                decay.value(controls),
                damping.value(controls),
                modulation.value(controls),
                width.value(controls),
                mix.value(controls),
                self.program.sample_rate,
            ),
            (
                ProcessorConfig::Saturation {
                    drive,
                    mix,
                    output,
                    mode,
                },
                ProcessorState::Saturation(state),
            ) => state.process(
                sample,
                drive.value(controls),
                mix.value(controls),
                output.value(controls),
                *mode,
            ),
            (
                ProcessorConfig::ModulationDelay {
                    delay,
                    depth,
                    rate,
                    feedback,
                    mix,
                    width,
                    mode,
                    ..
                },
                ProcessorState::ModulationDelay(state),
            ) => state.process(
                sample,
                delay.value(controls),
                depth.value(controls),
                rate.value(controls),
                feedback.value(controls),
                mix.value(controls),
                width.value(controls),
                *mode,
                self.program.sample_rate,
            ),
            (
                ProcessorConfig::Crusher {
                    bit_depth,
                    sample_rate,
                    reconstruction,
                    mix,
                    output,
                },
                ProcessorState::Crusher(state),
            ) => state.process(
                sample,
                bit_depth.value(controls),
                sample_rate.value(controls),
                reconstruction.value(controls),
                mix.value(controls),
                output.value(controls),
                self.program.sample_rate,
            ),
            (
                ProcessorConfig::Stutter {
                    trigger,
                    chance,
                    crossfade,
                    mix,
                    repeats,
                    interval_frames,
                    seed,
                    ..
                },
                ProcessorState::Stutter(state),
            ) => state.process(
                sample,
                trigger.value(controls),
                chance.value(controls),
                crossfade.value(controls),
                mix.value(controls),
                self.transport_frame,
                *repeats,
                *interval_frames,
                *seed,
            ),
            (
                ProcessorConfig::Limiter {
                    ceiling, release, ..
                },
                ProcessorState::Limiter(state),
            ) => state.process(
                sample,
                db_gain(ceiling.value(controls)).max(1.0e-6),
                release.value(controls),
                self.program.sample_rate,
            ),
            (ProcessorConfig::Latency, ProcessorState::Latency) => sample,
            _ => sample,
        };
        let wet = self.declared_delay.process(processed);
        let authored_bypass = self.program.bypass.value(controls) >= 0.5;
        let effective_bypass = if audition {
            self.transient_bypass.unwrap_or(authored_bypass)
        } else {
            authored_bypass
        };
        let target = if effective_bypass { 1.0 } else { 0.0 };
        if !audition || !self.transient_bypass_guard_active {
            self.bypass_mix = target;
            self.transient_bypass_guard_active = false;
        } else if self.bypass_mix < target {
            self.bypass_mix = (self.bypass_mix + 1.0 / BYPASS_GUARD_FRAMES).min(target);
            self.transient_bypass_guard_active = self.bypass_mix < target;
        } else if self.bypass_mix > target {
            self.bypass_mix = (self.bypass_mix - 1.0 / BYPASS_GUARD_FRAMES).max(target);
            self.transient_bypass_guard_active = self.bypass_mix > target;
        } else {
            self.transient_bypass_guard_active = false;
        }
        let output = if self.bypass_mix <= 0.0 {
            wet
        } else if self.bypass_mix >= 1.0 {
            dry
        } else {
            [
                wet[0] + (dry[0] - wet[0]) * self.bypass_mix,
                wet[1] + (dry[1] - wet[1]) * self.bypass_mix,
            ]
        };
        self.transport_frame = self.transport_frame.saturating_add(1);
        output
    }

    pub(crate) fn set_transport_frame(&mut self, frame: u64) {
        self.transport_frame = frame;
    }

    pub(crate) fn id(&self) -> &str {
        &self.program.id
    }

    pub(crate) fn set_transient_bypass(&mut self, bypass: Option<bool>) {
        if self.transient_bypass != bypass {
            self.transient_bypass = bypass;
            self.transient_bypass_guard_active = true;
        }
    }

    pub(crate) fn bypass_state(
        &self,
        controls: &[f64],
        audition: bool,
    ) -> (bool, Option<bool>, bool, f32) {
        let authored = self.program.bypass.value(controls) >= 0.5;
        let effective = if audition {
            self.transient_bypass.unwrap_or(authored)
        } else {
            authored
        };
        (authored, self.transient_bypass, effective, self.bypass_mix)
    }

    pub(crate) fn reset(&mut self) {
        match &mut self.state {
            ProcessorState::Utility | ProcessorState::Latency => {}
            ProcessorState::Eq(states) => states.fill(StereoBiquad::default()),
            ProcessorState::Compressor(state) => state.reset(),
            ProcessorState::Gate(state) => state.reset(),
            ProcessorState::Delay(state) => state.reset(),
            ProcessorState::Reverb(state) => state.reset(),
            ProcessorState::Saturation(state) => state.reset(),
            ProcessorState::ModulationDelay(state) => state.reset(),
            ProcessorState::Crusher(state) => state.reset(),
            ProcessorState::Stutter(state) => state.reset(),
            ProcessorState::Limiter(state) => state.reset(),
        }
        self.transport_frame = 0;
        self.declared_delay.reset();
        self.bypass_delay.reset();
        let bypass = self
            .transient_bypass
            .unwrap_or(self.program.authored_bypass);
        self.bypass_mix = if bypass { 1.0 } else { 0.0 };
        self.transient_bypass_guard_active = false;
    }

    #[cfg(test)]
    pub(crate) fn heap_storage_fingerprint(&self) -> u64 {
        fn include(seed: u64, pointer: *const (), capacity: usize) -> u64 {
            seed.rotate_left(7) ^ (pointer as usize as u64) ^ (capacity as u64).rotate_left(31)
        }
        let mut fingerprint = include(
            0xCBF2_9CE4_8422_2325,
            self.declared_delay.buffer.as_ptr().cast(),
            self.declared_delay.buffer.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.bypass_delay.buffer.as_ptr().cast(),
            self.bypass_delay.buffer.capacity(),
        );
        match &self.state {
            ProcessorState::Eq(states) => {
                fingerprint = include(fingerprint, states.as_ptr().cast(), states.capacity());
            }
            ProcessorState::Delay(state) => {
                fingerprint = include(
                    fingerprint,
                    state.buffer.as_ptr().cast(),
                    state.buffer.capacity(),
                );
            }
            ProcessorState::Reverb(state) => {
                fingerprint = include(
                    fingerprint,
                    state.pre_delay.buffer.as_ptr().cast(),
                    state.pre_delay.buffer.capacity(),
                );
                for delay in state.left_combs.iter().chain(&state.right_combs) {
                    fingerprint = include(
                        fingerprint,
                        delay.buffer.as_ptr().cast(),
                        delay.buffer.capacity(),
                    );
                }
                for delay in state.left_diffusers.iter().chain(&state.right_diffusers) {
                    fingerprint = include(
                        fingerprint,
                        delay.buffer.as_ptr().cast(),
                        delay.buffer.capacity(),
                    );
                }
            }
            ProcessorState::Saturation(state) => {
                fingerprint = include(
                    fingerprint,
                    state.dry_delay.buffer.as_ptr().cast(),
                    state.dry_delay.buffer.capacity(),
                );
            }
            ProcessorState::Stutter(state) => {
                fingerprint = include(
                    fingerprint,
                    state.buffer.as_ptr().cast(),
                    state.buffer.capacity(),
                );
            }
            ProcessorState::ModulationDelay(state) => {
                fingerprint = include(
                    fingerprint,
                    state.buffer.as_ptr().cast(),
                    state.buffer.capacity(),
                );
            }
            ProcessorState::Limiter(state) => {
                fingerprint = include(
                    fingerprint,
                    state.audio_buffer.as_ptr().cast(),
                    state.audio_buffer.capacity(),
                );
                fingerprint = include(
                    fingerprint,
                    state.peak_buffer.as_ptr().cast(),
                    state.peak_buffer.capacity(),
                );
            }
            ProcessorState::Utility
            | ProcessorState::Compressor(_)
            | ProcessorState::Gate(_)
            | ProcessorState::Crusher(_)
            | ProcessorState::Latency => {}
        }
        fingerprint
    }

    pub(crate) fn stutter_status(&self) -> Option<(bool, u64)> {
        match &self.state {
            ProcessorState::Stutter(state) => Some((state.repeating(), state.ignored_triggers())),
            _ => None,
        }
    }

    pub(crate) fn gain_reduction_db(&self) -> Option<f32> {
        match &self.state {
            ProcessorState::Compressor(state) => Some(state.gain_reduction_db()),
            ProcessorState::Gate(state) => Some(state.gain_reduction_db()),
            _ => None,
        }
    }
}
