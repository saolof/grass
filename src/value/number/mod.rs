use std::{
    cmp::Ordering,
    convert::From,
    fmt, mem,
    ops::{
        Add, AddAssign, Deref, DerefMut, Div, DivAssign, Mul, MulAssign, Neg, Rem, RemAssign, Sub,
        SubAssign,
    },
};

use crate::{
    error::SassResult,
    unit::{Unit, UNIT_CONVERSION_TABLE},
};

use codemap::Span;
use integer::Integer;

mod integer;

const PRECISION: i32 = 10;

fn epsilon() -> f64 {
    10.0_f64.powi(-PRECISION - 1)
}

fn inverse_epsilon() -> f64 {
    10.0_f64.powi(PRECISION + 1)
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(crate) struct Number(pub f64);

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Number {}

fn fuzzy_equals(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }

    (a - b).abs() <= epsilon() && (a * inverse_epsilon()).round() == (b * inverse_epsilon()).round()
}

fn fuzzy_as_int(num: f64) -> Option<i32> {
    if !num.is_finite() {
        return None;
    }

    let rounded = num.round();

    if fuzzy_equals(num, rounded) {
        Some(rounded as i32)
    } else {
        None
    }
}

impl Number {
    pub fn is_positive(self) -> bool {
        self.0.is_sign_positive() && !self.is_zero()
    }

    pub fn is_negative(self) -> bool {
        self.0.is_sign_negative() && !self.is_zero()
    }

    pub fn assert_int(self, span: Span) -> SassResult<i32> {
        match fuzzy_as_int(self.0) {
            Some(i) => Ok(i),
            None => Err((format!("{} is not an int.", self.0), span).into()),
        }
    }

    pub fn assert_int_with_name(self, name: &'static str, span: Span) -> SassResult<i32> {
        match fuzzy_as_int(self.0) {
            Some(i) => Ok(i),
            None => Err((format!("${name} is not an int."), span).into()),
        }
    }

    pub fn to_integer(self) -> Integer {
        Integer::Small(self.0 as i64)
    }

    pub fn small_ratio<A: Into<i64>, B: Into<i64>>(a: A, b: B) -> Self {
        Self(a.into() as f64 / b.into() as f64)
        // Number::new_small(Rational64::new(a.into(), b.into()))
    }

    pub fn round(self) -> Self {
        Self(self.0.round())
    }

    pub fn ceil(self) -> Self {
        Self(self.0.ceil())
    }

    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    pub fn is_decimal(self) -> bool {
        self.0.fract() != 0.0
    }

    pub fn clamp(self, min: f64, max: f64) -> Self {
        if self.0 > max {
            return Number(max);
        }

        if min == 0.0 && self.is_negative() {
            return Number::zero();
        }

        if self.0 < min {
            return Number(min);
        }

        self
    }

    pub fn sqrt(self) -> Self {
        Self(self.0.sqrt())
    }

    pub fn ln(self) -> Self {
        Self(self.0.ln())
    }

    pub fn log(self, base: Number) -> Self {
        Self(self.0.log(base.0))
    }

    pub fn pow(self, exponent: Self) -> Self {
        Self(self.0.powf(exponent.0))
    }

    /// Invariants: `from.comparable(&to)` must be true
    pub fn convert(self, from: &Unit, to: &Unit) -> Self {
        if from == &Unit::None || to == &Unit::None || from == to {
            return self;
        }

        debug_assert!(from.comparable(to), "from: {:?}, to: {:?}", from, to);

        Number(self.0 * UNIT_CONVERSION_TABLE[to][from])
    }
}

macro_rules! inverse_trig_fn(
    ($name:ident) => {
        pub fn $name(self) -> Self {
            Self(self.0.$name().to_degrees())
        }
    }
);

/// Trigonometry methods
impl Number {
    inverse_trig_fn!(acos);
    inverse_trig_fn!(asin);
    inverse_trig_fn!(atan);
}

impl Default for Number {
    fn default() -> Self {
        Self::zero()
    }
}

impl Number {
    pub const fn one() -> Self {
        Self(1.0)
    }

    pub fn is_one(self) -> bool {
        self.0 == 1.0
    }

