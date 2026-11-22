//! Implementation-only entries for private declarations owned by packaged core source.
use crate::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        context::NativeCall,
    },
    value::view::ValueView,
};
use kagari_common::identity::DefinitionKind;
use kagari_contract::native_import::foundation::FoundationNative;
use kagari_types::{language::binding, ty::Ty};

pub(super) fn binding(entry: FoundationNative) -> NativeBinding {
    match entry {
        FoundationNative::RangeBound(_) => {
            NativeBinding::new([Codec::Value, Codec::Value], Codec::Value, |call| {
                let value = call.argument(0)?;
                let upper = call
                    .argument(1)?
                    .as_bool()
                    .ok_or_else(|| RuntimeError::module_validation("range endpoint selector"))?;
                let range = call.argument_type_argument(0)?;
                let bound = call.result_type_argument()?;
                call.range_bound(&value, &range, &bound, upper)
            })
        }
        FoundationNative::MakeErrResidual | FoundationNative::ForwardErrOrigin => {
            NativeBinding::new(
                [Codec::Value, Codec::Value],
                Codec::Value,
                forward_err_origin,
            )
        }
    }
}

fn forward_err_origin<'call>(call: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    let invalid = || RuntimeError::module_validation("invalid Result origin transfer");
    let result = binding::result_declaration();
    if call.owner().bytecode.identity != result.module {
        return Err(invalid());
    }
    let definitions = call.owner().identity_admission().target();
    let owner = definitions.lookup(&result).ok_or_else(invalid)?;
    let err = definitions
        .lookup_child(owner, DefinitionKind::Variant, "Err", 0)
        .ok_or_else(invalid)?;
    let original = call.argument(0)?;
    let destination = call.argument(1)?;
    for (index, value) in [&original, &destination].into_iter().enumerate() {
        let (layout, count) = call.enum_shape(value)?;
        let (nominal, variant) = layout.value_identity().ok_or_else(invalid)?;
        let expected = call
            .argument_type_view(index)?
            .closed()
            .ok_or_else(invalid)?;
        if variant != err
            || nominal.declaration != owner
            || !layout.variant().reports_failure
            || count != 1
            || *expected != Ty::Enum(nominal)
        {
            return Err(invalid());
        }
    }
    call.forward_enum_origin(&original, &destination)
}
