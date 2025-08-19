/*
Copyright (c) 2018-2021 Emil Ernerfeldt <emil.ernerfeldt@gmail.com>

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
*/

use std::{fmt, ops};

use glam::Vec2;

#[derive(Clone, Copy, PartialEq)]
pub struct Rect {
    min: Vec2,
    max: Vec2,
}

const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// Return true when arguments are the same within some rounding error.
///
/// For instance `almost_equal(x, x.to_degrees().to_radians(), f32::EPSILON)` should hold true for all x.
/// The `epsilon`  can be `f32::EPSILON` to handle simple transforms (like degrees -> radians)
/// but should be higher to handle more complex transformations.
pub fn almost_equal(a: f32, b: f32, epsilon: f32) -> bool {
    if a == b {
        true // handle infinites
    } else {
        let abs_max = a.abs().max(b.abs());
        abs_max <= epsilon || ((a - b).abs() / abs_max) <= epsilon
    }
}

impl Rect {
    /// Infinite rectangle that contains every point.
    pub const EVERYTHING: Self = Self {
        min: vec2(-f32::INFINITY, -f32::INFINITY),
        max: vec2(f32::INFINITY, f32::INFINITY),
    };

    /// The inverse of [`Self::EVERYTHING`]: stretches from positive infinity to negative infinity.
    /// Contains no points.
    ///
    /// This is useful as the seed for bounding boxes.
    ///
    pub const NOTHING: Self = Self {
        min: vec2(f32::INFINITY, f32::INFINITY),
        max: vec2(-f32::INFINITY, -f32::INFINITY),
    };

    /// An invalid [`Rect`] filled with [`f32::NAN`].
    pub const NAN: Self = Self {
        min: vec2(f32::NAN, f32::NAN),
        max: vec2(f32::NAN, f32::NAN),
    };