    pub const fn zero() -> Self {
        Self(0.0)
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0.0
    }
}

impl Deref for Number {
    type Target = f64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Number {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

macro_rules! from_integer {
    ($ty:ty) => {
        impl From<$ty> for Number {
            fn from(b: $ty) -> Self {
                Number(b as f64)
            }
        }
    };
}

macro_rules! from_smaller_integer {
    ($ty:ty) => {
        impl From<$ty> for Number {
            fn from(val: $ty) -> Self {
                Self(f64::from(val))
            }
        }
    };
}

impl From<i64> for Number {
    fn from(val: i64) -> Self {
        Self(val as f64)
    }
}

impl From<f64> for Number {
    fn from(b: f64) -> Self {
        Self(b)
    }
}

from_integer!(usize);
from_integer!(isize);
from_smaller_integer!(i32);
from_smaller_integer!(u32);
from_smaller_integer!(u8);

impl fmt::Debug for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Number( {} )", self.to_string(false))
    }
}

impl Number {
    pub(crate) fn inspect(self) -> String {
        self.to_string(false)
    }

    pub(crate) fn to_string(self, is_compressed: bool) -> String {
        if self.0.is_infinite() && self.0.is_sign_negative() {
            return "-Infinity".to_owned();
        } else if self.0.is_infinite() {
            return "Infinity".to_owned();
        }

        let mut buffer = String::with_capacity(3);

        if self.0 < 0.0 {
            buffer.push('-');
        }

        let num = self.0.abs();

        if is_compressed && num < 1.0 {
            buffer.push_str(
                format!("{:.10}", num)[1..]
                    .trim_end_matches('0')
                    .trim_end_matches('.'),
            );
        } else {
            buffer.push_str(
                format!("{:.10}", num)
                    .trim_end_matches('0')
                    .trim_end_matches('.'),
            );
        }

        if buffer.is_empty() || buffer == "-" || buffer == "-0" {
            return "0".to_owned();
        }

        buffer
    }
}

impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl Ord for Number {
    fn cmp(&self, other: &Self) -> Ordering {
        if !self.is_finite() || !other.is_finite() {
            todo!()
        }

        self.0.partial_cmp(&other.0).unwrap()
    }
}

impl Add for Number {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl AddAssign for Number {
    fn add_assign(&mut self, other: Self) {
        let tmp = mem::take(self);
        *self = tmp + other;
    }
}

impl Sub for Number {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl SubAssign for Number {
    fn sub_assign(&mut self, other: Self) {
        let tmp = mem::take(self);
        *self = tmp - other;
    }
}

impl Mul for Number {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self(self.0 * other.0)
    }
}

impl Mul<i64> for Number {
    type Output = Self;

    fn mul(self, other: i64) -> Self {
        Self(self.0 * other as f64)
    }
}

impl MulAssign<i64> for Number {
    fn mul_assign(&mut self, other: i64) {
        let tmp = mem::take(self);
        *self = tmp * other;
    }
}

impl MulAssign for Number {
    fn mul_assign(&mut self, other: Self) {
        let tmp = mem::take(self);
        *self = tmp * other;
    }
}

impl Div for Number {
    type Output = Self;

    fn div(self, other: Self) -> Self {
        Self(self.0 / other.0)
    }
}

impl DivAssign for Number {
    fn div_assign(&mut self, other: Self) {
        let tmp = mem::take(self);
        *self = tmp / other;
    }
}

fn real_mod(n1: f64, n2: f64) -> f64 {
    n1.rem_euclid(n2)
}

fn modulo(n1: f64, n2: f64) -> f64 {
    if n2 > 0.0 {
        return real_mod(n1, n2);
    }

    if n2 == 0.0 {
        return f64::NAN;
    }

    let result = real_mod(n1, n2);

    if result == 0.0 {
        0.0
    } else {
        result + n2
    }
}

impl Rem for Number {
    type Output = Self;

    fn rem(self, other: Self) -> Self {
        Self(modulo(self.0, other.0))
    }
}

impl RemAssign for Number {
    fn rem_assign(&mut self, other: Self) {
        let tmp = mem::take(self);
        *self = tmp % other;
    }
}

impl Neg for Number {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}
