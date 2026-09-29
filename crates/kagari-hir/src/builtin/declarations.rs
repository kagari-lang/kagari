//! Public signatures compiled from the bundled declaration sources.

use crate::builtin::traits::StandardTraitSemantics;
use kagari_abi::{
    scalar::BuiltinType,
    standard::{
        declarations::{ApiBound, ApiImplementation, ApiType},
        surface::{self as standard_surface, STANDARD_IMPLEMENTATIONS, StandardEnum},
        traits::{self as standard_traits, StandardTrait},
    },
};
use kagari_common::{collection::CollectionAccess, identity};

use crate::types::{NominalType, TypeId};

use super::surface;
use std::collections::BTreeMap;

pub trait ApiImplementationSemantics {
    fn arguments(&self, receiver: &TypeId) -> Option<Arguments>;
    fn applied_arguments(&self, receiver: &TypeId, interface: &NominalType) -> Option<Arguments>;
}
impl ApiImplementationSemantics for ApiImplementation {
    fn arguments(&self, receiver: &TypeId) -> Option<Arguments> {
        let mut arguments = self
            .generics
            .iter()
            .map(|name| (*name, TypeId::Unknown))
            .collect();
        self.target.infer(receiver, &mut arguments);
        arguments.insert("Self", receiver.clone());
        let instantiated = self.target.instantiate(&arguments);
        (instantiated == *receiver
            || matches!(self.interface, "Iterable" | "List" | "Map" | "Set")
                && instantiated.can_weaken_to(receiver))
        .then_some(arguments)
    }

    fn applied_arguments(&self, receiver: &TypeId, interface: &NominalType) -> Option<Arguments> {
        if self.trait_declaration().item.identity() != interface.declaration
            || !interface.associated_types.is_empty()
            || self.trait_arguments.len() != interface.arguments.len()
        {
            return None;
        }
        let mut arguments = self.arguments(receiver)?;
        for (declared, actual) in self.trait_arguments.iter().zip(&interface.arguments) {
            declared.infer(actual, &mut arguments);
        }
        ((self.target.instantiate(&arguments) == *receiver
            || matches!(self.interface, "Iterable" | "List" | "Map" | "Set")
                && self.target.instantiate(&arguments).can_weaken_to(receiver))
            && self
                .trait_arguments
                .iter()
                .zip(&interface.arguments)
                .all(|(a, b)| a.instantiate(&arguments) == *b))
        .then_some(arguments)
    }
}

/// Explicit native implementations applicable to a checked receiver type.
pub fn implementations(receiver: &TypeId) -> Vec<&'static ApiImplementation> {
    STANDARD_IMPLEMENTATIONS
        .iter()
        .filter(|i| i.arguments(receiver).is_some())
        .collect()
}

pub type Arguments = BTreeMap<&'static str, TypeId>;

