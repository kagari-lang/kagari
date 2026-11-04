//! Scalar transport contracts across entry preparation, slots and host snapshots.

use crate::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    catalog::DeclarationCatalog,
    context::CallContext,
    declarations::FunctionDecl,
    managed::{
        call::{CallRegion, Memory},
        entry::ScopedEntry,
        slots::Kind,
    },
    scalar::NativeScalar,
    types::Type,
};
use kagari_gc::error::GcError;
use kagari_types::scalar::BuiltinType;

fn echo<T: Copy>(_: &mut CallRegion<'_, '_>, (value,): (T,)) -> NativeResult<T> {
    Ok(value)
}

fn round_trip<T>(value: T) -> NativeResult<T>
where
    T: Copy + NativeScalar + for<'gc> Kind<View<'gc> = T>,
{
    let mut builder = ModuleBuilder::new("example::scalars", &DeclarationCatalog::default())?;
    let ty = Type::scalar(T::scalar_type().unwrap());
    let declaration = builder.define_function(
        FunctionDecl::new("echo")
            .parameter("value", ty.clone())
            .returns(ty),
    )?;
    builder.bind(
        declaration,
        |_: &mut CallContext<'_>, value: T| -> NativeResult<T> { Ok(value) },
    )?;
    let module = builder.finish()?;
    let entry =
        ScopedEntry::<(T,), T, _>::prepare_scalar(&module.declaration().functions[0], echo::<T>)?;
    let memory = Memory::new(Default::default())?;
    let snapshot = memory.call(|call| {
        let input = call.local::<T>()?;
        let output = call.local::<T>()?;
        call.region(|r| r.write(input, value))?;
        entry.invoke(call, (input,), output)?;
        call.collect()?;
        call.region(|r| r.root(output))
    })?;
    let snapshot = snapshot.clone();
    memory.collect()?;
    let foreign = Memory::new(Default::default())?;
    foreign.call(|call| {
        call.region(|r| {
            assert!(r.admit_value(&snapshot).is_err());
            Ok(())
        })
    })?;
    let result = memory.call(|call| call.region(|r| r.admit_value(&snapshot)))?;
    assert_eq!(memory.stats()?.objects, 0);
    assert_eq!(memory.stats()?.root_registrations, 0);
    memory.call(|call| {
        assert_eq!(call.working_slots()?, 0);
        Ok(())
    })?;
    Ok(result)
}

#[test]
fn immediate_scalars_preserve_domains_and_float_bits_through_entries_slots_and_roots()
-> NativeResult<()> {
    round_trip(())?;
    for value in [false, true] {
        assert_eq!(round_trip(value)?, value);
    }
    for value in ['\0', '\u{d7ff}', '\u{e000}', '🙂', char::MAX] {
        assert_eq!(round_trip(value)?, value);
    }
    macro_rules! integers {
        ($($ty:ty),+) => { $(
            for value in [<$ty>::MIN, 0, <$ty>::MAX] {
                assert_eq!(round_trip(value)?, value);
            }
        )+ };
    }
    integers!(i8, i16, i32, i64, u8, u16, u32, u64);
    for bits in [0, 0x8000_0000, 0x7f80_0000, 0xff80_0000, 0x7fc1_2345, 1] {
        assert_eq!(round_trip(f32::from_bits(bits))?.to_bits(), bits);
    }
    for bits in [
        0,
        0x8000_0000_0000_0000,
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        0x7ff8_1234_5678_9abc,
        1,
    ] {
        assert_eq!(round_trip(f64::from_bits(bits))?.to_bits(), bits);
    }
    Ok(())
}

#[test]
fn erased_scalar_ingress_preserves_checked_narrowing_and_exact_carriers() -> NativeResult<()> {
    let memory = Memory::new(Default::default())?;
    memory.call(|call| {
        call.region(|r| {
            macro_rules! rejects {
                ($target:ty, $source:ty, $values:expr) => {
                    for value in $values {
                        assert!(matches!(
                            <$target>::admit(r.scope(), <$source>::lower(value)),
                            Err(GcError::Representation)
                        ));
                    }
                };
            }
            rejects!(i8, i32, [i32::MIN, -129, 128, i32::MAX]);
            rejects!(i16, i32, [-32769, 32768]);
            rejects!(u8, i64, [-1, 256, i64::MAX]);
            rejects!(u16, i64, [-1, 65536]);
            rejects!(u32, i64, [-1, 0x1_0000_0000]);
            rejects!(u64, i64, [0, i64::MAX]);
            rejects!(i64, u64, [0, u64::MAX]);
            rejects!(char, u64, [0, 0xd800, 0x110000]);
            rejects!(bool, u64, [0, 1, 2]);
            rejects!(f32, f64, [0.0, 1.0]);
            rejects!(f64, f32, [0.0, 1.0]);
            Ok(())
        })
    })
}

fn positive(_: &mut CallRegion<'_, '_>, (a, b): (i16, f32)) -> NativeResult<bool> {
    Ok(f32::from(a) + b > 0.0)
}

#[test]
fn mixed_scalar_native_entries_prepare_exact_declared_widths_before_body_effects()
-> NativeResult<()> {
    let mut builder = ModuleBuilder::new("example::scalars", &DeclarationCatalog::default())?;
    for (name, argument) in [
        ("correct", Type::scalar(BuiltinType::I16)),
        ("wrong_width", Type::i32()),
    ] {
        let declaration = builder.define_function(
            FunctionDecl::new(name)
                .parameter("a", argument)
                .parameter("b", Type::scalar(BuiltinType::F32))
                .returns(Type::bool()),
        )?;
        if name == "correct" {
            builder.bind(
                declaration,
                |_: &mut CallContext<'_>, a: i16, b: f32| -> NativeResult<bool> {
                    Ok(f32::from(a) + b > 0.0)
                },
            )?;
        } else {
            builder.bind(
                declaration,
                |_: &mut CallContext<'_>, a: i32, b: f32| -> NativeResult<bool> {
                    Ok(a as f32 + b > 0.0)
                },
            )?;
        }
    }
    let module = builder.finish()?;
    let correct = ScopedEntry::<(i16, f32), bool, _>::prepare_scalar(
        &module.declaration().functions[0],
        positive,
    )?;
    assert!(
        ScopedEntry::<(i16, f32), bool, _>::prepare_scalar(
            &module.declaration().functions[1],
            positive,
        )
        .is_err()
    );
    let memory = Memory::new(Default::default())?;
    memory.call(|call| {
        let a = call.local::<i16>()?;
        let b = call.local::<f32>()?;
        let result = call.local::<bool>()?;
        call.region(|r| {
            r.write(a, -2)?;
            r.write(b, 2.5)
        })?;
        correct.invoke(call, (a, b), result)?;
        call.region(|r| {
            assert!(r.read(result)?);
            Ok(())
        })
    })
}
