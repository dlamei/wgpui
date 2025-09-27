use std::{fmt, hash, mem};

pub type HashMap<K, V> = ahash::AHashMap<K, V>;
pub type HashSet<T> = ahash::AHashSet<T>;

#[cfg(target_arch = "wasm32")]
pub type Instant = web_time::Instant;
#[cfg(target_arch = "wasm32")]
pub type Duration = web_time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub type Instant = std::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
pub type Duration = std::time::Duration;

pub const fn rand_f32() -> f32 {
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

pub const fn hex_to_rgba(s: &str) -> RGBA {
    const fn hex_val(b: u8) -> u8 {
        match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => 0,
        }
    }

    const fn byte(h: u8, l: u8) -> u8 {
        (hex_val(h) << 4) | hex_val(l)
    }

    let bytes = s.as_bytes();
    match bytes.len() {
        7 => RGBA::rgb(
            byte(bytes[1], bytes[2]),
            byte(bytes[3], bytes[4]),
            byte(bytes[5], bytes[6]),
        ),
        9 => RGBA::rgba(
            byte(bytes[1], bytes[2]),
            byte(bytes[3], bytes[4]),
            byte(bytes[5], bytes[6]),
            byte(bytes[7], bytes[8]),
        ),
        _ => RGBA::rgba(0, 0, 0, 255),
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

    pub fn as_bytes(self) -> [u8; 4] {
        let r = (self.r * 255.0) as u8;
        let g = (self.g * 255.0) as u8;
        let b = (self.b * 255.0) as u8;
        let a = (self.a * 255.0) as u8;
        [r, g, b, a]
    }

    pub fn as_u32(self) -> u32 {
        u32::from_ne_bytes(self.as_bytes())
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

    pub const fn hex(hex: &str) -> Self {
        const fn hex_val(b: u8) -> u8 {
            match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => 0,
            }
        }

        const fn byte(h: u8, l: u8) -> u8 {
            (hex_val(h) << 4) | hex_val(l)
        }

        let bytes = hex.as_bytes();
        match bytes.len() {
            7 => RGBA::rgb(
                byte(bytes[1], bytes[2]),
                byte(bytes[3], bytes[4]),
                byte(bytes[5], bytes[6]),
            ),
            9 => RGBA::rgba(
                byte(bytes[1], bytes[2]),
                byte(bytes[3], bytes[4]),
                byte(bytes[5], bytes[6]),
                byte(bytes[7], bytes[8]),
            ),
            _ => RGBA::rgba(0, 0, 0, 255),
        }
    }

    pub const RED: RGBA = RGBA::rgb(255, 0, 0);
    pub const GREEN: RGBA = RGBA::rgb(0, 255, 0);
    pub const BLUE: RGBA = RGBA::rgb(0, 0, 255);

    pub const PURPLE: RGBA = RGBA::hex("#740580");
    pub const MAGENTA: RGBA = RGBA::hex("#B10065");
    pub const FOLLY: RGBA = RGBA::hex("#FF1D68");
    pub const ORANGE: RGBA = RGBA::hex("#F76218");
    pub const SAFFRON: RGBA = RGBA::hex("#F2C447");
    pub const INDIGO: RGBA = RGBA::hex("#214675");
    pub const DARK_BLUE: RGBA = RGBA::hex("#122741");
    pub const CYAN: RGBA = RGBA::hex("#00f7f7");
    pub const TEAL: RGBA = RGBA::hex("#007c7c");

    pub const WHITE: RGBA = RGBA::rgb(255, 255, 255);
    pub const BLACK: RGBA = RGBA::rgb(0, 0, 0);

    pub const PASTEL_PINK: RGBA = RGBA::hex("#FFB5E8");
    pub const PASTEL_BLUE: RGBA = RGBA::hex("#B5DEFF");
    pub const PASTEL_GREEN: RGBA = RGBA::hex("#C1FFD7");
    pub const PASTEL_YELLOW: RGBA = RGBA::hex("#FFFACD");
    pub const PASTEL_PURPLE: RGBA = RGBA::hex("#D7B5FF");
    pub const PASTEL_ORANGE: RGBA = RGBA::hex("#FFD1B5");
    pub const PASTEL_MINT: RGBA = RGBA::hex("#B5FFF9");

    pub const CARMINE: RGBA = RGBA::rgb(200, 0, 100);

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

#[cfg(not(target_arch = "wasm32"))]
pub mod futures {
    use std::sync::{Arc, Mutex};

    enum State {
        Idle,
        Blocking,
        Ready,
    }

    struct Signal {
        state: Mutex<State>,
        cond: std::sync::Condvar,
    }

    impl Signal {
        fn new() -> Self {
            Self {
                state: Mutex::new(State::Idle),
                cond: std::sync::Condvar::new(),
            }
        }

        fn wait(&self) {
            let mut state = self.state.lock().unwrap();
            match *state {
                State::Blocking => unreachable!(),
                State::Ready => *state = State::Idle,
                State::Idle => {
                    *state = State::Blocking;
                    while let State::Blocking = *state {
                        state = self.cond.wait(state).unwrap();
                    }
                }
            }
        }

        fn wake_(&self) {
            let mut state = self.state.lock().unwrap();

            match *state {
                State::Ready => (),
                State::Idle => *state = State::Ready,
                State::Blocking => {
                    *state = State::Idle;
                    self.cond.notify_one();
                }
            }
        }
    }

    impl std::task::Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.wake_()
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.wake_()
        }
    }

    pub fn wait_for<F: IntoFuture>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future.into_future());

        let signal = Arc::new(Signal::new());

        let waker = std::task::Waker::from(Arc::clone(&signal));
        let mut context = std::task::Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut context) {
                std::task::Poll::Pending => signal.wait(),
                std::task::Poll::Ready(res) => return res,
            }
        }
    }
}