    /// A [`Rect`] filled with zeroes.
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        max: Vec2::ZERO,
    };

    #[inline(always)]
    pub const fn from_min_max(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    /// left-top corner plus a size (stretching right-down).
    #[inline(always)]
    pub fn from_min_size(min: Vec2, size: Vec2) -> Self {
        Self {
            min,
            max: min + size,
        }
    }

    #[inline(always)]
    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        Self {
            min: center - size * 0.5,
            max: center + size * 0.5,
        }
    }

    /// Returns the bounding rectangle of the two points.
    #[inline]
    pub fn from_two_pos(a: Vec2, b: Vec2) -> Self {
        Self {
            min: vec2(a.x.min(b.x), a.y.min(b.y)),
            max: vec2(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// A zero-sized rect at a specific point.
    #[inline]
    pub fn from_pos(point: Vec2) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// Bounding-box around the points.
    pub fn from_points(points: &[Vec2]) -> Self {
        let mut rect = Self::NOTHING;
        for &p in points {
            rect.extend_with(p);
        }
        rect
    }

    /// A [`Rect`] that contains every point to the right of the given X coordinate.
    #[inline]
    pub fn everything_right_of(left_x: f32) -> Self {
        let mut rect = Self::EVERYTHING;
        rect.set_left(left_x);
        rect
    }

    /// A [`Rect`] that contains every point to the left of the given X coordinate.
    #[inline]
    pub fn everything_left_of(right_x: f32) -> Self {
        let mut rect = Self::EVERYTHING;
        rect.set_right(right_x);
        rect
    }

    /// A [`Rect`] that contains every point below a certain y coordinate
    #[inline]
    pub fn everything_below(top_y: f32) -> Self {
        let mut rect = Self::EVERYTHING;
        rect.set_top(top_y);
        rect
    }

    /// A [`Rect`] that contains every point above a certain y coordinate
    #[inline]
    pub fn everything_above(bottom_y: f32) -> Self {
        let mut rect = Self::EVERYTHING;
        rect.set_bottom(bottom_y);
        rect
    }

    #[must_use]
    #[inline]
    pub fn with_min_x(mut self, min_x: f32) -> Self {
        self.min.x = min_x;
        self
    }

    #[must_use]
    #[inline]
    pub fn with_min_y(mut self, min_y: f32) -> Self {
        self.min.y = min_y;
        self
    }

    #[must_use]
    #[inline]
    pub fn with_max_x(mut self, max_x: f32) -> Self {
        self.max.x = max_x;
        self
    }

    #[must_use]
    #[inline]
    pub fn with_max_y(mut self, max_y: f32) -> Self {
        self.max.y = max_y;
        self
    }

    /// Expand by this much in each direction, keeping the center
    #[must_use]
    pub fn expand(self, amnt: f32) -> Self {
        self.expand2(Vec2::splat(amnt))
    }

    /// Expand by this much in each direction, keeping the center
    #[must_use]
    pub fn expand2(self, amnt: Vec2) -> Self {
        Self::from_min_max(self.min - amnt, self.max + amnt)
    }

    /// Scale up by this factor in each direction, keeping the center
    #[must_use]
    pub fn scale_from_center(self, scale_factor: f32) -> Self {
        self.scale_from_center2(Vec2::splat(scale_factor))
    }

    /// Scale up by this factor in each direction, keeping the center
    #[must_use]
    pub fn scale_from_center2(self, scale_factor: Vec2) -> Self {
        Self::from_center_size(self.center(), self.size() * scale_factor)
    }

    /// Shrink by this much in each direction, keeping the center
    #[must_use]
    pub fn shrink(self, amnt: f32) -> Self {
        self.shrink2(Vec2::splat(amnt))
    }

    /// Shrink by this much in each direction, keeping the center
    #[must_use]
    pub fn shrink2(self, amnt: Vec2) -> Self {
        Self::from_min_max(self.min + amnt, self.max - amnt)
    }

    #[must_use]
    #[inline]
    pub fn translate(self, amnt: Vec2) -> Self {
        Self::from_min_size(self.min + amnt, self.size())
    }

    /// Rotate the bounds (will expand the [`Rect`])
    #[must_use]
    #[inline]
    pub fn rotate_bb(self, rot: Vec2) -> Self {
        let a = rot * self.left_top();
        let b = rot * self.right_top();
        let c = rot * self.left_bottom();
        let d = rot * self.right_bottom();

        Self::from_min_max(a.min(b).min(c).min(d), a.max(b).max(c).max(d))
    }

    #[must_use]
    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
    }

    /// keep min
    pub fn set_width(&mut self, w: f32) {
        self.max.x = self.min.x + w;
    }

    /// keep min
    pub fn set_height(&mut self, h: f32) {
        self.max.y = self.min.y + h;
    }

    /// Keep size
    pub fn set_center(&mut self, center: Vec2) {
        *self = self.translate(center - self.center());
    }

    #[must_use]
    #[inline(always)]
    pub fn contains(&self, p: Vec2) -> bool {
        self.min.x <= p.x && p.x <= self.max.x && self.min.y <= p.y && p.y <= self.max.y
    }

    #[must_use]
    pub fn contains_rect(&self, other: Self) -> bool {
        self.contains(other.min) && self.contains(other.max)
    }

    /// Return the given points clamped to be inside the rectangle
    /// Panics if [`Self::is_negative`].
    #[must_use]
    pub fn clamp(&self, p: Vec2) -> Vec2 {
        p.clamp(self.min, self.max)
    }

    #[inline(always)]
    pub fn extend_with(&mut self, p: Vec2) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    #[inline(always)]
    /// Expand to include the given x coordinate
    pub fn extend_with_x(&mut self, x: f32) {
        self.min.x = self.min.x.min(x);
        self.max.x = self.max.x.max(x);
    }

    #[inline(always)]
    /// Expand to include the given y coordinate
    pub fn extend_with_y(&mut self, y: f32) {
        self.min.y = self.min.y.min(y);
        self.max.y = self.max.y.max(y);
    }

    /// The union of two bounding rectangle, i.e. the minimum [`Rect`]
    /// that contains both input rectangles.
    #[inline(always)]
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// The intersection of two [`Rect`], i.e. the area covered by both.
    #[inline]
    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
    }

    #[inline(always)]
    pub fn center(&self) -> Vec2 {
        Vec2 {
            x: (self.min.x + self.max.x) / 2.0,
            y: (self.min.y + self.max.y) / 2.0,
        }
    }

    /// `rect.size() == Vec2 { x: rect.width(), y: rect.height() }`
    #[inline(always)]
    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }

    /// Note: this can be negative.
    #[inline(always)]
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Note: this can be negative.
    #[inline(always)]
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    /// Width / height
    ///
    /// * `aspect_ratio < 1`: portrait / high
    /// * `aspect_ratio = 1`: square
    /// * `aspect_ratio > 1`: landscape / wide
    pub fn aspect_ratio(&self) -> f32 {
        self.width() / self.height()
    }

    /// `[2, 1]` for wide screen, and `[1, 2]` for portrait, etc.
    /// At least one dimension = 1, the other >= 1
    /// Returns the proportions required to letter-box a square view area.
    pub fn square_proportions(&self) -> Vec2 {
        let w = self.width();
        let h = self.height();
        if w > h {
            vec2(w / h, 1.0)
        } else {
            vec2(1.0, h / w)
        }
    }

    /// This is never negative, and instead returns zero for negative rectangles.
    #[inline(always)]
    pub fn area(&self) -> f32 {
        self.width().clamp(f32::NEG_INFINITY, 0.0) * self.height().clamp(f32::NEG_INFINITY, 0.0)
    }

    /// The distance from the rect to the position.
    ///
    /// The distance is zero when the position is in the interior of the rectangle.
    ///
    /// [Negative rectangles](Self::is_negative) always return [`f32::INFINITY`].
    #[inline]
    pub fn distance_to_pos(&self, pos: Vec2) -> f32 {
        self.distance_sq_to_pos(pos).sqrt()
    }

    /// The distance from the rect to the position, squared.
    ///
    /// The distance is zero when the position is in the interior of the rectangle.
    ///
    /// [Negative rectangles](Self::is_negative) always return [`f32::INFINITY`].
    #[inline]
    pub fn distance_sq_to_pos(&self, pos: Vec2) -> f32 {
        if self.is_negative() {
            return f32::INFINITY;
        }

        let dx = if self.min.x > pos.x {
            self.min.x - pos.x
        } else if pos.x > self.max.x {
            pos.x - self.max.x
        } else {
            0.0
        };

        let dy = if self.min.y > pos.y {
            self.min.y - pos.y
        } else if pos.y > self.max.y {
            pos.y - self.max.y
        } else {
            0.0
        };

        dx * dx + dy * dy
    }

    /// Signed distance to the edge of the box.
    ///
    /// Negative inside the box.
    ///
    /// [Negative rectangles](Self::is_negative) always return [`f32::INFINITY`].
    ///
    /// ```
    /// # use emath::{vec2, Rect};
    /// let rect = Rect::from_min_max(vec2(0.0, 0.0), vec2(1.0, 1.0));
    /// assert_eq!(rect.signed_distance_to_pos(vec2(0.50, 0.50)), -0.50);
    /// assert_eq!(rect.signed_distance_to_pos(vec2(0.75, 0.50)), -0.25);
    /// assert_eq!(rect.signed_distance_to_pos(vec2(1.50, 0.50)), 0.50);
    /// ```
    pub fn signed_distance_to_pos(&self, pos: Vec2) -> f32 {
        if self.is_negative() {
            return f32::INFINITY;
        }

        let edge_distances = (pos - self.center()).abs() - self.size() * 0.5;
        let inside_dist = edge_distances.max_element().min(0.0);
        let outside_dist = edge_distances.max(Vec2::ZERO).length();
        inside_dist + outside_dist
    }

    /// `width < 0 || height < 0`
    #[inline(always)]
    pub fn is_negative(&self) -> bool {
        self.max.x < self.min.x || self.max.y < self.min.y
    }

    /// `width > 0 && height > 0`
    #[inline(always)]
    pub fn is_positive(&self) -> bool {
        self.min.x < self.max.x && self.min.y < self.max.y
    }

    /// True if all members are also finite.
    #[inline(always)]
    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }

    /// True if any member is NaN.
    #[inline(always)]
    pub fn is_nan(self) -> bool {
        self.min.is_nan() || self.max.is_nan()
    }
}