pub trait ApiTypeSemantics {
    fn instantiate(&self, arguments: &Arguments) -> TypeId;
    fn infer(&self, actual: &TypeId, arguments: &mut Arguments);
}
impl ApiTypeSemantics for ApiType {
    fn instantiate(&self, arguments: &Arguments) -> TypeId {
        match self {
            Self::Projection(receiver, bound, member) => {
                let interface = bound.nominal(arguments);
                TypeId::Projection {
                    receiver: Box::new(receiver.instantiate(arguments)),
                    member: identity::associated_type_id(&interface.declaration, member),
                    interface: Box::new(interface),
                    arguments: vec![],
                }
            }
            Self::Array(element) => {
                let mut interface = StandardTrait::List.nominal();
                interface.arguments.push(element.instantiate(arguments));
                TypeId::Trait(interface)
            }
            Self::Tuple([]) => TypeId::Builtin(BuiltinType::Unit),
            Self::Tuple(items) => {
                TypeId::Tuple(items.iter().map(|t| t.instantiate(arguments)).collect())
            }
            Self::Function(params, result) => TypeId::Function {
                params: params.iter().map(|t| t.instantiate(arguments)).collect(),
                result: Box::new(result.instantiate(arguments)),
            },
            Self::Named(name, params) => {
                if let Some(value) = arguments.get(name) {
                    return value.clone();
                }
                if let Some(member) = name.strip_prefix("Self::")
                    && let Some(TypeId::Trait(interface)) = arguments.get("@self_trait")
                {
                    return TypeId::Projection {
                        receiver: Box::new(TypeId::SelfType(interface.declaration.clone())),
                        interface: Box::new(interface.clone()),
                        member: identity::associated_type_id(&interface.declaration, member),
                        arguments: params.iter().map(|p| p.instantiate(arguments)).collect(),
                    };
                }
                if let Some((owner, "Item")) = name.split_once("::") {
                    if let Some(TypeId::Trait(interface)) =
                        arguments.get(format!("@bound:{owner}").as_str())
                    {
                        return TypeId::Projection {
                            receiver: Box::new(arguments[owner].clone()),
                            interface: Box::new(interface.clone()),
                            member: identity::associated_type_id(&interface.declaration, "Item"),
                            arguments: vec![],
                        };
                    }
                    return TypeId::Unknown;
                }
                if let Some(builtin) = standard_surface::builtin_type(name) {
                    return TypeId::Builtin(builtin);
                }
                let types = params
                    .iter()
                    .map(|t| t.instantiate(arguments))
                    .collect::<Vec<_>>();
                if standard_surface::range_kind(name).is_some() {
                    return surface::standard_generic_type(name, types).unwrap_or(TypeId::Error);
                }
                if let Some(kind) = StandardTrait::from_name(name) {
                    let mut interface = kind.nominal();
                    interface.arguments = types;
                    return TypeId::Trait(interface);
                }
                match (*name, types.as_slice()) {
                    ("ArrayList" | "LinkedHashMap" | "LinkedHashSet", _) => {
                        surface::standard_generic_type(name, types).unwrap_or(TypeId::Error)
                    }
                    ("Iter", [item]) => TypeId::Iter(Box::new(item.clone())),
                    ("Bound", [_]) => TypeId::StandardEnum {
                        kind: StandardEnum::Bound,
                        args: types,
                    },
                    ("Option", [_]) => TypeId::StandardEnum {
                        kind: StandardEnum::Option,
                        args: types,
                    },
                    ("Result", [_, _]) => TypeId::StandardEnum {
                        kind: StandardEnum::Result,
                        args: types,
                    },
                    ("ParseError", []) => TypeId::StandardEnum {
                        kind: StandardEnum::ParseError,
                        args: vec![],
                    },
                    ("TryFromIntError", []) => TypeId::StandardEnum {
                        kind: StandardEnum::TryFromIntError,
                        args: types,
                    },
                    ("Infallible", []) => TypeId::StandardEnum {
                        kind: StandardEnum::Infallible,
                        args: types,
                    },
                    ("Ordering", []) => TypeId::StandardEnum {
                        kind: StandardEnum::Ordering,
                        args: types,
                    },
                    _ => TypeId::Error,
                }
            }
        }
    }

