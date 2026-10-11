//! Preserve each inferred impl argument's lexical scope when preparing a member.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{binding::NativeResult, methods::InherentMember},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{Ty, substitution::MAX_TYPE_NODES};

impl InherentMember {
    pub(crate) fn arguments(
        &self,
        runtime: &Runtime,
        method_arguments: &[TypeArgument],
    ) -> NativeResult<Vec<TypeArgument>> {
        let invalid = || RuntimeError::module_validation("inherent method type arguments");
        if method_arguments.len() != self.method_arity {
            return Err(invalid());
        }
        for argument in method_arguments {
            argument.validate(runtime)?;
        }
        if self.parameters.is_empty() {
            return Ok(method_arguments.to_vec());
        }
        let owner = self.applied.owner();
        let mut arguments: Vec<Option<TypeArgument>> = vec![None; self.parameters.len()];
        let mut pending = vec![(&self.receiver, self.applied.type_argument().clone())];
        let mut remaining = MAX_TYPE_NODES;
        while let Some((template, actual)) = pending.pop() {
            if remaining == 0 {
                return Err(invalid());
            }
            remaining -= 1;
            if let Ty::Parameter {
                owner: binder,
                position,
            } = template
                && let Some(index) = self.parameters.iter().position(|parameter| {
                    parameter.owner == *binder && parameter.position == *position
                })
            {
                if let Some(previous) = &arguments[index] {
                    // Equal nominal IDs are insufficient across retained generations.
                    if !previous.view(owner).compatible(actual.view(owner)) {
                        return Err(invalid());
                    }
                } else {
                    arguments[index] = Some(actual);
                }
                continue;
            }
            // Semantic matching already established the shape. Descend one level
            // at a time so each parameter resolves in its own supplying scope.
            for (index, child) in children(template).into_iter().enumerate() {
                let scoped = actual.derive(runtime, owner, |ty| {
                    children(ty).get(index).map(|child| (*child).clone())
                })?;
                pending.push((child, scoped));
            }
            if pending.len() > MAX_TYPE_NODES {
                return Err(invalid());
            }
        }
        let mut arguments = arguments
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        arguments.extend_from_slice(method_arguments);
        Ok(arguments)
    }
}

fn children(ty: &Ty<DefinitionId>) -> Vec<&Ty<DefinitionId>> {
    match ty {
        Ty::Struct(ty) | Ty::Enum(ty) | Ty::NativeObject(ty) | Ty::Trait(ty) => ty
            .arguments
            .iter()
            .chain(ty.associated_types.values())
            .collect(),
        Ty::Tuple(items) => items.iter().collect(),
        Ty::Function { params, result } => params.iter().chain([result.as_ref()]).collect(),
        Ty::Array(item) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => vec![item],
        Ty::Map { key, value, .. } => vec![key, value],
        _ => Vec::new(),
    }
}
