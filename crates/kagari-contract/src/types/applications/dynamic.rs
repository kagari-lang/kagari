//! Dynamic admission for value positions, independently of declaration binders.
use crate::callable::interface::dynamic_interface_valid;
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::TraitDef,
    ty::{
        GenericParam, Ty,
        substitution::{MAX_TYPE_NODES, TypeTransformError},
    },
};

pub(super) fn validate<'ty, 'declaration>(
    types: impl IntoIterator<Item = &'ty Ty>,
    lookup: &impl Fn(&DefinitionId) -> Option<&'declaration TraitDef>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError> {
    for root in types {
        let mut pending = vec![root];
        let mut interfaces = vec![];
        let mut parameters = vec![];
        let mut remaining = MAX_TYPE_NODES;
        while let Some(ty) = pending.pop() {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            remaining = remaining
                .checked_sub(1)
                .ok_or(TypeTransformError::LimitExceeded)?;
            match ty {
                Ty::Trait(interface) => {
                    interfaces.push(interface);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                }
                Ty::Struct(nominal) | Ty::Enum(nominal) | Ty::NativeObject(nominal) => {
                    pending.extend(&nominal.arguments);
                    pending.extend(nominal.associated_types.values());
                }
                Ty::Projection {
                    receiver,
                    interface,
                    arguments,
                    ..
                } => {
                    // The projection's trait names a static contract, not a value.
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                    pending.extend(arguments);
                }
                Ty::Parameter { owner, position } => {
                    let parameter = GenericParam {
                        owner: *owner,
                        position: *position,
                    };
                    if !parameters.contains(&parameter) {
                        parameters.push(parameter);
                    }
                }
                Ty::Tuple(items) => pending.extend(items),
                Ty::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Ty::Array(item) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => {
                    pending.push(item)
                }
                Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Ty::Builtin(_) | Ty::Host(_) | Ty::SelfType(_) => {}
            }
            if pending.len() > remaining {
                return Err(TypeTransformError::LimitExceeded);
            }
        }
        // Binder ownership is checked by declaration/body validation. Admission
        // preserves these already scoped parameters while rejecting erased Self.
        for interface in interfaces {
            if !dynamic_interface_valid(&parameters, interface, lookup, definitions, cancel)? {
                return Err(TypeTransformError::InvalidContract);
            }
        }
    }
    Ok(())
}
