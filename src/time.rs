//! Exact authored musical time and deterministic sample conversion.

use std::{cmp::Ordering, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// An exact rational number of quarter notes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MusicalTime {
    numerator: i64,
    denominator: i64,
}

impl MusicalTime {
    /// Musical time zero.
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };

    /// Creates a normalized rational musical time.
    pub fn new(numerator: i64, denominator: i64) -> Result<Self, TimeError> {
        if denominator == 0 {
            return Err(TimeError::ZeroDenominator);
        }
        let sign = if denominator < 0 { -1 } else { 1 };
        let numerator = numerator.checked_mul(sign).ok_or(TimeError::Overflow)?;
        let denominator = denominator.checked_mul(sign).ok_or(TimeError::Overflow)?;
        let divisor = gcd(numerator.unsigned_abs(), denominator.unsigned_abs()) as i64;
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    /// Creates a whole number of quarter notes.
    #[must_use]
    pub const fn whole(quarter_notes: i64) -> Self {
        Self {
            numerator: quarter_notes,
            denominator: 1,
        }
    }

    /// Returns the normalized numerator.
    #[must_use]
    pub const fn numerator(self) -> i64 {
        self.numerator
    }

    /// Returns the positive normalized denominator.
    #[must_use]
    pub const fn denominator(self) -> i64 {
        self.denominator
    }

    /// Returns true when this time is greater than zero.
    #[must_use]
    pub const fn is_positive(self) -> bool {
        self.numerator > 0
    }

    /// Converts this musical time to floating-point quarter notes.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    /// Converts this time to a deterministic frame position at fixed tempo.
    ///
    /// Exact halfway values round toward positive infinity. All authored times are
    /// non-negative at this seam, so the policy is stable and unambiguous.
    pub fn to_frames(self, tempo_bpm: f64, sample_rate: u32) -> Result<u64, TimeError> {
        if !tempo_bpm.is_finite() || tempo_bpm <= 0.0 {
            return Err(TimeError::InvalidTempo);
        }
        let frames = self.as_f64() * 60.0 * f64::from(sample_rate) / tempo_bpm;
        if !frames.is_finite() || frames < 0.0 || frames > u64::MAX as f64 {
            return Err(TimeError::Overflow);
        }
        Ok((frames + 0.5).floor() as u64)
    }

    /// Checked addition.
    pub fn checked_add(self, rhs: Self) -> Result<Self, TimeError> {
        let lhs = self
            .numerator
            .checked_mul(rhs.denominator)
            .ok_or(TimeError::Overflow)?;
        let rhs_n = rhs
            .numerator
            .checked_mul(self.denominator)
            .ok_or(TimeError::Overflow)?;
        let numerator = lhs.checked_add(rhs_n).ok_or(TimeError::Overflow)?;
        let denominator = self
            .denominator
            .checked_mul(rhs.denominator)
            .ok_or(TimeError::Overflow)?;
        Self::new(numerator, denominator)
    }

    /// Checked subtraction.
    pub fn checked_sub(self, rhs: Self) -> Result<Self, TimeError> {
        let lhs = self
            .numerator
            .checked_mul(rhs.denominator)
            .ok_or(TimeError::Overflow)?;
        let rhs_n = rhs
            .numerator
            .checked_mul(self.denominator)
            .ok_or(TimeError::Overflow)?;
        let numerator = lhs.checked_sub(rhs_n).ok_or(TimeError::Overflow)?;
        let denominator = self
            .denominator
            .checked_mul(rhs.denominator)
            .ok_or(TimeError::Overflow)?;
        Self::new(numerator, denominator)
    }

    /// Checked multiplication by a non-negative integer.
    pub fn checked_mul(self, rhs: u64) -> Result<Self, TimeError> {
        let rhs = i64::try_from(rhs).map_err(|_| TimeError::Overflow)?;
        Self::new(
            self.numerator.checked_mul(rhs).ok_or(TimeError::Overflow)?,
            self.denominator,
        )
    }
}

impl Ord for MusicalTime {
    fn cmp(&self, other: &Self) -> Ordering {
        let lhs = i128::from(self.numerator) * i128::from(other.denominator);
        let rhs = i128::from(other.numerator) * i128::from(self.denominator);
        lhs.cmp(&rhs)
    }
}

impl PartialOrd for MusicalTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for MusicalTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.denominator == 1 {
            write!(formatter, "{}qn", self.numerator)
        } else {
            write!(formatter, "{}/{}qn", self.numerator, self.denominator)
        }
    }
}

/// A fixed meter used to interpret authored bars.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meter {
    /// Beats in each bar.
    pub numerator: u16,
    /// Beat-note denominator, which must be a power of two.
    pub denominator: u16,
}

impl Meter {
    /// Creates a validated fixed meter.
    pub fn new(numerator: u16, denominator: u16) -> Result<Self, TimeError> {
        if numerator == 0 || denominator == 0 || !denominator.is_power_of_two() {
            return Err(TimeError::InvalidMeter);
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    /// Returns the exact quarter-note length of one bar.
    pub fn bar_length(self) -> Result<MusicalTime, TimeError> {
        MusicalTime::new(i64::from(self.numerator) * 4, i64::from(self.denominator))
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

impl fmt::Display for Meter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.numerator, self.denominator)
    }
}

impl FromStr for Meter {
    type Err = TimeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (numerator, denominator) = value.split_once('/').ok_or(TimeError::InvalidMeter)?;
        Self::new(
            numerator.parse().map_err(|_| TimeError::InvalidMeter)?,
            denominator.parse().map_err(|_| TimeError::InvalidMeter)?,
        )
    }
}