/// ## Convenience functions (assumes origin is towards left top):
impl Rect {
    /// `min.x`
    #[inline(always)]
    pub fn left(&self) -> f32 {
        self.min.x
    }

    /// `min.x`
    #[inline(always)]
    pub fn left_mut(&mut self) -> &mut f32 {
        &mut self.min.x
    }

    /// `min.x`
    #[inline(always)]
    pub fn set_left(&mut self, x: f32) {
        self.min.x = x;
    }

    /// `max.x`
    #[inline(always)]
    pub fn right(&self) -> f32 {
        self.max.x
    }

    /// `max.x`
    #[inline(always)]
    pub fn right_mut(&mut self) -> &mut f32 {
        &mut self.max.x
    }

    /// `max.x`
    #[inline(always)]
    pub fn set_right(&mut self, x: f32) {
        self.max.x = x;
    }

    /// `min.y`
    #[inline(always)]
    pub fn top(&self) -> f32 {
        self.min.y
    }

    /// `min.y`
    #[inline(always)]
    pub fn top_mut(&mut self) -> &mut f32 {
        &mut self.min.y
    }

    /// `min.y`
    #[inline(always)]
    pub fn set_top(&mut self, y: f32) {
        self.min.y = y;
    }

    /// `max.y`
    #[inline(always)]
    pub fn bottom(&self) -> f32 {
        self.max.y
    }

