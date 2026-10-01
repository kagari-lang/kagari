//! Symbolic authoring types become ABI types under an explicit declaration binder.
use crate::{error::RuntimeError, native::catalog::NativeCatalog};

use kagari_abi::{
    native_api::NativeModule,
    scalar::BuiltinType,
    standard::surface::{StandardEnum, StandardTypeConstraint, builtin_type},
    types::{
        AbiType, GenericParameterAbi, NominalAbiType, TypeAbiKind, native::NativeTypeConstructor,
    },
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionId, DefinitionKind, ModuleIdentity, PackageId, associated_type_id},
    range::RangeKind,
};
use std::collections::BTreeMap;

/// Macro expansion input, resolved without parsing generated Kagari text.
#[doc(hidden)]
pub enum TypeExpression {
    Never,
    Constrained {
        ty: Box<Self>,
        constraint: StandardTypeConstraint,
    },
    Parameter(usize),
    Associated(usize),
    Projection {
        receiver: Box<Self>,
        interface: Box<Self>,
        member: &'static str,
    },
    Named {
        path: Vec<&'static str>,
        arguments: Vec<Self>,
        bindings: Vec<(&'static str, Self)>,
    },
    Array(Box<Self>),
    MutableArray(Box<Self>),
    Iter(Box<Self>),
    Range(Box<Self>, RangeKind),
    Tuple(Vec<Self>),
    Function {
        params: Vec<Self>,
        result: Box<Self>,
    },
}

pub(super) struct Scope<'a> {
    pub module: &'a NativeModule,
    pub catalog: &'a NativeCatalog,
    pub owner: DefinitionId,
    pub names: &'a [&'static str],
    pub receiver: Option<&'a AbiType>,
    pub associated: &'a [&'static str],
}

