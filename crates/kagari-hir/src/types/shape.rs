//! Bounds for converting independently constructed analysis types.
use crate::types::{NominalType, TypeId};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        reference::DefinitionReference,
    },
};

pub(super) fn validate<I: DefinitionReference>(
    ty: &TypeId<I>,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    let mut pending = vec![(ty, 0)];
    let mut nodes = 0;
    while let Some((ty, depth)) = pending.pop() {
        check_cancel(cancel)?;
        nodes += 1;
        if nodes > 4096 || depth > 64 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        let next = depth + 1;
        match ty {
            TypeId::Tuple(items) => append(items.iter(), next, &mut pending)?,
            TypeId::Function { params, result } => {
                append(params.iter(), next, &mut pending)?;
                pending.push((result, next));
            }
            TypeId::Iter(item)
            | TypeId::Range(item, _)
            | TypeId::Array(item)
            | TypeId::Set(item, _) => pending.push((item, next)),
            TypeId::Map { key, value, .. } => {
                pending.push((key, next));
                pending.push((value, next));
            }
            TypeId::NativeObject(ty)
            | TypeId::Struct(ty)
            | TypeId::Enum(ty)
            | TypeId::Trait(ty) => nominal(ty, next, &mut pending)?,
            TypeId::Projection {
                arguments,
                receiver,
                interface,
                ..
            } => {
                append(arguments.iter(), next, &mut pending)?;
                pending.push((receiver, next));
                nominal(interface, next, &mut pending)?;
            }
            TypeId::Inference(_)
            | TypeId::Unknown
            | TypeId::Error
            | TypeId::Builtin(_)
            | TypeId::Host(_)
            | TypeId::Generic(_)
            | TypeId::SelfType(_) => {}
        }
        if pending.len() > 4096 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
    }
    Ok(())
}

fn nominal<'a, I: DefinitionReference>(
    ty: &'a NominalType<I>,
    depth: usize,
    pending: &mut Vec<(&'a TypeId<I>, usize)>,
) -> Result<(), DefinitionMappingError> {
    append(
        ty.arguments.iter().chain(ty.associated_types.values()),
        depth,
        pending,
    )
}

fn append<'a, I: DefinitionReference>(
    items: impl Iterator<Item = &'a TypeId<I>>,
    depth: usize,
    pending: &mut Vec<(&'a TypeId<I>, usize)>,
) -> Result<(), DefinitionMappingError> {
    for item in items {
        if pending.len() == 4096 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        pending.push((item, depth));
    }
    Ok(())
}
