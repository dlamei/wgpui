use std::fmt;

pub fn rand_f32() -> f32 {
    static mut SEED: u32 = 123456789;
    unsafe {
        SEED = SEED.wrapping_mul(1664525).wrapping_add(1013904223);
        (SEED & 0x00FFFFFF) as f32 / 0x01000000 as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct RGBA {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl fmt::Display for RGBA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.a == 1.0 {
            write!(f, "({:.2}, {:.2}, {:.2})", self.r, self.g, self.b)
        } else {
            write!(
                f,
                "({:.2}, {:.2}, {:.2}, {:.2})",
                self.r, self.g, self.b, self.a
            )
        }
    }
}

impl RGBA {
    pub fn rand() -> Self {
        Self {
            r: rand_f32(),
            g: rand_f32(),
            b: rand_f32(),
            a: 1.0,
        }
    }

    pub fn rand_w_alpha() -> Self {
        Self {
            r: rand_f32(),
            g: rand_f32(),
            b: rand_f32(),
            a: rand_f32(),
        }
    }

    pub fn as_wgsl_vec4(&self) -> String {
        format!("vec4<f32>({},{},{},{})", self.r, self.g, self.b, self.a)
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba_f(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::rgba_f(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    pub const fn rgb_f(r: f32, g: f32, b: f32) -> Self {
        Self::rgba_f(r, g, b, 1.0)
    }

    pub const fn rgba_f(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    fn srgb_to_linear_u8(u: u8) -> f32 {
        let srgb = u as f32 / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    }

    fn linear_to_srgb(l: f32) -> f32 {
        if l <= 0.0031308 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        }
    }

    pub fn map_linear_to_srgb(&self) -> Self {
        let r = Self::linear_to_srgb(self.r);
        let g = Self::linear_to_srgb(self.g);
        let b = Self::linear_to_srgb(self.b);
        let a = self.a;
        Self::rgba_f(r, g, b, a)
    }

    pub fn hex(hex: &str) -> Self {
        let hex = hex.trim_start_matches('#');
        let vals: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();

        let (r8, g8, b8, a8) = match vals.as_slice() {
            [r, g, b] => (*r, *g, *b, 255),
            [r, g, b, a] => (*r, *g, *b, *a),
            _ => panic!("Hex code must be 6 or 8 characters long"),
        };

        Self::rgba_f(
            Self::srgb_to_linear_u8(r8),
            Self::srgb_to_linear_u8(g8),
            Self::srgb_to_linear_u8(b8),
            a8 as f32 / 255.0,
        )
        .map_linear_to_srgb()
    }

    pub const RED: RGBA = RGBA::rgb(255, 0, 0);
    pub const GREEN: RGBA = RGBA::rgb(0, 255, 0);
    pub const BLUE: RGBA = RGBA::rgb(0, 0, 255);

    pub const WHITE: RGBA = RGBA::rgb(255, 255, 255);
    pub const BLACK: RGBA = RGBA::rgb(0, 0, 0);

    pub const DEBUG: RGBA = RGBA::rgb(200, 0, 100);

    pub const ZERO: RGBA = RGBA::rgba(0, 0, 0, 0);
}

impl From<RGBA> for wgpu::Color {
    fn from(c: RGBA) -> Self {
        wgpu::Color {
            r: c.r as f64,
            g: c.g as f64,
            b: c.b as f64,
            a: c.a as f64,
        }
    }
}

pub fn hex_to_col(hex: &str) -> wgpu::Color {
    fn to_linear(u: u8) -> f64 {
        let srgb = u as f64 / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    }

    let hex = hex.trim_start_matches('#');
    let vals: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();

    let (r8, g8, b8, a8) = match vals.as_slice() {
        [r, g, b] => (*r, *g, *b, 255),
        [r, g, b, a] => (*r, *g, *b, *a),
        _ => panic!("Hex code must be 6 or 8 characters long"),
    };

    wgpu::Color {
        r: to_linear(r8),
        g: to_linear(g8),
        b: to_linear(b8),
        a: a8 as f64 / 255.0, // alpha is linear already
    }
}

impl From<(u8, u8, u8)> for RGBA {
    fn from(v: (u8, u8, u8)) -> Self {
        RGBA::rgb(v.0, v.1, v.2)
    }
}

impl From<(u8, u8, u8, u8)> for RGBA {
    fn from(v: (u8, u8, u8, u8)) -> Self {
        RGBA::rgba(v.0, v.1, v.2, v.3)
    }
}

impl From<[u8; 3]> for RGBA {
    fn from(v: [u8; 3]) -> Self {
        RGBA::rgb(v[0], v[1], v[2])
    }
}

impl From<[u8; 4]> for RGBA {
    fn from(v: [u8; 4]) -> Self {
        RGBA::rgba(v[0], v[1], v[2], v[3])
    }
}

impl From<(f32, f32, f32)> for RGBA {
    fn from(v: (f32, f32, f32)) -> Self {
        RGBA::rgb_f(v.0, v.1, v.2)
    }
}

impl From<(f32, f32, f32, f32)> for RGBA {
    fn from(v: (f32, f32, f32, f32)) -> Self {
        RGBA::rgba_f(v.0, v.1, v.2, v.3)
    }
}

impl From<[f32; 3]> for RGBA {
    fn from(v: [f32; 3]) -> Self {
        RGBA::rgb_f(v[0], v[1], v[2])
    }
}

impl From<[f32; 4]> for RGBA {
    fn from(v: [f32; 4]) -> Self {
        RGBA::rgba_f(v[0], v[1], v[2], v[3])
    }
}

impl From<&str> for RGBA {
    fn from(s: &str) -> Self {
        RGBA::hex(s)
    }
}

impl From<u32> for RGBA {
    /// Interprets `0xRRGGBB` or `0xAARRGGBB`. If value <= 0x00_FF_FF_FF it's treated as RRGGBB (opaque).
    fn from(v: u32) -> Self {
        if v <= 0x00FF_FF_FF {
            let r = ((v >> 16) & 0xFF) as u8;
            let g = ((v >> 8) & 0xFF) as u8;
            let b = (v & 0xFF) as u8;
            RGBA::rgb(r, g, b)
        } else {
            let a = ((v >> 24) & 0xFF) as u8;
            let r = ((v >> 16) & 0xFF) as u8;
            let g = ((v >> 8) & 0xFF) as u8;
            let b = (v & 0xFF) as u8;
            RGBA::rgba(r, g, b, a)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct RGB {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl RGB {
    pub fn to_rgba(&self) -> RGBA {
        RGBA {
            r: self.r,
            g: self.g,
            b: self.b,
            a: 1.0,
        }
    }

    pub fn rand() -> Self {
        Self {
            r: rand_f32(),
            g: rand_f32(),
            b: rand_f32(),
        }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgb_f(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }

    pub const fn rgb_f(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    pub fn hex(hex: &str) -> Self {
        let hex = hex.trim_start_matches('#');
        let vals: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();

        let (r8, g8, b8) = match vals.as_slice() {
            [r, g, b] => (*r, *g, *b),
            [r, g, b, _a] => (*r, *g, *b),
            _ => panic!("Hex code must be 6 or 8 characters long"),
        };

        Self::rgb(r8, g8, b8)
    }
}

/* From impls */

impl From<(u8, u8, u8)> for RGB {
    fn from(v: (u8, u8, u8)) -> Self {
        RGB::rgb(v.0, v.1, v.2)
    }
}

impl From<(u8, u8, u8, u8)> for RGB {
    fn from(v: (u8, u8, u8, u8)) -> Self {
        RGB::rgb(v.0, v.1, v.2)
    }
}

impl From<[u8; 3]> for RGB {
    fn from(v: [u8; 3]) -> Self {
        RGB::rgb(v[0], v[1], v[2])
    }
}

impl From<[u8; 4]> for RGB {
    fn from(v: [u8; 4]) -> Self {
        RGB::rgb(v[0], v[1], v[2])
    }
}

impl From<(f32, f32, f32)> for RGB {
    fn from(v: (f32, f32, f32)) -> Self {
        RGB::rgb_f(v.0, v.1, v.2)
    }
}

impl From<[f32; 3]> for RGB {
    fn from(v: [f32; 3]) -> Self {
        RGB::rgb_f(v[0], v[1], v[2])
    }
}

impl From<&str> for RGB {
    fn from(s: &str) -> Self {
        RGB::hex(s)
    }
}

impl From<u32> for RGB {
    /// Interprets `0xRRGGBB` or `0xAARRGGBB`. If value <= 0x00_FF_FF_FF it's treated as RRGGBB.
    fn from(v: u32) -> Self {
        if v <= 0x00FF_FF_FF {
            let r = ((v >> 16) & 0xFF) as u8;
            let g = ((v >> 8) & 0xFF) as u8;
            let b = (v & 0xFF) as u8;
            RGB::rgb(r, g, b)
        } else {
            let r = ((v >> 16) & 0xFF) as u8;
            let g = ((v >> 8) & 0xFF) as u8;
            let b = (v & 0xFF) as u8;
            RGB::rgb(r, g, b)
        }
    }
}

/* Conversions from RGBA (drop alpha). Assumes RGBA is in scope. */

impl From<RGBA> for RGB {
    fn from(c: RGBA) -> Self {
        RGB::rgb_f(c.r, c.g, c.b)
    }
}

/* Convenience: allow conversion to/from tuples with alpha dropped/ignored */

impl From<RGB> for (f32, f32, f32) {
    fn from(c: RGB) -> Self {
        (c.r, c.g, c.b)
    }
}

impl From<RGB> for [f32; 3] {
    fn from(c: RGB) -> Self {
        [c.r, c.g, c.b]
    }
}
