//! Public signatures compiled from the bundled declaration sources.

use crate::builtin::traits::StandardTraitSemantics;
use kagari_abi::{
    scalar::BuiltinType,
    standard::{
        StandardIntrinsic,
        declarations::{ApiBound, ApiImplementation, ApiItem, ApiTrait, ApiType},
        surface::{
            self as standard_surface, STANDARD_IMPLEMENTATIONS, STANDARD_ITEMS, StandardEnum,
            StandardVariant,
        },
        traits::{self as standard_traits, StandardTrait},
    },
};
use kagari_common::{SourceFile, Span, collection::CollectionAccess, identity};
use kagari_stdlib::bundled_sources;

use crate::{
    aggregates::{MethodParameter, MethodSignature, TraitSignature},
    declarations::{Declaration, DeclarationId},
    hir::Writeability,
    resolver::ResolvedName,
    typeck::{ConstraintTarget, GenericBounds},
    types::{GenericParameterType, NominalType, TypeId},
};

use super::surface;
use std::{collections::BTreeMap, sync::OnceLock};

pub fn sources() -> &'static [SourceFile] {
    static SOURCES: OnceLock<Vec<SourceFile>> = OnceLock::new();
    SOURCES.get_or_init(|| {
        bundled_sources()
            .iter()
            .map(|source| SourceFile::new(source.uri(), source.text()))
            .collect()
    })
}

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

pub trait ApiItemSemantics {
    fn declaration(&self) -> Declaration;
}
impl ApiItemSemantics for ApiItem {
    fn declaration(&self) -> Declaration {
        let source = sources()
            .iter()
            .find(|source| source.name() == self.uri)
            .expect("bundled declaration source");
        Declaration {
            id: DeclarationId::Definition(self.identity()),
            name: self.path.last().unwrap().1.into(),
            location: source
                .span(Span::new(self.start, self.end))
                .expect("declaration source span"),
        }
    }
}

/// Look up public metadata by semantic identity, including trait members.
pub fn item(id: &DeclarationId) -> Option<&'static ApiItem> {
    let DeclarationId::Definition(id) = id else {
        return None;
    };
    STANDARD_ITEMS.iter().find(|item| item.identity() == *id)
}

pub fn declaration(id: &DeclarationId) -> Option<&'static Declaration> {
    static DECLARATIONS: OnceLock<Vec<Declaration>> = OnceLock::new();
    DECLARATIONS
        .get_or_init(|| STANDARD_ITEMS.iter().map(ApiItem::declaration).collect())
        .iter()
        .find(|d| d.id == *id)
}

pub fn function(intrinsic: StandardIntrinsic) -> Option<&'static ApiItem> {
    let api = standard_surface::standard_function_by_intrinsic(intrinsic)?.api;
    STANDARD_ITEMS
        .iter()
        .find(|item| item.uri == api.uri && item.start == api.start)
}

pub fn variant(variant: StandardVariant) -> Option<&'static ApiItem> {
    let spec = variant.kind().spec();
    STANDARD_ITEMS.iter().find(|item| {
        item.path.len() == 2
            && item.path[0].1 == spec.name
            && item.path[1].1 == spec.variants[variant.index()].name
    })
}

pub fn native_type(ty: &TypeId) -> Option<&'static ApiItem> {
    let name = match ty {
        TypeId::Projection { member, .. } => {
            return item(&DeclarationId::Definition(member.clone()));
        }
        TypeId::Builtin(BuiltinType::String) => "String",
        TypeId::Array(_, CollectionAccess::ReadOnly) => "List",
        TypeId::Array(_, CollectionAccess::Mutable) => "ArrayList",
        TypeId::Map {
            access: CollectionAccess::ReadOnly,
            ..
        } => "Map",
        TypeId::Map {
            access: CollectionAccess::Mutable,
            ..
        } => "LinkedHashMap",
        TypeId::Set(_, CollectionAccess::ReadOnly) => "Set",
        TypeId::Set(_, CollectionAccess::Mutable) => "LinkedHashSet",
        TypeId::Iter(_) => "Iter",
        TypeId::Range(_, kind) => kind.name(),
        TypeId::StandardEnum { kind, .. } => kind.spec().name,
        _ => return None,
    };
    STANDARD_ITEMS
        .iter()
        .find(|item| item.path.len() == 1 && item.path[0].1 == name)
}

