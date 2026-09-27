//! Numeric casts shared by compile-time and runtime evaluation.
#[derive(Debug, Clone, Copy)]
pub enum Number {
    Integer(i128),
    F32(f32),
    F64(f64),
}
#[derive(Debug, Clone, Copy)]
pub enum NumberType {
    Integer { bits: u32, signed: bool },
    F32,
    F64,
}

pub fn cast(value: Number, target: NumberType) -> Number {
    use Number::*;
    match target {
        NumberType::Integer { bits, signed } => {
            let value = match value {
                Integer(value) => crate::integer::wrap(value, bits, signed),
                F32(value) => {
                    let (min, max) = crate::integer::bounds(bits, signed);
                    (value as i128).clamp(min, max)
                }
                F64(value) => {
                    let (min, max) = crate::integer::bounds(bits, signed);
                    (value as i128).clamp(min, max)
                }
            };
            Integer(value)
        }
        NumberType::F32 => F32(match value {
            Integer(v) => v as f32,
            F32(v) => v,
            F64(v) => v as f32,
        }),
        NumberType::F64 => F64(match value {
            Integer(v) => v as f64,
            F32(v) => f64::from(v),
            F64(v) => v,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn float_cast_boundaries_match_rust() {
        macro_rules! check {
            ($ty:ty, $signed:expr) => {{
                for value in [
                    f64::NAN,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    -1e30,
                    1e30,
                    -257.9,
                    -128.9,
                    -1.9,
                    -0.0,
                    0.9,
                    127.9,
                    255.9,
                    256.9,
                    9223372036854775808.0,
                    18446744073709551616.0,
                ] {
                    let target = NumberType::Integer {
                        bits: <$ty>::BITS,
                        signed: $signed,
                    };
                    let Number::Integer(actual) = cast(Number::F64(value), target) else {
                        panic!()
                    };
                    assert_eq!(actual, (value as $ty) as i128);
                    let Number::Integer(actual) = cast(Number::F32(value as f32), target) else {
                        panic!()
                    };
                    assert_eq!(actual, ((value as f32) as $ty) as i128);
                }
            }};
        }
        check!(i8, true);
        check!(i16, true);
        check!(i32, true);
        check!(i64, true);
        check!(u8, false);
        check!(u16, false);
        check!(u32, false);
        check!(u64, false);
    }
}
