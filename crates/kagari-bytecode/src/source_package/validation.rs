//! Reconcile complete source inventories with verified body and native origins.
use crate::{program::BytecodeProgram, source_package::SourcePackageError};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::check_cancel,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    source::{
        AuthoredDeclaration, AuthoredKind, ExecutableOrigin, ExecutableSource, SourcePackage,
        checked_text_fingerprint,
    },
    updates::{BodyClassification, Dependency},
};
use std::collections::{HashMap, HashSet};

pub(super) fn executable_sources(
    program: &BytecodeProgram,
    cancel: &CancellationToken,
) -> Result<Vec<ExecutableSource>, SourcePackageError> {
    let evidence = &program.update_evidence;
    let mut owners = HashMap::new();
    let mut source_ids = HashSet::new();
    for source in &evidence.sources {
        check_cancel(cancel)?;
        source_ids.insert(source.declaration);
        for dependency in &source.dependencies {
            if let Dependency::Body(body) = dependency
                && owners
                    .insert(body, source.declaration)
                    .is_some_and(|old| old != source.declaration)
            {
                return Err(SourcePackageError::OriginMismatch);
            }
        }
    }
    let placements = program
        .modules
        .iter()
        .flat_map(|module| &module.function_origins)
        .map(|origin| (origin.placement, origin.authored))
        .collect::<HashMap<_, _>>();
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for body in &evidence.bodies {
        check_cancel(cancel)?;
        let target = &body.requirement.origin;
        if matches!(body.classification, BodyClassification::IntrinsicAdapter(_)) {
            if owners.contains_key(target)
                || !seen.insert((target.clone(), ExecutableOrigin::Intrinsic))
            {
                return Err(SourcePackageError::OriginMismatch);
            }
            result.push(ExecutableSource {
                target: target.clone(),
                authored: None,
                native_binding: None,
                origin: ExecutableOrigin::Intrinsic,
            });
            continue;
        }
        let authored = *owners
            .get(target)
            .ok_or(SourcePackageError::OriginMismatch)?;
        match &body.classification {
            BodyClassification::Named(id) => {
                // Default implementations can be placed under a different named
                // declaration without a named-local FunctionOrigin. Their existing
                // checked source-body edge, not placement-name equality, owns the tie.
                if placements
                    .get(id)
                    .is_some_and(|expected| *expected != authored)
                {
                    return Err(SourcePackageError::OriginMismatch);
                }
            }
            BodyClassification::Closure(parent)
            | BodyClassification::Async(parent)
            | BodyClassification::Resume(parent)
            | BodyClassification::Adapter(parent) => {
                if owners.get(parent).copied() != Some(authored) {
                    return Err(SourcePackageError::OriginMismatch);
                }
            }
            BodyClassification::StaticInitializer(_) => {}
            BodyClassification::IntrinsicAdapter(_) => unreachable!("handled above"),
        }
        let origin = match &body.classification {
            BodyClassification::Named(_) => ExecutableOrigin::Named,
            BodyClassification::Closure(_) => ExecutableOrigin::Closure,
            BodyClassification::Async(_) => ExecutableOrigin::Async,
            BodyClassification::Resume(_) => ExecutableOrigin::Resume,
            BodyClassification::Adapter(_) => ExecutableOrigin::Adapter,
            BodyClassification::IntrinsicAdapter(_) => unreachable!("handled above"),
            BodyClassification::StaticInitializer(id) => {
                if *id != authored {
                    return Err(SourcePackageError::OriginMismatch);
                }
                ExecutableOrigin::StaticInitializer
            }
        };
        if !seen.insert((target.clone(), origin)) {
            return Err(SourcePackageError::OriginMismatch);
        }
        result.push(ExecutableSource {
            target: target.clone(),
            authored: Some(authored),
            native_binding: None,
            origin,
        });
    }
    for module in &program.modules {
        for native in &module.native_imports {
            check_cancel(cancel)?;
            if seen.insert((native.instance.clone(), ExecutableOrigin::Native)) {
                result.push(ExecutableSource {
                    target: native.instance.clone(),
                    authored: source_ids
                        .contains(&native.instance.declaration)
                        .then_some(native.instance.declaration),
                    native_binding: Some(native.binding),
                    origin: ExecutableOrigin::Native,
                });
            }
        }
    }
    Ok(result)
}