pub fn resolved(name: ResolvedName) -> Option<&'static Declaration> {
    let item = match name {
        ResolvedName::StandardFunction(intrinsic) => function(intrinsic)?,
        ResolvedName::StandardVariant(kind) => variant(kind)?,
        ResolvedName::StandardTrait(kind) => return Some(&kind.contract().declaration),
        _ => return None,
    };
    declaration(&DeclarationId::Definition(item.identity()))
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

pub trait ApiTraitSemantics {
    fn contract(&self) -> TraitSignature;
}
impl ApiTraitSemantics for ApiTrait {
    fn contract(&self) -> TraitSignature {
        let id = self.item.identity();
        let generics = self
            .generics
            .iter()
            .enumerate()
            .map(|(position, name)| GenericParameterType {
                owner: id.clone(),
                position,
                name: (*name).into(),
            })
            .collect::<Vec<_>>();
        let mut arguments: Arguments = self
            .generics
            .iter()
            .copied()
            .zip(generics.iter().cloned().map(TypeId::Generic))
            .collect();
        arguments.insert("Self", TypeId::SelfType(id.clone()));
        arguments.insert(
            "@self_trait",
            TypeId::Trait(NominalType {
                declaration: id.clone(),
                arguments: generics.iter().cloned().map(TypeId::Generic).collect(),
                associated_types: Default::default(),
            }),
        );
        TraitSignature {
            declaration: self.item.declaration(),
            id: id.clone(),
            generic_params: generics.clone(),
            bounds: Default::default(),
            supertraits: self
                .supertraits
                .iter()
                .map(|b| b.nominal(&arguments))
                .collect(),
            associated_types: self
                .associated_types
                .iter()
                .map(|a| {
                    (
                        a.item.identity(),
                        a.bounds
                            .iter()
                            .map(|b| ConstraintTarget::Trait(b.nominal(&arguments)))
                            .collect(),
                    )
                })
                .collect(),
            associated_type_parameters: Default::default(),
            associated_consts: Default::default(),
            methods: self
                .methods
                .iter()
                .enumerate()
                .map(|(slot, method)| {
                    let mut arguments = arguments.clone();
                    let mut parameters = generics.clone();
                    let mut method_bounds = GenericBounds::new();
                    for (position, generic) in method.generics.iter().enumerate() {
                        let name = generic.name;
                        let parameter = GenericParameterType {
                            owner: method.item.identity(),
                            position,
                            name: name.into(),
                        };
                        arguments.insert(name, TypeId::Generic(parameter.clone()));
                        parameters.push(parameter);
                    }
                    for generic in method.generics {
                        let (name, bounds) = (generic.name, generic.bounds);
                        if let Some(bound) = bounds
                            .iter()
                            .find(|b| matches!(b.name, "Iterator" | "Iterable"))
                        {
                            arguments.insert(
                                generic.projection_key,
                                TypeId::Trait(bound.nominal(&arguments)),
                            );
                        }
                        method_bounds.insert(
                            arguments[name].clone(),
                            bounds
                                .iter()
                                .map(|b| ConstraintTarget::Trait(b.nominal(&arguments)))
                                .collect(),
                        );
                    }
                    for (target, bounds) in method.bounds {
                        method_bounds
                            .entry(target.instantiate(&arguments))
                            .or_default()
                            .extend(
                                bounds
                                    .iter()
                                    .map(|b| ConstraintTarget::Trait(b.nominal(&arguments))),
                            );
                    }
                    MethodSignature {
                        has_default: method.native_default.is_some(),
                        declaration: method.item.declaration(),
                        id: method.item.identity(),
                        owner: id.clone(),
                        slot,
                        name: method.item.path.last().unwrap().1.into(),
                        generic_params: parameters,
                        bounds: method_bounds,
                        params: method
                            .params
                            .iter()
                            .map(|p| MethodParameter {
                                name: p.name.into(),
                                writeability: Writeability::Val,
                                ty: p.ty.instantiate(&arguments),
                            })
                            .collect(),
                        return_type: method.result.instantiate(&arguments),
                    }
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
    #[test]
    fn standard_declaration_locations_and_identities_are_unique() {
        let mut identities = std::collections::HashSet::new();
        for item in kagari_abi::standard::surface::STANDARD_ITEMS {
            assert!(identities.insert(item.identity()), "{:?}", item.path);
            let declaration = item.declaration();
            let source = sources()
                .iter()
                .find(|s| s.id() == declaration.location.file)
                .unwrap();
            assert_eq!(&source.text()[item.start..item.end], declaration.name);
            assert!(!item.documentation.is_empty());
        }
        assert_eq!(
            kagari_abi::standard::surface::STANDARD_TRAITS.len(),
            StandardTrait::ALL.len()
        );
    }
}
