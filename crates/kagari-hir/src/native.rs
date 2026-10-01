//! HIR representation hooks supplied only by an installed native declaration.
use crate::{host::HostFunctionId, types::TypeId};
use kagari_abi::{scalar::BuiltinType, standard::surface::StandardEnum};
use kagari_common::{collection::CollectionAccess, identity::DefinitionId, range::RangeKind};

pub(crate) mod api;
pub(crate) mod stdlib;

/// Opaque entry identity or an index into the checked host declaration catalog.
/// Both lower to a declaration ID; neither is a runtime function pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeBinding {
    Entry(DefinitionId),
    Host(HostFunctionId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTypeKind {
    String,
    ArrayList,
    LinkedHashMap,
    LinkedHashSet,
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
    pub(crate) fn from_binding(binding: &str) -> Option<Self> {
        Some(match binding {
            "String" => Self::String,
            "ArrayList" => Self::ArrayList,
            "LinkedHashMap" => Self::LinkedHashMap,
            "LinkedHashSet" => Self::LinkedHashSet,
            "Iter" => Self::Iter,
            "Range" => Self::Range(RangeKind::Exclusive),
            "RangeInclusive" => Self::Range(RangeKind::Inclusive),
            "RangeFrom" => Self::Range(RangeKind::From),
            "RangeTo" => Self::Range(RangeKind::To),
            "RangeToInclusive" => Self::Range(RangeKind::ToInclusive),
            "RangeFull" => Self::Range(RangeKind::Full),
            _ => return None,
        })
    }

    pub fn arity(self) -> usize {
        match self {
            Self::Enum(kind) => kind.arity(),
            Self::String | Self::Range(RangeKind::Full) => 0,
            Self::LinkedHashMap => 2,
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
            Self::LinkedHashMap => TypeId::Map {
                key: first(),
                value: Box::new(arguments[1].clone()),
                access: CollectionAccess::Mutable,
            },
            Self::LinkedHashSet => TypeId::Set(first(), CollectionAccess::Mutable),
            Self::Iter => TypeId::Iter(first()),
            Self::Range(RangeKind::Full) => TypeId::Range(
                Box::new(TypeId::Builtin(BuiltinType::Unit)),
                RangeKind::Full,
            ),
            Self::Range(kind) => TypeId::Range(first(), kind),
        })
    }
}
