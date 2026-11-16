//! Offline adapter for the existing native Exciter -> band-pass Filter path.
//! JSON on stdin; 48 kHz mono little-endian float32 on stdout.
use crump::acoustics::{Excitation, Exciter};
use crump::dsp::StateVariableFilter;
use serde::Deserialize;
use std::io::{Read, Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    excitation: Excitation,
    frequency_hz: f64,
    resonance: f64,
    #[serde(default)]
    highpass_hz: f64,
    gain: f64,
    velocity: f64,
    seed: u64,
    frames: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut json = String::new();
    std::io::stdin().take(8192).read_to_string(&mut json)?;
    let r: Request = serde_json::from_str(&json)?;
    r.excitation.validate()?;
    if !r.frequency_hz.is_finite()
        || !(20.0..=20000.0).contains(&r.frequency_hz)
        || !r.resonance.is_finite()
        || !(0.0..=1.0).contains(&r.resonance)
        || !r.highpass_hz.is_finite()
        || !(0.0..=20000.0).contains(&r.highpass_hz)
        || (r.highpass_hz > 0.0 && r.highpass_hz < 20.0)
        || !r.gain.is_finite()
        || !(0.0..=4.0).contains(&r.gain)
        || !r.velocity.is_finite()
        || !(0.0..=1.0).contains(&r.velocity)
        || r.frames > 48000 * 5
    {
        return Err("invalid filter-impact probe domain".into());
    }
    let mut exciter = Exciter::new(48000, r.seed);
    let mut filter = StateVariableFilter::default();
    let mut lowcut = [StateVariableFilter::default(); 2];
    exciter.trigger(r.excitation, r.velocity);
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    for _ in 0..r.frames {
        let force = (exciter.process() * r.gain) as f32;
        let mut y = filter
            .process(f64::from(force), r.frequency_hz, r.resonance, 48000.0)
            .bandpass as f32;
        if r.highpass_hz > 0.0 {
            for filter in &mut lowcut {
                y = filter
                    .process(f64::from(y), r.highpass_hz, 0.0, 48000.0)
                    .highpass as f32;
            }
        }
        if !y.is_finite() {
            return Err("non-finite filter-impact response".into());
        }
        output.write_all(&y.to_le_bytes())?;
    }
    output.flush()?;
    Ok(())
}
