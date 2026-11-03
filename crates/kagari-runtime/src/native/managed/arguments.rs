//! Prepared argument shapes project directly from typed call-owned slots.

use crate::native::{
    binding::NativeResult,
    managed::{
        call::CallRegion,
        slots::{Kind, Local},
    },
};
use kagari_types::scalar::BuiltinType;

/// Supported outer argument tuples. User conversions cannot assert this proof.
pub trait Arguments: private::Sealed {
    type Views<'gc>;
    type Locals<'call>: Copy;

    fn read<'gc>(
        region: &mut CallRegion<'_, 'gc>,
        locals: Self::Locals<'_>,
    ) -> NativeResult<Self::Views<'gc>>;

    fn scalar_types() -> Option<Vec<BuiltinType>>;
}

mod private {
    pub trait Sealed {}
}

impl private::Sealed for () {}

impl Arguments for () {
    type Views<'gc> = ();
    type Locals<'call> = ();

    fn read<'gc>(_: &mut CallRegion<'_, 'gc>, (): ()) -> NativeResult<()> {
        Ok(())
    }

    fn scalar_types() -> Option<Vec<BuiltinType>> {
        Some(Vec::new())
    }
}

impl<A: Kind> private::Sealed for (A,) {}

impl<A: Kind> Arguments for (A,) {
    type Views<'gc> = (A::View<'gc>,);
    type Locals<'call> = (Local<'call, A>,);

    fn read<'gc>(
        region: &mut CallRegion<'_, 'gc>,
        (a,): Self::Locals<'_>,
    ) -> NativeResult<Self::Views<'gc>> {
        Ok((region.read(a)?,))
    }

    fn scalar_types() -> Option<Vec<BuiltinType>> {
        Some(vec![A::scalar_type()?])
    }
}

impl<A: Kind, B: Kind> private::Sealed for (A, B) {}

impl<A: Kind, B: Kind> Arguments for (A, B) {
    type Views<'gc> = (A::View<'gc>, B::View<'gc>);
    type Locals<'call> = (Local<'call, A>, Local<'call, B>);

    fn read<'gc>(
        region: &mut CallRegion<'_, 'gc>,
        (a, b): Self::Locals<'_>,
    ) -> NativeResult<Self::Views<'gc>> {
        Ok((region.read(a)?, region.read(b)?))
    }

    fn scalar_types() -> Option<Vec<BuiltinType>> {
        Some(vec![A::scalar_type()?, B::scalar_type()?])
    }
}