impl Scope<'_> {
    pub fn generics(&self) -> Vec<GenericParameterAbi> {
        self.names
            .iter()
            .enumerate()
            .map(|(position, _)| GenericParameterAbi {
                owner: self.owner.clone(),
                position,
            })
            .collect()
    }

    pub fn resolve(&self, expression: &TypeExpression) -> Result<AbiType, RuntimeError> {
        Ok(match expression {
            TypeExpression::Never => AbiType::Builtin(BuiltinType::Never),
            TypeExpression::Constrained { ty, constraint } => {
                let ty = self.resolve(ty)?;
                let valid = match &ty {
                    AbiType::Builtin(kind) => constraint.accepts_builtin_number(*kind),
                    AbiType::Parameter { .. }
                    | AbiType::SelfType(_)
                    | AbiType::Projection { .. } => true,
                    _ => false,
                };
                if !valid {
                    return Err(RuntimeError::metadata_conflict(
                        "native numeric adapter requires a compatible builtin number",
                    ));
                }
                ty
            }
            TypeExpression::Projection {
                receiver,
                interface,
                member,
            } => {
                let AbiType::Trait(interface) = self.resolve(interface)? else {
                    return Err(RuntimeError::metadata_conflict(
                        "native projection requires a trait",
                    ));
                };
                let declaration = self
                    .module
                    .traits
                    .iter()
                    .find(|contract| {
                        self.module
                            .definition(DefinitionKind::Trait, &contract.name)
                            == interface.declaration
                    })
                    .or_else(|| self.catalog.get(&interface.declaration))
                    .ok_or_else(|| {
                        RuntimeError::metadata_conflict(
                            "native projection requires a registered trait",
                        )
                    })?;
                let member = associated_type_id(&interface.declaration, member);
                if interface.arguments.len() != declaration.generic_params.len()
                    || !declaration.associated_types.iter().any(|declared| {
                        declared.declaration == member && declared.generic_params.is_empty()
                    })
                    || interface.associated_types.keys().any(|bound| {
                        !declaration.associated_types.iter().any(|declared| {
                            &declared.declaration == bound && declared.generic_params.is_empty()
                        })
                    })
                {
                    return Err(RuntimeError::metadata_conflict(
                        "native projection names an absent or unsupported associated type",
                    ));
                }
                AbiType::Projection {
                    receiver: Box::new(self.resolve(receiver)?),
                    interface: Box::new(interface),
                    member,
                    arguments: vec![],
                }
            }
            TypeExpression::Associated(position) => {
                let name = self.associated.get(*position).ok_or_else(|| {
                    RuntimeError::metadata_conflict("native associated slot is not declared")
                })?;
                AbiType::Projection {
                    receiver: Box::new(AbiType::SelfType(self.owner.clone())),
                    interface: Box::new(NominalAbiType {
                        declaration: self.owner.clone(),
                        arguments: self
                            .generics()
                            .iter()
                            .map(GenericParameterAbi::as_type)
                            .collect(),
                        associated_types: BTreeMap::new(),
                    }),
                    member: associated_type_id(&self.owner, name),
                    arguments: vec![],
                }
            }
            TypeExpression::Parameter(position) => {
                if *position >= self.names.len() {
                    return Err(RuntimeError::metadata_conflict(
                        "native generic slot is not declared",
                    ));
                }
                AbiType::Parameter {
                    owner: self.owner.clone(),
                    position: *position,
                }
            }
            TypeExpression::MutableArray(item) => {
                AbiType::Array(Box::new(self.resolve(item)?), CollectionAccess::Mutable)
            }
            TypeExpression::Array(item) => {
                AbiType::Array(Box::new(self.resolve(item)?), CollectionAccess::ReadOnly)
            }
            TypeExpression::Iter(item) => AbiType::Iter(Box::new(self.resolve(item)?)),
            TypeExpression::Range(item, kind) => {
                AbiType::Range(Box::new(self.resolve(item)?), *kind)
            }
            TypeExpression::Tuple(items) if items.is_empty() => AbiType::Builtin(BuiltinType::Unit),
            TypeExpression::Tuple(items) => AbiType::Tuple(self.resolve_arguments(items)?),
            TypeExpression::Function { params, result } => AbiType::Function {
                params: self.resolve_arguments(params)?,
                result: Box::new(self.resolve(result)?),
            },
            TypeExpression::Named {
                path,
                arguments,
                bindings,
            } => self.named(path, arguments, bindings)?,
        })
    }

    fn resolve_arguments(&self, items: &[TypeExpression]) -> Result<Vec<AbiType>, RuntimeError> {
        items.iter().map(|item| self.resolve(item)).collect()
    }

    fn named(
        &self,
        path: &[&str],
        arguments: &[TypeExpression],
        bindings: &[(&str, TypeExpression)],
    ) -> Result<AbiType, RuntimeError> {
        let invalid = || RuntimeError::metadata_conflict("invalid native authoring type");
        let name = *path.last().ok_or_else(invalid)?;
        if path.len() == 1 {
            if let Some(position) = self.names.iter().position(|parameter| *parameter == name) {
                if !arguments.is_empty() || !bindings.is_empty() {
                    return Err(invalid());
                }
                return Ok(AbiType::Parameter {
                    owner: self.owner.clone(),
                    position,
                });
            }
            if name == "Self" {
                if !arguments.is_empty() || !bindings.is_empty() {
                    return Err(invalid());
                }
                return Ok(self
                    .receiver
                    .cloned()
                    .unwrap_or_else(|| AbiType::SelfType(self.owner.clone())));
            }
            if let Some(builtin) = builtin_type(name) {
                if !arguments.is_empty() || !bindings.is_empty() {
                    return Err(invalid());
                }
                return Ok(AbiType::Builtin(builtin));
            }
            let kind = match name {
                "Bound" => Some(StandardEnum::Bound),
                "ParseError" => Some(StandardEnum::ParseError),
                "TryFromIntError" => Some(StandardEnum::TryFromIntError),
                "Infallible" => Some(StandardEnum::Infallible),
                "Option" => Some(StandardEnum::Option),
                "Result" => Some(StandardEnum::Result),
                "Ordering" => Some(StandardEnum::Ordering),
                _ => None,
            };
            if let Some(kind) = kind {
                if arguments.len() != kind.arity() || !bindings.is_empty() {
                    return Err(invalid());
                }
                return Ok(AbiType::StandardEnum {
                    kind,
                    args: self.resolve_arguments(arguments)?,
                });
            }
            if let Some(ty) = self.module.types.iter().find(|ty| ty.name == name) {
                let TypeAbiKind::Native(constructor) = ty.kind else {
                    return Err(invalid());
                };
                if arguments.len() != constructor.arity() || !bindings.is_empty() {
                    return Err(invalid());
                }
                let arguments = self.resolve_arguments(arguments)?;
                return Ok(match constructor {
                    NativeTypeConstructor::Array => {
                        AbiType::Array(Box::new(arguments[0].clone()), CollectionAccess::Mutable)
                    }
                    NativeTypeConstructor::String => AbiType::Builtin(BuiltinType::String),
                    NativeTypeConstructor::Range(RangeKind::Full) => AbiType::Range(
                        Box::new(AbiType::Builtin(BuiltinType::Unit)),
                        RangeKind::Full,
                    ),
                    NativeTypeConstructor::Range(kind) => {
                        AbiType::Range(Box::new(arguments[0].clone()), kind)
                    }
                    NativeTypeConstructor::Enum(kind) => AbiType::StandardEnum {
                        kind,
                        args: arguments,
                    },
                    NativeTypeConstructor::Map => AbiType::Map {
                        key: Box::new(arguments[0].clone()),
                        value: Box::new(arguments[1].clone()),
                        access: CollectionAccess::Mutable,
                    },
                    NativeTypeConstructor::Set => {
                        AbiType::Set(Box::new(arguments[0].clone()), CollectionAccess::Mutable)
                    }
                    NativeTypeConstructor::Iter => AbiType::Iter(Box::new(arguments[0].clone())),
                });
            }
        }
        let declaration = if path.len() == 1 {
            self.module.definition(DefinitionKind::Trait, name)
        } else {
            let module = NativeModule::new(ModuleIdentity {
                package: PackageId(
                    if path[0] == "std" {
                        "kagari-std"
                    } else {
                        path[0]
                    }
                    .into(),
                ),
                path: path[1..path.len() - 1]
                    .iter()
                    .map(|name| (*name).into())
                    .collect(),
            });
            module.definition(DefinitionKind::Trait, name)
        };
        let mut associated_types = BTreeMap::new();
        for (name, value) in bindings {
            if associated_types
                .insert(associated_type_id(&declaration, name), self.resolve(value)?)
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(AbiType::Trait(NominalAbiType {
            declaration,
            arguments: self.resolve_arguments(arguments)?,
            associated_types,
        }))
    }
}
