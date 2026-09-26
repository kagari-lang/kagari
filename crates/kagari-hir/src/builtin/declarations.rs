//! Public signatures compiled from the bundled declaration sources.
use super::surface::{self, StandardEnum};
use crate::types::{BuiltinType, TypeId};
use std::collections::BTreeMap;

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