    /// Infer only declared parameters; concrete mismatches remain diagnostics.
    fn infer(&self, actual: &TypeId, arguments: &mut Arguments) {
        match (self, actual) {
            (Self::Named(name, params), TypeId::Trait(interface))
                if StandardTrait::from_id(&interface.declaration).is_some_and(
                    |kind| match *name {
                        "ArrayList" => {
                            matches!(kind, StandardTrait::List | StandardTrait::MutableList)
                        }
                        "LinkedHashMap" => {
                            matches!(kind, StandardTrait::Map | StandardTrait::MutableMap)
                        }
                        "LinkedHashSet" => {
                            matches!(kind, StandardTrait::Set | StandardTrait::MutableSet)
                        }
                        _ => kind.name() == *name,
                    },
                ) && params.len() == interface.arguments.len() =>
            {
                for (param, actual) in params.iter().zip(&interface.arguments) {
                    param.infer(actual, arguments);
                }
            }
            (Self::Array(item), TypeId::Trait(interface))
                if matches!(
                    StandardTrait::from_id(&interface.declaration),
                    Some(StandardTrait::List | StandardTrait::MutableList)
                ) =>
            {
                if let [actual] = interface.arguments.as_slice() {
                    item.infer(actual, arguments);
                }
            }

            (Self::Named(name, []), actual) if arguments.contains_key(name) => {
                let argument = arguments.get_mut(name).unwrap();
                if *argument == TypeId::Unknown {
                    *argument = actual.clone();
                } else {
                    argument.recover_from(actual);
                }
            }
            (Self::Named(name, [item]), TypeId::Range(actual, kind)) if *name == kind.name() => {
                item.infer(actual, arguments)
            }
            (Self::Array(element), TypeId::Array(actual, _)) => element.infer(actual, arguments),
            (Self::Named("ArrayList", [element]), TypeId::Array(actual, _)) => {
                element.infer(actual, arguments)
            }
            (Self::Tuple(items), TypeId::Tuple(actual)) if items.len() == actual.len() => {
                for (item, actual) in items.iter().zip(actual) {
                    item.infer(actual, arguments);
                }
            }
            (
                Self::Function(params, result),
                TypeId::Function {
                    params: actual,
                    result: output,
                },
            ) if params.len() == actual.len() => {
                for (item, actual) in params.iter().zip(actual) {
                    item.infer(actual, arguments);
                }
                result.infer(output, arguments);
            }
            (
                Self::Named("Map" | "LinkedHashMap", [key, value]),
                TypeId::Map {
                    key: actual,
                    value: output,
                    ..
                },
            ) => {
                key.infer(actual, arguments);
                value.infer(output, arguments);
            }
            (Self::Named("Set" | "LinkedHashSet", [item]), TypeId::Set(actual, _))
            | (Self::Named("Iter", [item]), TypeId::Iter(actual)) => item.infer(actual, arguments),
            (Self::Named(name, params), TypeId::StandardEnum { kind, args })
                if *name == kind.spec().name && params.len() == args.len() =>
            {
                for (item, actual) in params.iter().zip(args) {
                    item.infer(actual, arguments);
                }
            }
            _ => {}
        }
    }
}

pub trait ApiBoundSemantics {
    fn nominal(&self, arguments: &Arguments) -> NominalType;
}
impl ApiBoundSemantics for ApiBound {
    fn nominal(&self, arguments: &Arguments) -> NominalType {
        let kind = StandardTrait::from_name(self.name).expect("standard trait bound");
        let id = standard_traits::identity(kind);
        NominalType {
            declaration: id.clone(),
            arguments: self.args.iter().map(|p| p.instantiate(arguments)).collect(),
            associated_types: self
                .bindings
                .iter()
                .map(|(name, ty)| {
                    (
                        identity::associated_type_id(&id, name),
                        ty.instantiate(arguments),
                    )
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_native_signature_instantiates_without_unresolved_public_types() {
        for spec in kagari_abi::standard::surface::standard_functions() {
            let arguments: Arguments = spec
                .type_params
                .iter()
                .map(|name| {
                    let ty = if *name == "I" {
                        TypeId::Array(
                            Box::new(TypeId::Builtin(BuiltinType::I32)),
                            CollectionAccess::Mutable,
                        )
                    } else {
                        TypeId::Builtin(BuiltinType::I32)
                    };
                    (*name, ty)
                })
                .collect();
            assert_eq!(spec.arity, spec.api.params.len());
            for parameter in spec.api.params {
                assert!(
                    !parameter.ty.instantiate(&arguments).is_unresolved(),
                    "{}::{}",
                    spec.api.qualified_name,
                    parameter.name
                );
            }
            assert!(
                !spec.api.result.instantiate(&arguments).is_unresolved(),
                "{}",
                spec.api.qualified_name
            );
        }
    }

    #[test]
    fn native_enum_declarations_match_runtime_discriminants_and_payloads() {
        for (name, arity, expected) in [
            ("Option", 1, vec![("Some", 1), ("None", 0)]),
            ("Result", 2, vec![("Ok", 1), ("Err", 1)]),
            (
                "Ordering",
                0,
                vec![("Less", 0), ("Equal", 0), ("Greater", 0)],
            ),
        ] {
            let spec = kagari_abi::standard::surface::standard_enum(name).unwrap();
            assert_eq!(spec.arity, arity);
            assert_eq!(
                spec.variants
                    .iter()
                    .map(|v| (v.name, v.payload_arity))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
