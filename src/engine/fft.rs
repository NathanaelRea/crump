//! Deterministic in-place radix-2 transform for native DSP.

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Complex {
    pub(super) re: f64,
    pub(super) im: f64,
}

pub(super) fn fft(values: &mut [Complex], inverse: bool) {
    let length = values.len();
    debug_assert!(length.is_power_of_two());
    let mut target = 0_usize;
    for source in 1..length {
        let mut bit = length >> 1;
        while target & bit != 0 {
            target ^= bit;
            bit >>= 1;
        }
        target ^= bit;
        if source < target {
            values.swap(source, target);
        }
    }
    let mut width = 2;
    while width <= length {
        let angle = if inverse {
            std::f64::consts::TAU / width as f64
        } else {
            -std::f64::consts::TAU / width as f64
        };
        let root = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..length).step_by(width) {
            let mut rotation = Complex { re: 1.0, im: 0.0 };
            for offset in 0..width / 2 {
                let even = values[start + offset];
                let odd = multiply(values[start + offset + width / 2], rotation);
                values[start + offset] = Complex {
                    re: even.re + odd.re,
                    im: even.im + odd.im,
                };
                values[start + offset + width / 2] = Complex {
                    re: even.re - odd.re,
                    im: even.im - odd.im,
                };
                rotation = multiply(rotation, root);
            }
        }
        width *= 2;
    }
    if inverse {
        for value in values {
            value.re /= length as f64;
            value.im /= length as f64;
        }
    }
}

pub(super) const fn multiply(left: Complex, right: Complex) -> Complex {
    Complex {
        re: left.re * right.re - left.im * right.im,
        im: left.re * right.im + left.im * right.re,
    }
}
