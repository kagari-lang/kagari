//! Frontend interpretation of language constructors and nominal Native interfaces.

use crate::{
    lower::LoweredModule,
    native::NativeTypeKind,
    types::{NominalType, TypeId, semantic::lower_type},
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_types::{
    declaration::{native::NativeTypeConstructor, ownership::ReceiverOwners},
    range::RangeKind,
    reference::ReferenceAccess,
    scalar::BuiltinType,
};

pub(crate) fn builtin_constructor(kind: &NativeTypeKind) -> Option<NativeTypeConstructor> {
    Some(match kind {
        NativeTypeKind::String => NativeTypeConstructor::String,
        NativeTypeKind::Range(kind) => NativeTypeConstructor::Range(*kind),
        NativeTypeKind::Reference(access) => NativeTypeConstructor::Reference(*access),
        NativeTypeKind::Host { .. } | NativeTypeKind::Storage { .. } => return None,
    })
}

/// Static receiver ownership does not imply installed dispatch or callable authority.
/// Constructor bindings have already passed mixed-environment uniqueness checks.
pub(crate) fn external_receiver_owned(
    lowered: &LoweredModule,
    ty: &TypeId,
    definitions: &DefinitionTable,
) -> bool {
    if !lowered.external_api {
        return false;
    }
    let module = lowered.source.module_identity();
    let owners = ReceiverOwners::from_constructors(
        lowered
            .native_types
            .values()
            .filter_map(|kind| builtin_constructor(kind).map(|constructor| (module, constructor))),
    )
    .expect("one module cannot have conflicting constructor owners");
    owners.owner(&lower_type(ty), definitions).as_ref() == Some(module)
}

/// Checked constructor identity, independent of registration storage/layout policy.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TypeConstructor {
    Native {
        declaration: DefinitionId,
        arity: usize,
    },
    Host(DefinitionId),
    String,
    Range(RangeKind),
    Reference(ReferenceAccess),
}

impl TypeConstructor {
    /// Interprets a validated installed correspondence, never an identifier spelling.
    pub(crate) fn registered(kind: &NativeTypeKind) -> Self {
        match kind {
            NativeTypeKind::Host { declaration, .. } => Self::Host(*declaration),
            NativeTypeKind::Storage {
                declaration, arity, ..
            } => Self::Native {
                declaration: *declaration,
                arity: *arity,
            },
            NativeTypeKind::String => Self::String,
            NativeTypeKind::Range(kind) => Self::Range(*kind),
            NativeTypeKind::Reference(access) => Self::Reference(*access),
        }
    }

    pub(crate) fn arity(self) -> usize {
        match self {
            Self::Native { arity, .. } => arity,
            Self::Host(_) | Self::String | Self::Range(RangeKind::Full) => 0,
            Self::Range(_) | Self::Reference(_) => 1,
        }
    }

    /// Applies language semantics without granting executable representation or access.
    pub(crate) fn apply(self, arguments: &[TypeId]) -> Option<TypeId> {
        if arguments.len() != self.arity() {
            return None;
        }
        let first = || Box::new(arguments[0].clone());
        Some(match self {
            Self::Native { declaration, .. } => TypeId::NativeObject(NominalType {
                declaration,
                arguments: arguments.to_vec(),
                associated_types: Default::default(),
            }),
            Self::Host(declaration) => TypeId::Host(declaration),
            Self::String => TypeId::Builtin(BuiltinType::String),
            Self::Range(RangeKind::Full) => TypeId::Range(
                Box::new(TypeId::Builtin(BuiltinType::Unit)),
                RangeKind::Full,
            ),
            Self::Range(kind) => TypeId::Range(first(), kind),
            Self::Reference(access) => TypeId::Reference {
                target: first(),
                access,
            },
        })
    }
}
