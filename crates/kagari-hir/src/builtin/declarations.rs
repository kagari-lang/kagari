//! Public signatures compiled from the bundled declaration sources.
use super::surface::{self, StandardEnum};
use crate::types::{BuiltinType, TypeId};
use std::collections::BTreeMap;

pub fn sources() -> &'static [kagari_common::SourceFile] {
    static SOURCES: std::sync::OnceLock<Vec<kagari_common::SourceFile>> =
        std::sync::OnceLock::new();
    SOURCES.get_or_init(|| {
        surface::STANDARD_SOURCES
            .iter()
            .map(|(uri, text)| kagari_common::SourceFile::new(*uri, *text))
            .collect()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiItem {
    pub module: &'static str,
    pub uri: &'static str,
    pub path: &'static [(kagari_common::identity::DefinitionKind, &'static str)],
    pub start: usize,
    pub end: usize,
    pub documentation: &'static str,
    pub signature: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiBound {
    pub name: &'static str,
    pub args: &'static [ApiType],
    pub bindings: &'static [(&'static str, ApiType)],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiAssociatedType {
    pub item: ApiItem,
    pub bounds: &'static [ApiBound],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiMethod {
    pub item: ApiItem,
    pub params: &'static [ApiParameter],
    pub result: ApiType,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiTrait {
    pub item: ApiItem,
    pub generics: &'static [&'static str],
    pub supertraits: &'static [ApiBound],
    pub associated_types: &'static [ApiAssociatedType],
    pub methods: &'static [ApiMethod],
}

impl ApiItem {
    pub fn identity(&self) -> kagari_common::identity::DefinitionId {
        use kagari_common::identity::*;
        DefinitionId {
            module: ModuleIdentity {
                package: PackageId("kagari-std".into()),
                path: vec![self.module.into()],
            },
            path: self
                .path
                .iter()
                .map(|(kind, name)| DefinitionPathSegment {
                    kind: *kind,
                    name: (*name).into(),
                    occurrence: 0,
                })
                .collect(),
        }
    }
    pub fn declaration(&self) -> crate::declarations::Declaration {
        use crate::declarations::*;
        let source = sources()
            .iter()
            .find(|source| source.name() == self.uri)
            .expect("bundled declaration source");
        Declaration {
            id: DeclarationId::Definition(self.identity()),
            name: self.path.last().unwrap().1.into(),
            location: source
                .span(kagari_common::Span::new(self.start, self.end))
                .expect("declaration source span"),
        }
    }
}

pub type Arguments = BTreeMap<&'static str, TypeId>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiType {
    Named(&'static str, &'static [ApiType]),
    Array(&'static ApiType),
    Tuple(&'static [ApiType]),
    Function(&'static [ApiType], &'static ApiType),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiParameter {
    pub name: &'static str,
    pub ty: ApiType,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiFunction {
    pub qualified_name: &'static str,
    pub uri: &'static str,
    pub start: usize,
    pub end: usize,
    pub documentation: &'static str,
    pub signature: &'static str,
    pub params: &'static [ApiParameter],
    pub result: ApiType,
}

impl ApiType {
    pub fn instantiate(&self, arguments: &Arguments) -> TypeId {
        match self {
            Self::Array(element) => TypeId::Array(Box::new(element.instantiate(arguments))),
            Self::Tuple(items) if items.is_empty() => TypeId::Builtin(BuiltinType::Unit),
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
                        member: crate::types::associated_type_id(&interface.declaration, member),
                        arguments: params.iter().map(|p| p.instantiate(arguments)).collect(),
                    };
                }
                if let Some((owner, "Item")) = name.split_once("::") {
                    return arguments
                        .get(owner)
                        .and_then(surface::iterable_protocol)
                        .map(|p| match p {
                            surface::IterableProtocol::Array { item }
                            | surface::IterableProtocol::Set { item } => item,
                            surface::IterableProtocol::Map { key, value } => {
                                TypeId::Tuple(vec![key, value])
                            }
                            surface::IterableProtocol::String { item } => TypeId::Builtin(item),
                        })
                        .unwrap_or(TypeId::Unknown);
                }
                if let Some(builtin) = surface::builtin_type(name) {
                    return TypeId::Builtin(builtin);
                }
                let types = params
                    .iter()
                    .map(|t| t.instantiate(arguments))
                    .collect::<Vec<_>>();
                match (*name, types.as_slice()) {
                    ("Map", [key, value]) => TypeId::Map {
                        key: Box::new(key.clone()),
                        value: Box::new(value.clone()),
                    },
                    ("Set", [item]) => TypeId::Set(Box::new(item.clone())),
                    ("Cursor", [item]) => TypeId::Cursor(Box::new(item.clone())),
                    ("Option", [_]) => TypeId::StandardEnum {
                        kind: StandardEnum::Option,
                        args: types,
                    },
                    ("Result", [_, _]) => TypeId::StandardEnum {
                        kind: StandardEnum::Result,
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
    pub fn infer(&self, actual: &TypeId, arguments: &mut Arguments) {
        match (self, actual) {
            (Self::Named(name, []), actual) if arguments.contains_key(name) => {
                arguments.get_mut(name).unwrap().recover_from(actual);
            }
            (Self::Array(element), TypeId::Array(actual)) => element.infer(actual, arguments),
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
                Self::Named("Map", [key, value]),
                TypeId::Map {
                    key: actual,
                    value: output,
                },
            ) => {
                key.infer(actual, arguments);
                value.infer(output, arguments);
            }
            (Self::Named("Set", [item]), TypeId::Set(actual))
            | (Self::Named("Cursor", [item]), TypeId::Cursor(actual)) => {
                item.infer(actual, arguments)
            }
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

impl ApiBound {
    fn nominal(&self, arguments: &Arguments) -> crate::types::NominalType {
        let kind =
            super::traits::StandardTrait::from_name(self.name).expect("standard trait bound");
        let id = super::traits::identity(kind);
        crate::types::NominalType {
            declaration: id.clone(),
            arguments: self.args.iter().map(|p| p.instantiate(arguments)).collect(),
            associated_types: self
                .bindings
                .iter()
                .map(|(name, ty)| {
                    (
                        crate::types::associated_type_id(&id, name),
                        ty.instantiate(arguments),
                    )
                })
                .collect(),
        }
    }
}

impl ApiTrait {
    pub fn contract(&self) -> crate::aggregates::TraitSignature {
        use crate::{
            aggregates::{MethodParameter, MethodSignature, TraitSignature},
            hir::Writeability,
            typeck::ConstraintTarget,
            types::{GenericParameterType, NominalType},
        };
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
                .map(|(slot, method)| MethodSignature {
                    has_default: false,
                    declaration: method.item.declaration(),
                    id: method.item.identity(),
                    owner: id.clone(),
                    slot,
                    name: method.item.path.last().unwrap().1.into(),
                    generic_params: generics.clone(),
                    bounds: Default::default(),
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
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            let spec = surface::standard_enum(name).unwrap();
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
        for item in surface::STANDARD_ITEMS {
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
            surface::STANDARD_TRAITS.len(),
            super::super::traits::StandardTrait::ALL.len()
        );
    }
}
