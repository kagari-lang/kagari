//! Native representation constructors carried by public type contracts.
//! These are runtime shape facts, not a source declaration or name-resolution table.

use crate::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    numeric,
    scalar::BuiltinType,
    standard::{bindings::NativeProtocolMethod, surface::StandardEnum, traits::StandardTrait},
    types::{AbiType, InterfaceTableAbi, TypeAbi, verify::engine_signature_valid},
};
use kagari_common::{collection::CollectionAccess, identity::DefinitionKind, range::RangeKind};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeTypeConstructor {
    String,
    Array,
    Map,
    Set,
    Iter,
    Range(RangeKind),
    Enum(StandardEnum),
}

impl NativeTypeConstructor {
    pub fn arity(self) -> usize {
        match self {
            Self::String | Self::Range(RangeKind::Full) => 0,
            Self::Map => 2,
            Self::Array | Self::Set | Self::Iter | Self::Range(_) => 1,
            Self::Enum(kind) => kind.arity(),
        }
    }

    pub(crate) fn declaration_kind(self) -> DefinitionKind {
        match self {
            Self::Enum(_) => DefinitionKind::Enum,
            _ => DefinitionKind::AssociatedType,
        }
    }

    pub(crate) fn shape_valid(self, declaration: &TypeAbi) -> bool {
        if !declaration.fields.is_empty() || declaration.generic_params.len() != self.arity() {
            return false;
        }
        let Self::Enum(kind) = self else {
            return declaration.variants.is_empty();
        };
        kind.variants().len() == declaration.variants.len()
            && kind
                .variants()
                .iter()
                .zip(&declaration.variants)
                .all(|(expected, actual)| match expected.payload() {
                    None => actual.payload.is_empty(),
                    Some(slot) => {
                        actual.payload.as_slice() == [declaration.generic_params[slot].as_type()]
                    }
                })
    }
}

/// Native implementation records grant only the storage families consumed by the
/// engine adapters. Generic arguments and associated outputs remain carried facts;
/// linked trait validation separately checks every method and required bound.
pub fn engine_implementation_shape(table: &InterfaceTableAbi) -> bool {
    let AbiType::Trait(interface) = &table.trait_type else {
        return false;
    };
    if table.host_bridge
        || table.methods.is_empty()
        || !table.methods.iter().all(|method| {
            let CallableImplementation::Native(NativeBinding::Engine(binding)) =
                method.implementation
            else {
                return false;
            };
            engine_signature_valid(method, &table.bounds)
                && match binding {
                    EngineNativeBinding::Protocol(
                        NativeProtocolMethod::CollectionFromIterator
                        | NativeProtocolMethod::OptionFromIterator
                        | NativeProtocolMethod::ResultFromIterator
                        | NativeProtocolMethod::NumericSum
                        | NativeProtocolMethod::NumericProduct,
                    ) => method.return_type == table.for_type,
                    EngineNativeBinding::Protocol(NativeProtocolMethod::NumericFromStr) => {
                        matches!(&method.return_type, AbiType::StandardEnum {
                        kind: StandardEnum::Result, args
                    } if args.first() == Some(&table.for_type))
                    }
                    _ => method
                        .params
                        .first()
                        .is_some_and(|param| param.ty == table.for_type),
                }
        })
    {
        return false;
    }
    let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
        return false;
    };
    match (kind, &table.for_type, interface.arguments.as_slice()) {
        (
            StandardTrait::List | StandardTrait::MutableList,
            AbiType::Array(item, CollectionAccess::Mutable),
            [input],
        )
        | (
            StandardTrait::Set | StandardTrait::MutableSet,
            AbiType::Set(item, CollectionAccess::Mutable),
            [input],
        ) => item.as_ref() == input,
        (
            StandardTrait::Map | StandardTrait::MutableMap,
            AbiType::Map {
                key,
                value,
                access: CollectionAccess::Mutable,
            },
            [k, v],
        ) => key.as_ref() == k && value.as_ref() == v,
        (
            StandardTrait::Iterable,
            AbiType::Array(_, _)
            | AbiType::Map { .. }
            | AbiType::Set(_, _)
            | AbiType::Builtin(BuiltinType::String)
            | AbiType::Range(_, RangeKind::Exclusive | RangeKind::Inclusive | RangeKind::From),
            [],
        ) => true,
        (StandardTrait::Iterator, AbiType::Iter(_), []) => true,
        (StandardTrait::RangeBounds, AbiType::Range(item, kind), [input]) => {
            *kind == RangeKind::Full || item.as_ref() == input
        }
        (StandardTrait::Sum | StandardTrait::Product, AbiType::Builtin(scalar), [input]) => {
            scalar.number_type().is_some()
                && input == &table.for_type
                && table.methods.len() == 1
                && table.methods[0].implementation
                    == CallableImplementation::Native(NativeBinding::Engine(
                        EngineNativeBinding::Protocol(if kind == StandardTrait::Sum {
                            NativeProtocolMethod::NumericSum
                        } else {
                            NativeProtocolMethod::NumericProduct
                        }),
                    ))
        }
        (StandardTrait::FromStr, AbiType::Builtin(scalar), []) => {
            numeric::parsing_error(*scalar).is_some()
        }
        (
            StandardTrait::FromIterator,
            AbiType::Array(item, CollectionAccess::Mutable)
            | AbiType::Set(item, CollectionAccess::Mutable),
            [input],
        ) => item.as_ref() == input,
        (
            StandardTrait::FromIterator,
            AbiType::Map {
                key,
                value,
                access: CollectionAccess::Mutable,
            },
            [AbiType::Tuple(items)],
        ) => items.as_slice() == [key.as_ref().clone(), value.as_ref().clone()],
        (
            StandardTrait::FromIterator,
            AbiType::StandardEnum {
                kind: StandardEnum::Option,
                args,
            },
            [
                AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args: inputs,
                },
            ],
        ) => args.len() == 1 && inputs.len() == 1,
        (
            StandardTrait::FromIterator,
            AbiType::StandardEnum {
                kind: StandardEnum::Result,
                args,
            },
            [
                AbiType::StandardEnum {
                    kind: StandardEnum::Result,
                    args: inputs,
                },
            ],
        ) => args.len() == 2 && inputs.len() == 2 && args[1] == inputs[1],
        _ => false,
    }
}
