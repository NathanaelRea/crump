//! Offline fitting adapter: JSON request on stdin, mono little-endian f32 on stdout.
//! Uses exactly the shared native acoustic DSP, with graph-equivalent f32 force input.
use crump::acoustics::{Excitation, Exciter, ModalBank, Profile};
use crump::dsp::StateVariableFilter;
use serde::Deserialize;
use std::io::{Read, Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    profile: Profile,
    excitation: Excitation,
    frames: usize,
    velocity: f64,
    pitch_ratio: f64,
    decay_scale: f64,
    brightness: f64,
    #[serde(default)]
    rise_seconds: f64,
    #[serde(default)]
    highpass_hz: f64,
    #[serde(default)]
    highpass_stages: usize,
    #[serde(default)]
    lowpass_hz: f64,
    #[serde(default)]
    lowpass_stages: usize,
    seed: u64,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut json = String::new();
    std::io::stdin().take(262144).read_to_string(&mut json)?;
    let r: Request = serde_json::from_str(&json)?;
    r.profile.validate()?;
    r.excitation.validate()?;
    if r.frames > 48000 * 30
        || !r.velocity.is_finite()
        || !(0.0..=1.0).contains(&r.velocity)
        || !r.pitch_ratio.is_finite()
        || !(0.03125..=32.0).contains(&r.pitch_ratio)
        || !r.decay_scale.is_finite()
        || !(0.1..=4.0).contains(&r.decay_scale)
        || !r.brightness.is_finite()
        || !(-2.0..=2.0).contains(&r.brightness)
        || !r.rise_seconds.is_finite()
        || !(0.0..=1.0).contains(&r.rise_seconds)
        || !r.highpass_hz.is_finite()
        || !(0.0..=20000.0).contains(&r.highpass_hz)
        || r.highpass_stages > 2
        || (r.highpass_stages > 0 && r.highpass_hz < 20.0)
        || !r.lowpass_hz.is_finite()
        || !(0.0..=20000.0).contains(&r.lowpass_hz)
        || r.lowpass_stages > 2
        || (r.lowpass_stages > 0 && r.lowpass_hz < 20.0)
    {
        return Err("invalid probe domain".into());
    }
    let mut exciter = Exciter::new(48000, r.seed);
    let mut bank = ModalBank::new(r.profile.modes.len(), 48000);
    exciter.trigger(r.excitation, r.velocity);
    bank.trigger_with_rise(
        &r.profile,
        r.pitch_ratio,
        r.decay_scale,
        r.brightness,
        r.rise_seconds,
    );
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    let mut filters = [StateVariableFilter::default(); 2];
    let mut lowpass = [StateVariableFilter::default(); 2];
    for _ in 0..r.frames {
        let mut y = bank.process(exciter.process() as f32 as f64) as f32;
        for filter in filters.iter_mut().take(r.highpass_stages) {
            y = filter
                .process(f64::from(y), r.highpass_hz, 0.0, 48000.0)
                .highpass as f32;
        }
        for filter in lowpass.iter_mut().take(r.lowpass_stages) {
            y = filter
                .process(f64::from(y), r.lowpass_hz, 0.0, 48000.0)
                .lowpass as f32;
        }
        if !y.is_finite() {
            return Err("non-finite response".into());
        }
        output.write_all(&y.to_le_bytes())?;
    }
    output.flush()?;
    Ok(())
}
