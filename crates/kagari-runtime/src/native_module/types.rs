//! Symbolic authoring types become ABI types under an explicit declaration binder.
use crate::{error::RuntimeError, native::catalog::NativeCatalog};

use kagari_abi::{
    native_api::NativeModule,
    scalar::BuiltinType,
    standard::surface::{StandardEnum, builtin_type},
    types::{AbiType, GenericParameterAbi, NominalAbiType},
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionId, DefinitionKind, ModuleIdentity, PackageId, associated_type_id},
};
use std::collections::BTreeMap;

/// Macro expansion input, resolved without parsing generated Kagari text.
#[doc(hidden)]
pub enum TypeExpression {
    Parameter(usize),
    Associated(usize),
    Named {
        path: Vec<&'static str>,
        arguments: Vec<Self>,
        bindings: Vec<(&'static str, Self)>,
    },
    Array(Box<Self>),
    MutableArray(Box<Self>),
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
            if self.module.types.iter().any(|ty| ty.name == name) {
                if arguments.len() != 1 || !bindings.is_empty() {
                    return Err(invalid());
                }
                return Ok(AbiType::Array(
                    Box::new(self.resolve(&arguments[0])?),
                    CollectionAccess::Mutable,
                ));
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
