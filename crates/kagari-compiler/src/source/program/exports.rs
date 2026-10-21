//! Translate checked source references once; executable layers retain only identities.

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::DefinitionId,
    },
};
use kagari_contract::exports::{ExportNamespace, ProgramExports};
use kagari_hir::{
    declarations::DeclarationId, imports::SourceDeclRef, program::CheckedProgram,
    resolver::resolved::ResolvedName,
};

pub(super) fn lower(
    program: &CheckedProgram,
    cancel: &CancellationToken,
) -> Result<ProgramExports, DefinitionMappingError> {
    check_cancel(cancel)?;
    let declaration = |source: &SourceDeclRef| -> Result<DefinitionId, DefinitionMappingError> {
        check_cancel(cancel)?;
        let (module, signature) = program
            .source_function(source)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        let target = module
            .declarations
            .target(ResolvedName::Function(signature.id))
            .ok_or(DefinitionMappingError::InvalidContract)?;
        match target.id {
            DeclarationId::Definition(id) => Ok(id),
            _ => Err(DefinitionMappingError::InvalidContract),
        }
    };
    let definitions = &program.root().names.catalog.definitions;
    Ok(ProgramExports {
        namespaces: program
            .exports()
            .namespaces()
            .iter()
            .map(|(identity, namespace)| {
                check_cancel(cancel)?;
                Ok((
                    definitions.module_identity(*identity)?.clone(),
                    ExportNamespace {
                        modules: namespace
                            .modules
                            .iter()
                            .map(|(name, module)| {
                                Ok((name.clone(), definitions.module_identity(*module)?.clone()))
                            })
                            .collect::<Result<_, DefinitionMappingError>>()?,
                        functions: namespace
                            .functions
                            .iter()
                            .map(|(name, source)| Ok((name.clone(), declaration(source)?)))
                            .collect::<Result<_, DefinitionMappingError>>()?,
                    },
                ))
            })
            .collect::<Result<_, DefinitionMappingError>>()?,
        entry: program.exports().entry().map(declaration).transpose()?,
    })
}
