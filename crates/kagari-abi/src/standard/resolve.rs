//! Interpret the engine's generated declarations as portable ABI templates.
//!
//! This is descriptor expansion, not source inference. Its input tree is generated
//! from bundled declarations; artifact-provided types are verified separately.
use crate::scalar::BuiltinType;
use crate::standard::declarations::{ApiBound, ApiType};
use crate::standard::surface;
use crate::standard::traits::{self, StandardTrait};
use crate::types::{AbiType, NominalAbiType};
use kagari_common::collection::CollectionAccess;
use kagari_common::identity::associated_type_id;
use kagari_common::range::RangeKind;
use std::collections::BTreeMap;

pub(crate) type Arguments = BTreeMap<&'static str, AbiType>;

impl ApiBound {
    pub(crate) fn resolve(&self, arguments: &Arguments) -> Option<NominalAbiType> {
        let kind = StandardTrait::from_name(self.name)?;
        let declaration = traits::identity(kind);
        Some(NominalAbiType {
            associated_types: self
                .bindings
                .iter()
                .map(|(name, ty)| {
                    Some((
                        associated_type_id(&declaration, name),
                        ty.resolve(arguments)?,
                    ))
                })
                .collect::<Option<_>>()?,
            declaration,
            arguments: resolve_all(self.args, arguments)?,
        })
    }
}

impl ApiType {
    pub(crate) fn resolve(&self, arguments: &Arguments) -> Option<AbiType> {
        Some(match self {
            Self::Projection(receiver, bound, member) => {
                let interface = bound.resolve(arguments)?;
                AbiType::Projection {
                    receiver: Box::new(receiver.resolve(arguments)?),
                    member: associated_type_id(&interface.declaration, member),
                    interface: Box::new(interface),
                    arguments: vec![],
                }
            }
            Self::Array(element) => AbiType::Trait(NominalAbiType {
                declaration: traits::identity(StandardTrait::List),
                arguments: vec![element.resolve(arguments)?],
                associated_types: BTreeMap::new(),
            }),
            Self::Tuple([]) => AbiType::Builtin(BuiltinType::Unit),
            Self::Tuple(items) => AbiType::Tuple(resolve_all(items, arguments)?),
            Self::Function(params, result) => AbiType::Function {
                params: resolve_all(params, arguments)?,
                result: Box::new(result.resolve(arguments)?),
            },
            Self::Named(name, params) => resolve_named(name, params, arguments)?,
        })
    }
}

fn resolve_all(types: &[ApiType], arguments: &Arguments) -> Option<Vec<AbiType>> {
    types.iter().map(|ty| ty.resolve(arguments)).collect()
}

fn resolve_named(name: &str, params: &[ApiType], arguments: &Arguments) -> Option<AbiType> {
    if let Some(ty) = arguments.get(name) {
        return Some(ty.clone());
    }
    if let Some(member) = name.strip_prefix("Self::")
        && let Some(AbiType::Trait(interface)) = arguments.get("@self_trait")
    {
        return Some(AbiType::Projection {
            receiver: Box::new(AbiType::SelfType(interface.declaration.clone())),
            member: associated_type_id(&interface.declaration, member),
            interface: Box::new(interface.clone()),
            arguments: resolve_all(params, arguments)?,
        });
    }
    if let Some((owner, "Item")) = name.split_once("::") {
        let AbiType::Trait(interface) = arguments.get(format!("@bound:{owner}").as_str())? else {
            return None;
        };
        return Some(AbiType::Projection {
            receiver: Box::new(arguments.get(owner)?.clone()),
            member: associated_type_id(&interface.declaration, "Item"),
            interface: Box::new(interface.clone()),
            arguments: vec![],
        });
    }
    if let Some(builtin) = surface::builtin_type(name) {
        return params.is_empty().then_some(AbiType::Builtin(builtin));
    }
    let types = resolve_all(params, arguments)?;
    if let Some(kind) = surface::range_kind(name) {
        return match (kind, types.as_slice()) {
            (RangeKind::Full, []) => Some(AbiType::Range(
                Box::new(AbiType::Builtin(BuiltinType::Unit)),
                kind,
            )),
            (RangeKind::Full, _) => None,
            (_, [element]) => Some(AbiType::Range(Box::new(element.clone()), kind)),
            _ => None,
        };
    }
    if let Some(kind) = StandardTrait::from_name(name) {
        return (types.len() == kind.declaration().generics.len()).then(|| {
            AbiType::Trait(NominalAbiType {
                declaration: traits::identity(kind),
                arguments: types,
                associated_types: BTreeMap::new(),
            })
        });
    }
    if let Some(spec) = surface::standard_enum(name) {
        return (types.len() == spec.arity).then_some(AbiType::StandardEnum {
            kind: spec.kind,
            args: types,
        });
    }
    Some(match (name, types.as_slice()) {
        ("ArrayList", [element]) => {
            AbiType::Array(Box::new(element.clone()), CollectionAccess::Mutable)
        }
        ("LinkedHashMap", [key, value]) => AbiType::Map {
            key: Box::new(key.clone()),
            value: Box::new(value.clone()),
            access: CollectionAccess::Mutable,
        },
        ("LinkedHashSet", [element]) => {
            AbiType::Set(Box::new(element.clone()), CollectionAccess::Mutable)
        }
        ("Iter", [element]) => AbiType::Iter(Box::new(element.clone())),
        _ => return None,
    })
}