/// Parses an authored duration such as `2bar`, `1/2beat`, or `3qn`.
pub fn parse_musical_time(value: &str, meter: Meter) -> Result<MusicalTime, TimeError> {
    let (number, unit) = if let Some(number) = value.strip_suffix("bars") {
        (number, "bar")
    } else if let Some(number) = value.strip_suffix("bar") {
        (number, "bar")
    } else if let Some(number) = value.strip_suffix("beats") {
        (number, "beat")
    } else if let Some(number) = value.strip_suffix("beat") {
        (number, "beat")
    } else if let Some(number) = value.strip_suffix("qn") {
        (number, "beat")
    } else {
        (value, "beat")
    };

    let scalar = parse_rational(number)?;
    let unit_length = match unit {
        "bar" => meter.bar_length()?,
        _ => MusicalTime::whole(1),
    };
    multiply_rationals(scalar, unit_length)
}

/// Parses seconds from `2s`, `250ms`, or a bare seconds value.
pub fn parse_seconds(value: &str) -> Result<f64, TimeError> {
    let seconds = if let Some(milliseconds) = value.strip_suffix("ms") {
        milliseconds
            .parse::<f64>()
            .map_err(|_| TimeError::InvalidDuration)?
            / 1_000.0
    } else if let Some(seconds) = value.strip_suffix('s') {
        seconds
            .parse::<f64>()
            .map_err(|_| TimeError::InvalidDuration)?
    } else {
        value
            .parse::<f64>()
            .map_err(|_| TimeError::InvalidDuration)?
    };
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(TimeError::InvalidDuration);
    }
    Ok(seconds)
}

fn parse_rational(value: &str) -> Result<MusicalTime, TimeError> {
    if let Some((numerator, denominator)) = value.split_once('/') {
        return MusicalTime::new(
            numerator.parse().map_err(|_| TimeError::InvalidDuration)?,
            denominator
                .parse()
                .map_err(|_| TimeError::InvalidDuration)?,
        );
    }
    if let Ok(integer) = value.parse::<i64>() {
        return MusicalTime::new(integer, 1);
    }

    let decimal = value
        .parse::<f64>()
        .map_err(|_| TimeError::InvalidDuration)?;
    if !decimal.is_finite() || decimal < 0.0 {
        return Err(TimeError::InvalidDuration);
    }
    // Decimal spellings are converted through their source digits, not through a
    // binary float, so common values remain exact.
    let (whole, fraction) = value.split_once('.').ok_or(TimeError::InvalidDuration)?;
    let scale = 10_i64
        .checked_pow(u32::try_from(fraction.len()).map_err(|_| TimeError::Overflow)?)
        .ok_or(TimeError::Overflow)?;
    let whole: i64 = whole.parse().map_err(|_| TimeError::InvalidDuration)?;
    let fraction: i64 = fraction.parse().map_err(|_| TimeError::InvalidDuration)?;
    MusicalTime::new(
        whole
            .checked_mul(scale)
            .and_then(|n| n.checked_add(fraction))
            .ok_or(TimeError::Overflow)?,
        scale,
    )
}

fn multiply_rationals(lhs: MusicalTime, rhs: MusicalTime) -> Result<MusicalTime, TimeError> {
    MusicalTime::new(
        lhs.numerator
            .checked_mul(rhs.numerator)
            .ok_or(TimeError::Overflow)?,
        lhs.denominator
            .checked_mul(rhs.denominator)
            .ok_or(TimeError::Overflow)?,
    )
}

const fn gcd(mut lhs: u64, mut rhs: u64) -> u64 {
    while rhs != 0 {
        let remainder = lhs % rhs;
        lhs = rhs;
        rhs = remainder;
    }
    if lhs == 0 {
        1
    } else {
        lhs
    }
}

/// Errors produced while interpreting authored time.
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum TimeError {
    /// A rational denominator was zero.
    #[error("time denominator must not be zero")]
    ZeroDenominator,
    /// The fixed tempo was not finite and positive.
    #[error("tempo must be finite and greater than zero")]
    InvalidTempo,
    /// Meter was not positive with a power-of-two denominator.
    #[error("meter must have a positive numerator and a power-of-two denominator")]
    InvalidMeter,
    /// A duration could not be parsed or was negative/non-finite.
    #[error("duration must be a non-negative number with a supported unit")]
    InvalidDuration,
    /// Arithmetic exceeded the supported authored range.
    #[error("authored time exceeds the supported range")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_rationals() {
        assert_eq!(
            MusicalTime::new(6, 8).unwrap(),
            MusicalTime::new(3, 4).unwrap()
        );
        assert_eq!(MusicalTime::new(1, -2).unwrap().numerator(), -1);
    }

    #[test]
    fn parses_bar_and_beat_units_exactly() {
        let meter = Meter::default();
        assert_eq!(
            parse_musical_time("2bar", meter).unwrap(),
            MusicalTime::whole(8)
        );
        assert_eq!(
            parse_musical_time("1/2beat", meter).unwrap(),
            MusicalTime::new(1, 2).unwrap()
        );
        assert_eq!(
            parse_musical_time("0.25beat", meter).unwrap(),
            MusicalTime::new(1, 4).unwrap()
        );
    }

    #[test]
    fn rounds_sample_positions_deterministically() {
        assert_eq!(
            MusicalTime::whole(1).to_frames(120.0, 48_000).unwrap(),
            24_000
        );
    }
}