    /// `max.y`
    #[inline(always)]
    pub fn bottom_mut(&mut self) -> &mut f32 {
        &mut self.max.y
    }

    /// `max.y`
    #[inline(always)]
    pub fn set_bottom(&mut self, y: f32) {
        self.max.y = y;
    }

    #[inline(always)]
    #[doc(alias = "top_left")]
    pub fn left_top(&self) -> Vec2 {
        vec2(self.left(), self.top())
    }

    #[inline(always)]
    pub fn center_top(&self) -> Vec2 {
        vec2(self.center().x, self.top())
    }

    #[inline(always)]
    #[doc(alias = "top_right")]
    pub fn right_top(&self) -> Vec2 {
        vec2(self.right(), self.top())
    }

    #[inline(always)]
    pub fn left_center(&self) -> Vec2 {
        vec2(self.left(), self.center().y)
    }

    #[inline(always)]
    pub fn right_center(&self) -> Vec2 {
        vec2(self.right(), self.center().y)
    }

    #[inline(always)]
    #[doc(alias = "bottom_left")]
    pub fn left_bottom(&self) -> Vec2 {
        vec2(self.left(), self.bottom())
    }

    #[inline(always)]
    pub fn center_bottom(&self) -> Vec2 {
        vec2(self.center().x, self.bottom())
    }

    #[inline(always)]
    #[doc(alias = "bottom_right")]
    pub fn right_bottom(&self) -> Vec2 {
        vec2(self.right(), self.bottom())
    }

    /// Split rectangle in left and right halves at the given `x` coordinate.
    pub fn split_left_right_at_x(&self, split_x: f32) -> (Self, Self) {
        let left = Self::from_min_max(self.min, Vec2::new(split_x, self.max.y));
        let right = Self::from_min_max(Vec2::new(split_x, self.min.y), self.max);
        (left, right)
    }

