//! Checked argument projection recipes for receiver comparison dispatch.
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use kagari_types::ty::{GenericParam, Ty, substitution::TypeTransformError};
use std::iter;

/// One explicit implementation and the receiver paths supplying its parameters.
/// Paths are checked against the declaration before execution. Runtime projects
/// arguments and compares complete type identities; it does not infer bindings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReceiverCase {
    pub implementation: DefinitionId,
    pub arguments: Vec<Vec<ReceiverTypeStep>>,
}

/// Positional children and keyed associated outputs survive definition remapping.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReceiverTypeStep {
    Child(usize),
    Associated(DefinitionId),
}

/// Ordered structural children used only by comparison argument recipes.
/// Positional function parameters precede the result. Associated outputs use
/// their definition IDs. Whole-type identity checks every constructor afterward.
pub fn receiver_child<'a>(ty: &'a Ty, step: &ReceiverTypeStep) -> Option<&'a Ty> {
    let index = match step {
        ReceiverTypeStep::Child(index) => *index,
        ReceiverTypeStep::Associated(member) => {
            return match ty {
                Ty::Struct(nominal)
                | Ty::NativeObject(nominal)
                | Ty::Enum(nominal)
                | Ty::Trait(nominal) => nominal.associated_types.get(member),
                _ => None,
            };
        }
    };
    match ty {
        Ty::Struct(nominal)
        | Ty::NativeObject(nominal)
        | Ty::Enum(nominal)
        | Ty::Trait(nominal) => nominal.arguments.get(index),
        Ty::Tuple(items) => items.get(index),
        Ty::Function { params, result } => {
            params.iter().chain(iter::once(result.as_ref())).nth(index)
        }
        Ty::Array(item) | Ty::Iter(item) | Ty::Range(item, _) | Ty::Set(item, _) if index == 0 => {
            Some(item)
        }
        Ty::Map { key, value, .. } => match index {
            0 => Some(key),
            1 => Some(value),
            _ => None,
        },
        _ => None,
    }
}

/// Derive a bounded first-occurrence path for each declared parameter.
/// Missing parameters cannot be supplied by receiver construction.
pub fn receiver_argument_paths(
    receiver: &Ty,
    parameters: &[GenericParam],
    cancel: &CancellationToken,
) -> Result<Option<Vec<Vec<ReceiverTypeStep>>>, TypeTransformError> {
    let mut paths = vec![None; parameters.len()];
    let mut pending = vec![(receiver, Vec::new())];
    let mut remaining = 100_000usize;
    while let Some((ty, path)) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if remaining == 0 || path.len() >= 64 {
            return Err(TypeTransformError::LimitExceeded);
        }
        remaining -= 1;
        if let Some(index) = parameters
            .iter()
            .position(|parameter| parameter.as_type() == *ty)
        {
            if paths[index].is_none() {
                paths[index] = Some(path);
            }
            continue;
        }
        let mut children = Vec::new();
        for index in 0.. {
            let Some(child) = receiver_child(ty, &ReceiverTypeStep::Child(index)) else {
                break;
            };
            let mut next = path.clone();
            next.push(ReceiverTypeStep::Child(index));
            children.push((child, next));
        }
        if let Ty::Struct(nominal)
        | Ty::NativeObject(nominal)
        | Ty::Enum(nominal)
        | Ty::Trait(nominal) = ty
        {
            for (member, child) in &nominal.associated_types {
                let mut next = path.clone();
                next.push(ReceiverTypeStep::Associated(*member));
                children.push((child, next));
            }
        }
        pending.extend(children.into_iter().rev());
    }
    Ok(paths.into_iter().collect())
}