pub(super) fn validate(
    package: &SourcePackage,
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), SourcePackageError> {
    let mut origins = HashSet::new();
    let mut document_modules = Vec::with_capacity(package.documents.len());
    for document in &package.documents {
        check_cancel(cancel)?;
        if document.uri.is_empty()
            || document.modules.is_empty()
            || document.fingerprint != checked_text_fingerprint(&document.text, cancel)?
        {
            return Err(SourcePackageError::InvalidDocument);
        }
        let mut modules = HashSet::new();
        for module in &document.modules {
            check_cancel(cancel)?;
            if !modules.insert(module) || !origins.insert((module, &document.uri)) {
                return Err(SourcePackageError::InvalidDocument);
            }
        }
        document_modules.push(modules);
    }
    let mut declarations = HashMap::new();
    let mut used_origins = HashSet::new();
    for declaration in &package.declarations {
        check_cancel(cancel)?;
        let document = package
            .documents
            .get(declaration.document as usize)
            .ok_or(SourcePackageError::InvalidDocument)?;
        let text = document
            .text
            .get(declaration.range.start..declaration.range.end)
            .filter(|text| !text.is_empty())
            .ok_or(SourcePackageError::InvalidDocument)?;
        if checked_text_fingerprint(text, cancel)? != declaration.fingerprint {
            return Err(SourcePackageError::InvalidDocument);
        }
        let view = definitions
            .resolve(declaration.declaration)
            .map_err(|_| SourcePackageError::DeclarationMismatch)?;
        if !document_modules[declaration.document as usize].contains(view.module()) {
            return Err(SourcePackageError::InvalidDocument);
        }
        if !program
            .modules
            .iter()
            .any(|module| &module.identity == view.module())
            || !kind_matches(declaration.kind, view.last().map(|part| part.kind))
            || declarations
                .insert(declaration.declaration, declaration)
                .is_some()
        {
            return Err(SourcePackageError::DeclarationMismatch);
        }
        used_origins.insert((view.module(), &document.uri));
    }
    if used_origins != origins {
        return Err(SourcePackageError::InvalidDocument);
    }
    for source in &program.update_evidence.sources {
        check_cancel(cancel)?;
        if !declarations.contains_key(&source.declaration) {
            return Err(SourcePackageError::DeclarationMismatch);
        }
    }
    let manifest = program
        .update_evidence
        .source_manifest
        .as_ref()
        .ok_or(SourcePackageError::MissingManifest)?;
    if manifest.documents.len() != package.documents.len()
        || manifest.declarations.len() != package.declarations.len()
    {
        return Err(SourcePackageError::DeclarationMismatch);
    }
    for (anchor, document) in manifest.documents.iter().zip(&package.documents) {
        check_cancel(cancel)?;
        if anchor.modules != document.modules
            || anchor.uri != document.uri
            || anchor.byte_length != document.text.len() as u64
            || anchor.fingerprint != document.fingerprint
        {
            return Err(SourcePackageError::InvalidDocument);
        }
    }
    for anchor in &manifest.declarations {
        check_cancel(cancel)?;
        if declarations.get(&anchor.declaration).copied() != Some(anchor) {
            return Err(SourcePackageError::DeclarationMismatch);
        }
    }
    let expected = executable_sources(program, cancel)?;
    let expected = expected
        .iter()
        .map(|record| {
            (
                (&record.target, record.origin),
                (record.authored, record.native_binding),
            )
        })
        .collect::<HashMap<_, _>>();
    if expected.len() != package.executables.len() {
        return Err(SourcePackageError::OriginMismatch);
    }
    let mut seen = HashSet::new();
    for record in &package.executables {
        check_cancel(cancel)?;
        let key = (&record.target, record.origin);
        if !seen.insert(key)
            || expected.get(&key) != Some(&(record.authored, record.native_binding))
        {
            return Err(SourcePackageError::OriginMismatch);
        }
        if let Some(authored) = record.authored {
            validate_body_owner(&declarations, authored, record.origin)?;
        } else if !matches!(
            record.origin,
            ExecutableOrigin::Native | ExecutableOrigin::Intrinsic
        ) {
            return Err(SourcePackageError::OriginMismatch);
        }
    }
    Ok(())
}

fn kind_matches(kind: AuthoredKind, identity: Option<DefinitionKind>) -> bool {
    match kind {
        AuthoredKind::Function
        | AuthoredKind::AsyncFunction
        | AuthoredKind::Native
        | AuthoredKind::Required => matches!(
            identity,
            Some(DefinitionKind::Function | DefinitionKind::Method)
        ),
        AuthoredKind::Constant => identity == Some(DefinitionKind::Const),
        AuthoredKind::StaticInitializer => identity == Some(DefinitionKind::Static),
        AuthoredKind::Declaration => !matches!(
            identity,
            None | Some(
                DefinitionKind::Function
                    | DefinitionKind::Method
                    | DefinitionKind::Const
                    | DefinitionKind::Static
            )
        ),
    }
}

fn validate_body_owner(
    declarations: &HashMap<DefinitionId, &AuthoredDeclaration>,
    authored: DefinitionId,
    origin: ExecutableOrigin,
) -> Result<(), SourcePackageError> {
    let kind = declarations
        .get(&authored)
        .ok_or(SourcePackageError::OriginMismatch)?
        .kind;
    let valid = match origin {
        ExecutableOrigin::Native => kind == AuthoredKind::Native,
        ExecutableOrigin::Intrinsic => false,
        ExecutableOrigin::StaticInitializer => kind == AuthoredKind::StaticInitializer,
        ExecutableOrigin::Named => {
            matches!(kind, AuthoredKind::Function | AuthoredKind::AsyncFunction)
        }
        ExecutableOrigin::Closure
        | ExecutableOrigin::Async
        | ExecutableOrigin::Resume
        | ExecutableOrigin::Adapter => matches!(
            kind,
            AuthoredKind::Function | AuthoredKind::AsyncFunction | AuthoredKind::StaticInitializer
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(SourcePackageError::OriginMismatch)
    }
}
