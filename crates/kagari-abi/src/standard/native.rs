//! Physical storage capabilities of the trusted engine interface adapter.
//! This check grants no trait proof: the linked catalog must match the installed
//! implementation and discharge its carried bounds before publishing a bridge.
use crate::{
    standard::traits::StandardTrait,
    types::{AbiType, NominalAbiType},
};
use kagari_common::{collection::CollectionAccess, identity::associated_type_id};

pub(crate) fn interface_applies(interface: &NominalAbiType, receiver: &AbiType) -> bool {
    let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
        return false;
    };
    if !kind.dynamic() {
        return false;
    }
    let outputs = match (kind, receiver, interface.arguments.as_slice()) {
        (StandardTrait::List, AbiType::Array(item, _), [input])
        | (StandardTrait::MutableList, AbiType::Array(item, CollectionAccess::Mutable), [input])
        | (StandardTrait::Set, AbiType::Set(item, _), [input])
        | (StandardTrait::MutableSet, AbiType::Set(item, CollectionAccess::Mutable), [input])
            if item.as_ref() == input =>
        {
            vec![]
        }
        (StandardTrait::Map, AbiType::Map { key, value, .. }, [k, v])
        | (
            StandardTrait::MutableMap,
            AbiType::Map {
                key,
                value,
                access: CollectionAccess::Mutable,
            },
            [k, v],
        ) if key.as_ref() == k && value.as_ref() == v => vec![],
        (StandardTrait::Iterable, AbiType::Iter(item), []) => {
            vec![("Item", item.as_ref().clone()), ("Iter", receiver.clone())]
        }
        (StandardTrait::Iterable, AbiType::Array(item, _) | AbiType::Set(item, _), []) => vec![
            ("Item", item.as_ref().clone()),
            ("Iter", AbiType::Iter(item.clone())),
        ],
        (StandardTrait::Iterable, AbiType::Map { key, value, .. }, []) => {
            let item = AbiType::Tuple(vec![key.as_ref().clone(), value.as_ref().clone()]);
            vec![
                ("Item", item.clone()),
                ("Iter", AbiType::Iter(Box::new(item))),
            ]
        }
        (StandardTrait::Index, AbiType::Array(item, _), [AbiType::Builtin(index)])
            if index.integer_layout().is_some() =>
        {
            vec![("Output", item.as_ref().clone())]
        }
        _ => return false,
    };
    interface.associated_types.iter().all(|(member, ty)| {
        outputs.iter().any(|(name, output)| {
            *member == associated_type_id(&interface.declaration, name) && ty == output
        })
    })
}

#[cfg(test)]
mod tests;
