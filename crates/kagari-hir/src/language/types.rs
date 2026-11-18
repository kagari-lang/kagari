//! Frontend interpretation of language constructors and nominal Native interfaces.

use crate::types::{NominalType, TypeId};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::native::NativeTypeConstructor, range::RangeKind, scalar::BuiltinType,
};

/// Checked constructor identity, independent of registration storage/layout policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeConstructor {
    Native {
        declaration: DefinitionId,
        arity: usize,
    },
    Host(DefinitionId),
    String,
    Range(RangeKind),
}

impl TypeConstructor {
    pub(crate) fn declaration(
        declaration: DefinitionId,
        arity: usize,
        host: bool,
        definitions: &DefinitionTable,
    ) -> Self {
        if host {
            return Self::Host(declaration);
        }
        match NativeTypeConstructor::from_definition(declaration, definitions) {
            Some(NativeTypeConstructor::String) => Self::String,
            Some(NativeTypeConstructor::Range(kind)) => Self::Range(kind),

            None => Self::Native { declaration, arity },
        }
    }

    pub(crate) fn arity(self) -> usize {
        match self {
            Self::Native { arity, .. } => arity,
            Self::Host(_) | Self::String | Self::Range(RangeKind::Full) => 0,
            Self::Range(_) => 1,
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
        })
    }
}

impl RemapDefinitions for TypeConstructor {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Native { declaration, arity } => Self::Native {
                declaration: mapper.reference(declaration)?,
                arity: *arity,
            },
            Self::Host(id) => Self::Host(mapper.reference(id)?),
            _ => *self,
        })
    }
}

impl DefinitionReferences for TypeConstructor {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        match self {
            Self::Native { declaration, .. } | Self::Host(declaration) => visit(declaration)?,
            _ => {}
        }
        Ok(())
    }
}
