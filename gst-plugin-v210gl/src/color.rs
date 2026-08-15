//! YCbCr ↔ RGB conversion matching the GLSL used by the GL elements.

/// Which matrix to apply. `Auto` is treated as BT.709 (the HD default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Matrix {
    Bt601,
    #[default]
    Bt709,
    Bt2020,
}

impl Matrix {
    pub fn parse(s: &str) -> Self {
        match s {
            "bt601" => Self::Bt601,
            "bt2020" => Self::Bt2020,
            _ => Self::Bt709,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bt601 => "bt601",
            Self::Bt709 => "bt709",
            Self::Bt2020 => "bt2020",
        }
    }

    /// RGB → Y' coefficients.
    pub fn rgb_to_y(self) -> [f32; 3] {
        match self {
            Self::Bt601 => [0.299, 0.587, 0.114],
            Self::Bt709 => [0.2126, 0.7152, 0.0722],
            Self::Bt2020 => [0.2627, 0.6780, 0.0593],
        }
    }

    /// Kr / Kb used to recover Cb/Cr scale factors.
    pub fn kr_kb(self) -> (f32, f32) {
        match self {
            Self::Bt601 => (0.299, 0.114),
            Self::Bt709 => (0.2126, 0.0722),
            Self::Bt2020 => (0.2627, 0.0593),
        }
    }

    /// Multipliers applied to chroma-offset Cb'/Cr' to recover B-Y' and R-Y'.
    pub fn ycbcr_to_rgb_chroma_scale(self) -> (f32, f32) {
        let (kr, kb) = self.kr_kb();
        (2.0 * (1.0 - kb), 2.0 * (1.0 - kr))
    }
}

/// Convert 10-bit YCbCr to 8-bit-ish RGB in 0..1 (the mixer path is 8-bit).
#[cfg_attr(not(test), allow(dead_code))]
pub fn ycbcr_to_rgb(y: u16, cb: u16, cr: u16, matrix: Matrix, full_range: bool) -> [f32; 3] {
    let (y_n, cb_n, cr_n) = if full_range {
        (
            y as f32 / 1023.0,
            (cb as f32 - 512.0) / 512.0,
            (cr as f32 - 512.0) / 512.0,
        )
    } else {
        (
            (y as f32 - 64.0) / 876.0,
            (cb as f32 - 512.0) / 896.0,
            (cr as f32 - 512.0) / 896.0,
        )
    };

    let (cb_scale, cr_scale) = matrix.ycbcr_to_rgb_chroma_scale();
    let (kr, kb) = matrix.kr_kb();
    let kg = 1.0 - kr - kb;

    let r = y_n + cr_n * cr_scale;
    let b = y_n + cb_n * cb_scale;
    let g = y_n - (kb * cb_scale / kg) * cb_n - (kr * cr_scale / kg) * cr_n;
    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
}

/// Convert 0..1 RGB to 10-bit YCbCr.
#[cfg_attr(not(test), allow(dead_code))]
pub fn rgb_to_ycbcr(rgb: [f32; 3], matrix: Matrix, full_range: bool) -> (u16, u16, u16) {
    let [r, g, b] = rgb;
    let y_coeff = matrix.rgb_to_y();
    let y_n = y_coeff[0] * r + y_coeff[1] * g + y_coeff[2] * b;
    let (cb_scale, cr_scale) = matrix.ycbcr_to_rgb_chroma_scale();
    let cb_n = (b - y_n) / cb_scale;
    let cr_n = (r - y_n) / cr_scale;

    let (y, cb, cr) = if full_range {
        (y_n * 1023.0, cb_n * 512.0 + 512.0, cr_n * 512.0 + 512.0)
    } else {
        (
            y_n * 876.0 + 64.0,
            cb_n * 896.0 + 512.0,
            cr_n * 896.0 + 512.0,
        )
    };

    (
        y.round().clamp(0.0, 1023.0) as u16,
        cb.round().clamp(0.0, 1023.0) as u16,
        cr.round().clamp(0.0, 1023.0) as u16,
    )
}

/// Guess a matrix from a GStreamer `colorimetry` field. Unknown → BT.709.
pub fn matrix_from_colorimetry(colorimetry: &str) -> Matrix {
    let s = colorimetry.to_ascii_lowercase();
    if s.contains("bt601") || s.contains("smpte170m") || s.contains("bt470") {
        Matrix::Bt601
    } else if s.contains("bt2020") || s.contains("bt2100") {
        Matrix::Bt2020
    } else {
        Matrix::Bt709
    }
}

/// Guess limited vs full from a GStreamer `colorimetry` field.
pub fn full_range_from_colorimetry(colorimetry: &str) -> bool {
    let s = colorimetry.to_ascii_lowercase();
    s.contains("1:") || s.contains("full")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt709_limited_black_and_white() {
        let black = ycbcr_to_rgb(64, 512, 512, Matrix::Bt709, false);
        assert!(black[0] < 0.01 && black[1] < 0.01 && black[2] < 0.01);

        let white = ycbcr_to_rgb(940, 512, 512, Matrix::Bt709, false);
        assert!(white[0] > 0.99 && white[1] > 0.99 && white[2] > 0.99);
    }

    #[test]
    fn bt709_full_black_and_white() {
        let black = ycbcr_to_rgb(0, 512, 512, Matrix::Bt709, true);
        assert!(black[0] < 0.01 && black[1] < 0.01 && black[2] < 0.01);

        let white = ycbcr_to_rgb(1023, 512, 512, Matrix::Bt709, true);
        assert!(white[0] > 0.99 && white[1] > 0.99 && white[2] > 0.99);
    }

    #[test]
    fn bt601_limited_black() {
        let black = ycbcr_to_rgb(64, 512, 512, Matrix::Bt601, false);
        assert!(black.iter().all(|c| *c < 0.01));
    }

    #[test]
    fn rgb_roundtrip_neutral_gray() {
        let rgb = [0.5, 0.5, 0.5];
        let (y, cb, cr) = rgb_to_ycbcr(rgb, Matrix::Bt709, false);
        assert!((cb as i32 - 512).abs() <= 1);
        assert!((cr as i32 - 512).abs() <= 1);
        let back = ycbcr_to_rgb(y, cb, cr, Matrix::Bt709, false);
        for i in 0..3 {
            assert!((back[i] - rgb[i]).abs() < 0.01, "{back:?} vs {rgb:?}");
        }
    }
}