pub trait ExplicitCopy: Copy {
    #[inline(always)]
    fn copy(&self) -> Self {
        *self
    }
}

impl<T: Copy> ExplicitCopy for T {}

pub struct ArrVec<T, const N: usize> {
    data: [mem::MaybeUninit<T>; N],
    count: usize,
}

pub struct ArrVecIter<'a, T, const N: usize> {
    vec: &'a ArrVec<T, N>,
    index: usize,
}

impl<'a, T, const N: usize> Iterator for ArrVecIter<'a, T, N> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.vec.count {
            let item = unsafe { self.vec.data[self.index].assume_init_ref() };
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.vec.count - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, T, const N: usize> ExactSizeIterator for ArrVecIter<'a, T, N> {}

pub struct ArrVecIterMut<'a, T, const N: usize> {
    vec: &'a mut ArrVec<T, N>,
    index: usize,
}

impl<'a, T, const N: usize> Iterator for ArrVecIterMut<'a, T, N> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.vec.count {
            let item = unsafe {
                // We need to extend the lifetime here, which is safe because
                // the iterator holds a mutable reference to the vec
                std::mem::transmute(self.vec.data[self.index].assume_init_mut())
            };
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.vec.count - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, T, const N: usize> ExactSizeIterator for ArrVecIterMut<'a, T, N> {}

impl<T, const N: usize> fmt::Debug for ArrVec<T, N>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        for i in 0..self.count {
            unsafe {
                list.entry(self.data[i].assume_init_ref());
            }
        }
        list.finish()
    }
}

impl<T, const N: usize> ArrVec<T, N> {
    pub fn new() -> Self {
        Self {
            data: unsafe { mem::MaybeUninit::uninit().assume_init() },
            count: 0, // Start with 0 elements, not 1
        }
    }

    pub fn len(&self) -> usize {
        self.count // Return count of initialized elements, not array length
    }

