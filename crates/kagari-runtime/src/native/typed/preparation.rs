//! Binding/application preparation owns stable conversion and scoped slot facts.
use crate::{
    error::RuntimeError,
    frame::types::arguments::ScopedSignature,
    module::execution::{
        OperandSlot,
        layout::{FrameLayout, Location},
    },
    native::{
        binding::NativeResult,
        conversion::{IntoKagari, arguments::FromKagariArguments, context::ConversionContext},
    },
};
use kagari_abi::representation::ValueType;
use kagari_types::{reference::ReferenceAccess, ty::Ty};
use std::{iter, sync::Arc};

pub(super) fn prepare<A: FromKagariArguments, R: IntoKagari>(
    context: &ConversionContext<'_>,
    signature: &ScopedSignature,
    receiver_access: Option<ReferenceAccess>,
) -> NativeResult<Arc<FrameLayout>> {
    context.check_type::<R>(&signature.result)?;
    A::check_types(context, &signature.params)?;
    let width = signature
        .params
        .len()
        .checked_add(1)
        .ok_or_else(|| RuntimeError::resource_limit("native input layout width"))?;
    let mut locations = Vec::new();
    locations
        .try_reserve_exact(width)
        .map_err(|_| context.runtime().resources().limit("native input layout"))?;
    for (index, argument) in signature
        .params
        .iter()
        .chain(iter::once(&signature.result))
        .enumerate()
    {
        // These are execution-only roots, not ordinary language storage. The
        // declared receiver/reference role admits scoped aliases; value/capability
        // checks remain at entry and at each decoding boundary.
        locations.push(Location {
            operand: OperandSlot::new(index, true),
            representation: ValueType::Generic,
            semantic: None,
            borrowed_receiver: (index == 0 && receiver_access.is_some())
                || matches!(argument.ty(), Ty::Reference { .. }),
        });
    }
    Ok(Arc::new(FrameLayout {
        locations: locations.into_boxed_slice(),
        count: 0,
        scalar_count: 0,
        managed_count: width,
        reused_slots: Box::default(),
    }))
}
