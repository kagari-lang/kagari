//! Aggregate type work is bounded in addition to each type's existing wire shape limit.
use crate::{decode_limits::MAX_TABLE_RECORDS, source::MAX_SOURCE_BYTES};
use kagari_common::{
    cancellation::CancellationToken,
    identity::mapping::{DefinitionMappingError, check_cancel},
};
use kagari_types::ty::{NominalTy, Ty};

pub(super) fn charge_types(
    arguments: &[Ty],
    bytes: &mut usize,
    nodes: &mut usize,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    for argument in arguments {
        check_cancel(cancel)?;
        if !argument.within_wire_limits() {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        let mut pending = vec![argument];
        while let Some(ty) = pending.pop() {
            check_cancel(cancel)?;
            *nodes = nodes
                .checked_add(1)
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            // Conservative allowance for the node tag, reference and container metadata.
            *bytes = bytes
                .checked_add(64)
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            if *nodes > MAX_TABLE_RECORDS || *bytes > MAX_SOURCE_BYTES {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            match ty {
                Ty::Projection {
                    arguments,
                    receiver,
                    interface,
                    ..
                } => {
                    pending.extend(arguments);
                    pending.push(receiver);
                    nominal(interface, &mut pending);
                }
                Ty::Struct(value)
                | Ty::NativeObject(value)
                | Ty::Enum(value)
                | Ty::Trait(value) => nominal(value, &mut pending),
                Ty::Tuple(values) => pending.extend(values),
                Ty::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Ty::Iter(value)
                | Ty::Range(value, _)
                | Ty::Array(value)
                | Ty::Reference { target: value, .. } => pending.push(value),

                Ty::Host(_) | Ty::SelfType(_) | Ty::Parameter { .. } | Ty::Builtin(_) => {}
            }
        }
    }
    Ok(())
}

fn nominal<'a>(value: &'a NominalTy, pending: &mut Vec<&'a Ty>) {
    pending.extend(&value.arguments);
    pending.extend(value.associated_types.values());
}
