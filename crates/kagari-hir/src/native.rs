//! HIR representation hooks supplied only by an installed native declaration.
use crate::{host::HostFunctionId, types::TypeId};
use kagari_abi::{
    callable::NativeDefaultApplication, scalar::BuiltinType, standard::surface::StandardEnum,
};
use kagari_common::{collection::CollectionAccess, identity::DefinitionId, range::RangeKind};

pub(crate) mod api;

/// Installed entry identity, checked host catalog index or symbolic default mapping.
/// Selection resolves defaults to ordinary entries; bindings are not function pointers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeBinding {
    Entry(DefinitionId),
    Host(HostFunctionId),
    /// Symbolic registered template application; selection resolves an Entry.
    Default(NativeDefaultApplication),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTypeKind {
    String,
    ArrayList,
    HashMap,
    HashSet,
    Iter,
    Range(RangeKind),
    Enum(StandardEnum),
}

/// Canonical diagnostic labels for native representation tags. This does not
/// resolve source names; declarations and aliases are resolved through HIR.
pub(crate) fn enum_display_name(kind: StandardEnum) -> &'static str {
    match kind {
        StandardEnum::Bound => "Bound",
        StandardEnum::ParseError => "ParseError",
        StandardEnum::TryFromIntError => "TryFromIntError",
        StandardEnum::Infallible => "Infallible",
        StandardEnum::Option => "Option",
        StandardEnum::Result => "Result",
        StandardEnum::Ordering => "Ordering",
    }
}

impl NativeTypeKind {
    pub fn arity(self) -> usize {
        match self {
            Self::Enum(kind) => kind.arity(),
            Self::String | Self::Range(RangeKind::Full) => 0,
            Self::HashMap => 2,
            _ => 1,
        }
    }

    pub fn apply(self, arguments: &[TypeId]) -> Option<TypeId> {
        if arguments.len() != self.arity() {
            return None;
        }
        let first = || Box::new(arguments[0].clone());
        Some(match self {
            Self::Enum(kind) => TypeId::StandardEnum {
                kind,
                args: arguments.to_vec(),
            },
            Self::String => TypeId::Builtin(BuiltinType::String),
            Self::ArrayList => TypeId::Array(first(), CollectionAccess::Mutable),
            Self::HashMap => TypeId::Map {
                key: first(),
                value: Box::new(arguments[1].clone()),
                access: CollectionAccess::Mutable,
            },
            Self::HashSet => TypeId::Set(first(), CollectionAccess::Mutable),
            Self::Iter => TypeId::Iter(first()),
            Self::Range(RangeKind::Full) => TypeId::Range(
                Box::new(TypeId::Builtin(BuiltinType::Unit)),
                RangeKind::Full,
            ),
            Self::Range(kind) => TypeId::Range(first(), kind),
        })
    }
}