    /// Split rectangle in top and bottom halves at the given `y` coordinate.
    pub fn split_top_bottom_at_y(&self, split_y: f32) -> (Self, Self) {
        let top = Self::from_min_max(self.min, Vec2::new(self.max.x, split_y));
        let bottom = Self::from_min_max(Vec2::new(self.min.x, split_y), self.max);
        (top, bottom)
    }
}

impl Rect {
    /// Does this Rect intersect the given ray (where `d` is normalized)?
    ///
    /// A ray that starts inside the rect will return `true`.
    pub fn intersects_ray(&self, o: Vec2, d: Vec2) -> bool {
        debug_assert!(
            d.is_normalized(),
            "Debug assert: expected normalized direction, but `d` has length {}",
            d.length()
        );

        let mut tmin = -f32::INFINITY;
        let mut tmax = f32::INFINITY;

        if d.x != 0.0 {
            let tx1 = (self.min.x - o.x) / d.x;
            let tx2 = (self.max.x - o.x) / d.x;

            tmin = tmin.max(tx1.min(tx2));
            tmax = tmax.min(tx1.max(tx2));
        }

        if d.y != 0.0 {
            let ty1 = (self.min.y - o.y) / d.y;
            let ty2 = (self.max.y - o.y) / d.y;

            tmin = tmin.max(ty1.min(ty2));
            tmax = tmax.min(ty1.max(ty2));
        }

        0.0 <= tmax && tmin <= tmax
    }

    /// Where does a ray from the center intersect the rectangle?
    ///
    /// `d` is the direction of the ray and assumed to be normalized.
    pub fn intersects_ray_from_center(&self, d: Vec2) -> Vec2 {
        debug_assert!(
            d.is_normalized(),
            "expected normalized direction, but `d` has length {}",
            d.length()
        );

        let mut tmin = f32::NEG_INFINITY;
        let mut tmax = f32::INFINITY;

        for i in 0..2 {
            let inv_d = 1.0 / -d[i];
            let mut t0 = (self.min[i] - self.center()[i]) * inv_d;
            let mut t1 = (self.max[i] - self.center()[i]) * inv_d;

            if inv_d < 0.0 {
                std::mem::swap(&mut t0, &mut t1);
            }

            tmin = tmin.max(t0);
            tmax = tmax.min(t1);
        }

        let t = tmax.min(tmin);
        self.center() + t * -d
    }
}

impl fmt::Debug for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(precision) = f.precision() {
            write!(f, "[{1:.0$?} - {2:.0$?}]", precision, self.min, self.max)
        } else {
            write!(f, "[{:?} - {:?}]", self.min, self.max)
        }
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        self.min.fmt(f)?;
        f.write_str(" - ")?;
        self.max.fmt(f)?;
        f.write_str("]")?;
        Ok(())
    }
}

/// from (min, max) or (left top, right bottom)
impl From<[Vec2; 2]> for Rect {
    #[inline]
    fn from([min, max]: [Vec2; 2]) -> Self {
        Self { min, max }
    }
}

impl ops::Mul<f32> for Rect {
    type Output = Self;

    #[inline]
    fn mul(self, factor: f32) -> Self {
        Self {
            min: self.min * factor,
            max: self.max * factor,
        }
    }
}

impl ops::Mul<Rect> for f32 {
    type Output = Rect;

    #[inline]
    fn mul(self, vec: Rect) -> Rect {
        Rect {
            min: self * vec.min,
            max: self * vec.max,
        }
    }
}

impl ops::Div<f32> for Rect {
    type Output = Self;

    #[inline]
    fn div(self, factor: f32) -> Self {
        Self {
            min: self.min / factor,
            max: self.max / factor,
        }
    }
}

impl ops::BitOr for Rect {
    type Output = Self;

    #[inline]
    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl ops::BitOrAssign for Rect {
    #[inline]
    fn bitor_assign(&mut self, other: Self) {
        *self = self.union(other);
    }
}
