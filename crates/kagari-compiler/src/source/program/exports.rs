//! Translate checked source references once; executable layers retain only identities.

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::{DefinitionId, DefinitionTable, module::ModuleHandle},
    },
};
use kagari_contract::exports::{ExportNamespace, ProgramExports, TypeExport};
use kagari_hir::{
    declarations::DeclarationId,
    imports::SourceDeclRef,
    program::{CheckedProgram, exports::TypeExport as SourceTypeExport},
    resolver::resolved::ResolvedName,
    types::semantic::lower_type,
};
use kagari_types::ty::{Ty, substitution::TypeTransformError};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
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
    let definitions = program.root().names.catalog.definitions();
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
                        types: namespace
                            .types
                            .iter()
                            .map(|(name, export)| {
                                check_cancel(cancel)?;
                                let export = match export {
                                    SourceTypeExport::Declaration(id) => {
                                        TypeExport::Declaration(*id)
                                    }
                                    SourceTypeExport::Closed(ty) => {
                                        TypeExport::Closed(lower_type(ty))
                                    }
                                    SourceTypeExport::OpenAlias { arity } => {
                                        TypeExport::OpenAlias { arity: *arity }
                                    }
                                };
                                Ok((name.clone(), export))
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

/// Export-only applications use the supplying module's ordinary bounded layout queue.
/// Structs need binding layouts; enums also supply payload evidence for structural
/// application-bound proofs. This includes nested applications without granting
/// object authority to non-object aliases. Open aliases never contribute a template.
pub(super) fn layout_requests(
    exports: &ProgramExports,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<HashMap<ModuleHandle, Vec<Ty>>, DefinitionMappingError> {
    let mut roots = HashSet::new();
    for export in exports
        .namespaces
        .values()
        .flat_map(|namespace| namespace.types.values())
    {
        check_cancel(cancel)?;
        let TypeExport::Closed(ty) = export else {
            continue;
        };
        if !ty.within_wire_limits() {
            return Err(DefinitionMappingError::InvalidContract);
        }
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            check_cancel(cancel)?;
            match ty {
                Ty::Struct(nominal)
                | Ty::Enum(nominal)
                | Ty::Trait(nominal)
                | Ty::NativeObject(nominal) => {
                    if matches!(ty, Ty::Struct(_) | Ty::Enum(_)) {
                        roots.insert(ty.clone());
                    }
                    pending.extend(&nominal.arguments);
                    pending.extend(nominal.associated_types.values());
                }
                Ty::Tuple(items) => pending.extend(items),
                Ty::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Ty::Reference { target, .. }
                | Ty::Array(target)
                | Ty::Set(target, _)
                | Ty::Iter(target)
                | Ty::Range(target, _) => pending.push(target),
                Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Ty::Builtin(_) | Ty::Host(_) => {}
                Ty::Parameter { .. } | Ty::SelfType(_) | Ty::Projection { .. } => {
                    return Err(DefinitionMappingError::InvalidContract);
                }
            }
        }
    }
    let mut roots = roots.into_iter().collect::<Vec<_>>();
    let mut failure = None;
    roots.sort_by(|left, right| {
        left.canonical_cmp(right, definitions, cancel)
            .unwrap_or_else(|error| {
                failure = Some(error);
                Ordering::Equal
            })
    });
    if let Some(error) = failure {
        return Err(match error {
            TypeTransformError::Cancelled => DefinitionMappingError::Cancelled,
            _ => DefinitionMappingError::InvalidContract,
        });
    }

    let mut requests: HashMap<_, Vec<_>> = HashMap::new();
    for ty in roots {
        check_cancel(cancel)?;
        let (Ty::Struct(nominal) | Ty::Enum(nominal)) = &ty else {
            unreachable!()
        };
        requests
            .entry(definitions.module_of(nominal.declaration)?)
            .or_default()
            .push(ty);
    }
    Ok(requests)
}
