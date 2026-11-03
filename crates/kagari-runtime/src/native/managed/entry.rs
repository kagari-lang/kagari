//! Typed scoped returns are published before their generative region ends.

use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        managed::{
            arguments::Arguments,
            call::{CallRegion, NativeCall},
            slots::{Kind, Local},
        },
    },
};
use kagari_types::{declaration::FnDecl, ty::Ty};
use std::marker::PhantomData;

/// A Rust-typed scoped body. It does not itself certify a script declaration.
/// Production nominal/application admission remains with the existing linker.
///
/// ```compile_fail
/// use kagari_runtime::native::{binding::NativeResult, managed::{call::CallRegion, entry::ScopedEntry}};
/// fn wrong(_: &mut CallRegion<'_, '_>, _: (u64,)) -> NativeResult<bool> { Ok(true) }
/// let entry = ScopedEntry::<(u64,), u64, _>::new(wrong);
/// ```
///
/// ```compile_fail
/// use kagari_runtime::native::{binding::NativeResult, managed::{call::CallRegion, entry::ScopedEntry}};
/// fn missing(_: &mut CallRegion<'_, '_>, _: (u64,)) -> NativeResult<u64> {}
/// let entry = ScopedEntry::<(u64,), u64, _>::new(missing);
/// ```
pub struct ScopedEntry<A: Arguments, R: Kind, F> {
    body: F,
    marker: PhantomData<fn(A) -> R>,
}

impl<A: Arguments, R: Kind, F> ScopedEntry<A, R, F>
where
    F: for<'a, 'gc> Fn(&mut CallRegion<'a, 'gc>, A::Views<'gc>) -> NativeResult<R::View<'gc>>,
{
    /// Check a closed scalar declaration once, before any body/converter effects.
    /// Nominal/generic application admission must not use this restricted adapter.
    pub fn prepare_scalar(declaration: &FnDecl, body: F) -> NativeResult<Self> {
        let arguments = A::scalar_types().ok_or_else(|| {
            RuntimeError::module_validation("managed scalar adapter requires scalar arguments")
        })?;
        let result = R::scalar_type().ok_or_else(|| {
            RuntimeError::module_validation("managed scalar adapter requires a scalar result")
        })?;
        if declaration.params.len() != arguments.len()
            || declaration
                .params
                .iter()
                .zip(arguments)
                .any(|(parameter, kind)| parameter.ty != Ty::Builtin(kind))
            || declaration.return_type != Ty::Builtin(result)
        {
            return Err(RuntimeError::module_validation(
                "managed Rust entry differs from declared signature",
            ));
        }
        Ok(Self::new(body))
    }

    /// Build a Rust-only scoped body; a typed Local is not nominal link evidence.
    pub fn new(body: F) -> Self {
        Self {
            body,
            marker: PhantomData,
        }
    }

    pub fn invoke<'call>(
        &self,
        call: &mut NativeCall<'call>,
        argument: A::Locals<'call>,
        result: Local<'call, R>,
    ) -> NativeResult<()> {
        call.region(|region| {
            let argument = A::read(region, argument)?;
            let result_value = (self.body)(region, argument)?;
            region.write(result, result_value)
        })
    }
}