    pub fn cap(&self) -> usize {
        N
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn push(&mut self, elem: T) {
        assert!(self.count < N, "ArrVec is full");
        self.data[self.count].write(elem);
        self.count += 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.count == 0 {
            None
        } else {
            self.count -= 1;
            Some(unsafe { self.data[self.count].assume_init_read() })
        }
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        if index < self.count {
            Some(unsafe { self.data[index].assume_init_ref() })
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index < self.count {
            Some(unsafe { self.data[index].assume_init_mut() })
        } else {
            None
        }
    }

    pub fn as_slice(&self) -> &[T] {
        unsafe { mem::transmute(&self.data[..self.count]) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { mem::transmute(&mut self.data[..self.count]) }
    }

    pub fn iter(&self) -> ArrVecIter<'_, T, N> {
        ArrVecIter {
            vec: self,
            index: 0,
        }
    }

    pub fn iter_mut(&mut self) -> ArrVecIterMut<'_, T, N> {
        ArrVecIterMut {
            vec: self,
            index: 0,
        }
    }
}

impl<T, const N: usize> ArrVec<T, N>
where
    T: Copy,
{
    pub fn as_padded_arr(&self, pad: T) -> [T; N] {
        let mut res = [pad; N];
        res[0..self.count].copy_from_slice(self.as_slice());
        res
    }
}

impl<T, const N: usize> Default for ArrVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> PartialEq for ArrVec<T, N>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        if self.count != other.count {
            return false;
        }

        for i in 0..self.count {
            unsafe {
                if self.data[i].assume_init_ref() != other.data[i].assume_init_ref() {
                    return false;
                }
            }
        }
        true
    }
}

impl<T, const N: usize> Eq for ArrVec<T, N> where T: Eq {}

impl<T, const N: usize> hash::Hash for ArrVec<T, N>
where
    T: hash::Hash,
{
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        use hash::Hash;
        self.count.hash(state);
        for i in 0..self.count {
            unsafe {
                self.data[i].assume_init_ref().hash(state);
            }
        }
    }
}

impl<T, const N: usize> Copy for ArrVec<T, N> where T: Copy {}

impl<T, const N: usize> Clone for ArrVec<T, N>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        let mut new_vec = Self::new();
        for i in 0..self.count {
            unsafe {
                new_vec.push(self.data[i].assume_init_ref().clone());
            }
        }
        new_vec
    }
}

impl<T, const N: usize> std::ops::Index<usize> for ArrVec<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("index out of bounds")
    }
}

impl<T, const N: usize> std::ops::IndexMut<usize> for ArrVec<T, N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index).expect("index out of bounds")
    }
}

// Example usage and tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut vec: ArrVec<i32, 5> = ArrVec::new();

        assert_eq!(vec.len(), 0);
        assert!(vec.is_empty());

        vec.push(1);
        vec.push(2);
        vec.push(3);

        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], 1);
        assert_eq!(vec[1], 2);
        assert_eq!(vec[2], 3);

        assert_eq!(vec.pop(), Some(3));
        assert_eq!(vec.pop(), Some(2));
        assert_eq!(vec.len(), 1);

        assert_eq!(vec.pop(), Some(1));
        assert_eq!(vec.pop(), None);
        assert!(vec.is_empty());
    }

    #[test]
    fn test_traits() {
        let mut vec1: ArrVec<i32, 5> = ArrVec::new();
        vec1.push(1);
        vec1.push(2);
        vec1.push(3);

        let vec2 = vec1.clone();
        assert_eq!(vec1, vec2);

        let mut vec3: ArrVec<i32, 5> = ArrVec::default();
        vec3.push(1);
        vec3.push(2);
        assert_ne!(vec1, vec3);

        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(vec1.clone(), "test");
        assert_eq!(map.get(&vec1), Some(&"test"));
    }

    #[test]
    fn test_iterators() {
        let mut vec: ArrVec<i32, 5> = ArrVec::new();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        let collected: Vec<&i32> = vec.iter().collect();
        assert_eq!(collected, vec![&1, &2, &3]);

        for item in vec.iter_mut() {
            *item *= 2;
        }

        let collected: Vec<&i32> = vec.iter().collect();
        assert_eq!(collected, vec![&2, &4, &6]);
    }
}
