//! Concrete capabilities implemented by the engine's native storage families.
use crate::standard::surface::STANDARD_IMPLEMENTATIONS;
use crate::standard::traits::StandardTrait;
use crate::standard::{implementation, intrinsic};
use crate::types::{AbiType, NominalAbiType};
use kagari_common::cancellation::CancellationToken;

/// Match native dispatch capability and associated outputs. The linked verifier
/// separately proves collection key bounds against the complete implementation
/// catalog; local capability matching cannot resolve nominal Eq/Hash overrides.
pub(crate) fn interface_applies(interface: &NominalAbiType, receiver: &AbiType) -> bool {
    let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
        return false;
    };
    if !kind.dynamic() || matches!(receiver, AbiType::Trait(_)) {
        return false;
    }
    let cancel = CancellationToken::default();
    if kind == StandardTrait::Index {
        return intrinsic::requirements(interface, receiver, &cancel)
            .is_ok_and(|requirements| requirements == Some(vec![]));
    }
    if kind == StandardTrait::Iterable && matches!(receiver, AbiType::Iter(_)) {
        let Some(iterator) = intrinsic::identity_iterator(interface, receiver) else {
            return false;
        };
        return intrinsic::requirements(&iterator, receiver, &cancel)
            .is_ok_and(|requirements| requirements == Some(vec![]));
    }
    STANDARD_IMPLEMENTATIONS
        .iter()
        .filter(|declaration| declaration.interface == kind.name())
        .any(|declaration| {
            implementation::match_application(declaration, interface, receiver, &cancel)
                .is_ok_and(|bindings| bindings.is_some())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scalar::BuiltinType;
    use crate::standard::traits;
    use kagari_common::collection::CollectionAccess;
    use kagari_common::identity::associated_type_id;
    use std::collections::BTreeMap;

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
        for name in ["Item", "Iter"] {
            let member = associated_type_id(&iterable.declaration, name);
            let value = intrinsic::associated_output(
                &iterable,
                &storage,
                &member,
                &CancellationToken::default(),
            )
            .unwrap()
            .unwrap();
            iterable.associated_types.insert(member, value);
        }
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
