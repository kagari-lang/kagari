//! Concrete capabilities implemented by the engine's native storage families.
use crate::scalar::BuiltinType;
use crate::standard::traits::StandardTrait;
use crate::types::{AbiType, NominalAbiType};
use kagari_common::collection::CollectionAccess;
use kagari_common::identity::{DefinitionId, associated_type_id};
use kagari_common::range::RangeKind;
use std::collections::BTreeMap;

pub(crate) fn interface_applies(interface: &NominalAbiType, receiver: &AbiType) -> bool {
    let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
        return false;
    };
    if !kind.dynamic() || matches!(receiver, AbiType::Trait(_)) {
        return false;
    }
    if kind == StandardTrait::Iterable {
        return interface.arguments.is_empty()
            && iteration_outputs(&interface.declaration, receiver).is_some_and(|outputs| {
                interface
                    .associated_types
                    .iter()
                    .all(|(member, ty)| outputs.get(member) == Some(ty))
            });
    }
    if kind == StandardTrait::Index {
        let AbiType::Array(element, _) = receiver else {
            return false;
        };
        return matches!(interface.arguments.as_slice(), [AbiType::Builtin(index)] if index.integer_layout().is_some())
            && interface.associated_types.iter().all(|(member, ty)| {
                *member == associated_type_id(&interface.declaration, "Output")
                    && ty == element.as_ref()
            });
    }
    if !interface.associated_types.is_empty() {
        return false;
    }
    match (kind, receiver, interface.arguments.as_slice()) {
        (
            StandardTrait::List | StandardTrait::MutableList,
            AbiType::Array(element, access),
            [item],
        ) => {
            element.as_ref() == item
                && (kind == StandardTrait::List || *access == CollectionAccess::Mutable)
        }
        (StandardTrait::Set | StandardTrait::MutableSet, AbiType::Set(element, access), [item]) => {
            element.as_ref() == item
                && (kind == StandardTrait::Set || *access == CollectionAccess::Mutable)
        }
        (
            StandardTrait::Map | StandardTrait::MutableMap,
            AbiType::Map { key, value, access },
            [k, v],
        ) => {
            key.as_ref() == k
                && value.as_ref() == v
                && (kind == StandardTrait::Map || *access == CollectionAccess::Mutable)
        }
        _ => false,
    }
}

fn iteration_outputs(
    owner: &DefinitionId,
    receiver: &AbiType,
) -> Option<BTreeMap<DefinitionId, AbiType>> {
    let item = match receiver {
        AbiType::Array(item, _)
        | AbiType::Set(item, _)
        | AbiType::Iter(item)
        | AbiType::Range(item, RangeKind::Exclusive | RangeKind::Inclusive | RangeKind::From) => {
            item.as_ref().clone()
        }
        AbiType::Map { key, value, .. } => {
            AbiType::Tuple(vec![key.as_ref().clone(), value.as_ref().clone()])
        }
        AbiType::Builtin(BuiltinType::String) => receiver.clone(),
        _ => return None,
    };
    Some(
        [
            (associated_type_id(owner, "Item"), item.clone()),
            (
                associated_type_id(owner, "Iter"),
                AbiType::Iter(Box::new(item)),
            ),
        ]
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standard::traits;

    fn applied(kind: StandardTrait, arguments: Vec<AbiType>) -> NominalAbiType {
        NominalAbiType {
            declaration: traits::identity(kind),
            arguments,
            associated_types: BTreeMap::new(),
        }
    }

    #[test]
    fn native_collection_bridges_preserve_access_and_invariant_arguments() {
        let item = AbiType::Builtin(BuiltinType::I32);
        let readonly = AbiType::Array(Box::new(item.clone()), CollectionAccess::ReadOnly);
        let mutable = AbiType::Array(Box::new(item.clone()), CollectionAccess::Mutable);
        let list = applied(StandardTrait::List, vec![item.clone()]);
        let writable = applied(StandardTrait::MutableList, vec![item]);
        assert!(interface_applies(&list, &readonly));
        assert!(interface_applies(&list, &mutable));
        assert!(!interface_applies(&writable, &readonly));
        assert!(interface_applies(&writable, &mutable));
        assert!(!interface_applies(
            &applied(
                StandardTrait::List,
                vec![AbiType::Builtin(BuiltinType::Bool)]
            ),
            &mutable
        ));
        assert!(!interface_applies(&list, &AbiType::Trait(list.clone())));
    }

    #[test]
    fn native_iteration_and_index_bridges_validate_associated_outputs() {
        let item = AbiType::Builtin(BuiltinType::String);
        let storage = AbiType::Array(Box::new(item.clone()), CollectionAccess::ReadOnly);
        let mut iterable = applied(StandardTrait::Iterable, vec![]);
        iterable.associated_types = iteration_outputs(&iterable.declaration, &storage).unwrap();
        assert!(interface_applies(&iterable, &storage));
        iterable.associated_types.insert(
            associated_type_id(&iterable.declaration, "Item"),
            AbiType::Builtin(BuiltinType::Bool),
        );
        assert!(!interface_applies(&iterable, &storage));
        let mut index = applied(
            StandardTrait::Index,
            vec![AbiType::Builtin(BuiltinType::U64)],
        );
        index
            .associated_types
            .insert(associated_type_id(&index.declaration, "Output"), item);
        assert!(interface_applies(&index, &storage));
        index.arguments[0] = AbiType::Builtin(BuiltinType::F64);
        assert!(!interface_applies(&index, &storage));
    }

    #[test]
    fn generated_dynamic_implementation_outputs_match_native_storage_contracts() {
        use crate::standard::resolve::Arguments;
        use crate::standard::surface::STANDARD_IMPLEMENTATIONS;
        for declaration in STANDARD_IMPLEMENTATIONS {
            let Some(kind) =
                StandardTrait::from_name(declaration.interface).filter(|kind| kind.dynamic())
            else {
                continue;
            };
            let arguments: Arguments = declaration
                .generics
                .iter()
                .map(|name| (*name, AbiType::Builtin(BuiltinType::I32)))
                .collect();
            let receiver = declaration.target.resolve(&arguments).unwrap();
            let mut interface = applied(
                kind,
                declaration
                    .trait_arguments
                    .iter()
                    .map(|ty| ty.resolve(&arguments).unwrap())
                    .collect(),
            );
            interface.associated_types = declaration
                .associated_types
                .iter()
                .map(|(member, ty)| {
                    (
                        associated_type_id(&interface.declaration, member.path.last().unwrap().1),
                        ty.resolve(&arguments).unwrap(),
                    )
                })
                .collect();
            assert!(
                interface_applies(&interface, &receiver),
                "{} for {:?}",
                declaration.interface,
                receiver
            );
        }
    }
}
