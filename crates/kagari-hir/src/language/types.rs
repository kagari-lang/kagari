//! Frontend interpretation of language constructors and nominal Native interfaces.

use crate::{
    DiagnosticBuffer,
    analysis::error::AnalysisError,
    declarations::Declarations,
    hir::item::Module,
    resolver::resolved::ResolvedName,
    types::{NominalType, TypeId},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{
    declaration::{module::DeclarationError, native::NativeTypeConstructor},
    range::RangeKind,
    scalar::BuiltinType,
    visibility::Visibility,
};

/// Inspect the collected identity (including kind and occurrence), not a candidate
/// reconstructed from a spelling or selected by a producer.
pub(crate) fn validate_constructors(
    module: &Module,
    declarations: &Declarations,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) -> Result<(), AnalysisError> {
    for item in &module.native_types {
        cancel.check()?;
        if item.is_host
            && let Some(first) = item.generic_params.first()
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::GenericHostType {
                    name: item.name.clone(),
                })
                .with_span(module.generic_parameter(*first).origin.diagnostic_span()),
            );
        }
        let owner = declarations
            .definition(ResolvedName::NativeType(item.id))
            .expect("collected native declaration");
        let definitions = declarations.definitions();
        let constructor = if item.is_host {
            // A Host has a Struct identity, but cannot claim a reserved Native constructor's name.
            let view = definitions
                .resolve(*owner)
                .map_err(DefinitionMappingError::from)?;
            NativeTypeConstructor::reserved_name(view.module(), &item.name)
        } else {
            NativeTypeConstructor::from_definition(*owner, definitions)
        };
        let Some(constructor) = constructor else {
            continue;
        };
        if item.is_host
            || item.visibility != Visibility::Public
            || item.generic_params.len() != constructor.arity()
            || !item.fields.is_empty()
            || !item.bounds.is_empty()
            || !item.trait_bounds.is_empty()
            || item
                .generic_params
                .iter()
                .any(|id| !module.generic_parameter(*id).bounds.is_empty())
        {
            return Err(DeclarationError(format!(
                "invalid language constructor declaration shape for {}",
                item.name
            ))
            .into());
        }
    }
    Ok(())
}

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
