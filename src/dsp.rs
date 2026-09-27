//! Small shared DSP primitives used by native Nodes and offline analysis probes.

use std::f64::consts::PI;

/// Allocation-free state-variable filter; coefficients follow the native Filter Node.
#[derive(Debug, Clone, Copy, Default)]
pub struct StateVariableFilter {
    integrator_one: f64,
    integrator_two: f64,
}

/// Simultaneous responses of a state-variable filter.
#[derive(Debug, Clone, Copy)]
pub struct FilterOutputs {
    /// Low-frequency response.
    pub lowpass: f64,
    /// High-frequency response.
    pub highpass: f64,
    /// Band centered on the cutoff.
    pub bandpass: f64,
    /// Sum of lowpass and highpass responses.
    pub notch: f64,
}

impl StateVariableFilter {
    /// Process a sample. Callers provide a finite positive rate and a cutoff
    /// within the native Filter domain; resonance is defensively bounded.
    pub fn process(
        &mut self,
        input: f64,
        cutoff: f64,
        resonance: f64,
        sample_rate: f64,
    ) -> FilterOutputs {
        let g = (PI * cutoff / sample_rate).tan().min(10.0);
        let damping = 2.0 - 1.9 * resonance.clamp(0.0, 1.0);
        let denominator = 1.0 + g * (g + damping);
        let bandpass = (self.integrator_one + g * (input - self.integrator_two)) / denominator;
        let lowpass = self.integrator_two + g * bandpass;
        let highpass = input - damping * bandpass - lowpass;
        self.integrator_one = 2.0 * bandpass - self.integrator_one;
        self.integrator_two = 2.0 * lowpass - self.integrator_two;
        FilterOutputs {
            lowpass,
            highpass,
            bandpass,
            notch: lowpass + highpass,
        }
    }
}
