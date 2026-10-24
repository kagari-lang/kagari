//! Pair checked physical source with immutable compiler-captured declaration anchors.
use kagari_bytecode::{
    artifact::ArtifactFingerprint,
    program::verified::VerifiedBytecodeProgram,
    source_package::{SourcePackageError, VerifiedSourcePackage, executable_sources},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        metadata::DefinitionMetadata,
        scoped::RemapDefinitions,
    },
};
use kagari_contract::source::{
    AuthoredDeclaration, AuthoredKind, SourceDocument, SourcePackage, checked_text_fingerprint,
    manifest::{SourceDocumentAnchor, SourceManifest},
};
use kagari_hir::{
    program::CheckedProgram, source_inspection::collect_authored_sources,
    update_source::SourceBodyKind,
};

mod correspondence;

/// Retain exact definitions for a bytecode product of this checked source closure.
///
/// Includes unused/private/generic/named-local declarations. Generated bodies reuse
/// compiler-produced authored-owner edges, including foreign default placements.
/// Exact anchors captured before executable verification must match: equivalent
/// tokens alone cannot establish definition extents. No HIR is retained.
pub fn build_source_package(
    checked: &CheckedProgram,
    bytecode: &VerifiedBytecodeProgram,
    cancel: &CancellationToken,
) -> Result<VerifiedSourcePackage, SourcePackageError> {
    correspondence::validate(checked, bytecode, cancel)?;
    let (manifest, documents) = capture(checked, true, cancel)?;
    let definitions = manifest.definitions().clone();
    let package = SourcePackage {
        executable_fingerprint: ArtifactFingerprint::of_program(
            bytecode.program(),
            bytecode.definitions(),
        )?
        .0,
        documents,
        declarations: manifest.into_records().declarations,
        // These identities already belong to the receiving bytecode scope. Map
        // declarations first via package admission, so build in that same scope.
        executables: vec![],
    };
    let package = DefinitionMetadata::checked(definitions, package, cancel)?;
    let mut package = package
        .records()
        .remap_definitions(&mut DefinitionMapper::new(
            &mut |id| {
                let path = package.definitions().resolve(*id)?.to_path();
                bytecode
                    .definitions()
                    .lookup(&path)
                    .ok_or(DefinitionMappingError::InvalidContract)
            },
            bytecode.definitions(),
            cancel,
        ))?;
    package.executables = executable_sources(bytecode, cancel)?;
    VerifiedSourcePackage::validate(
        DefinitionMetadata::checked(bytecode.definitions().clone(), package, cancel)?,
        bytecode,
        cancel,
    )
}

/// Capture anchors before optimization/verification, without copying source text.
pub(crate) fn build_source_manifest(
    checked: &CheckedProgram,
    cancel: &CancellationToken,
) -> Result<SourceManifest, DefinitionMappingError> {
    Ok(capture(checked, false, cancel)?.0.into_records())
}

fn capture(
    checked: &CheckedProgram,
    retain_text: bool,
    cancel: &CancellationToken,
) -> Result<(DefinitionMetadata<SourceManifest>, Vec<SourceDocument>), DefinitionMappingError> {
    let index = collect_authored_sources(checked, cancel)?;
    let mut anchors = Vec::new();
    for document in &index.records().documents {
        check_cancel(cancel)?;
        anchors.push(SourceDocumentAnchor {
            modules: Vec::new(),
            uri: document.name().to_owned(),
            byte_length: document.text().len() as u64,
            fingerprint: checked_text_fingerprint(document.text(), cancel)?,
        });
    }
    let mut declarations = Vec::new();
    for record in &index.records().declarations {
        check_cancel(cancel)?;
        let kind = match record.kind {
            Some(SourceBodyKind::Function) => AuthoredKind::Function,
            Some(SourceBodyKind::AsyncFunction) => AuthoredKind::AsyncFunction,
            Some(SourceBodyKind::Native) => AuthoredKind::Native,
            Some(SourceBodyKind::Required) => AuthoredKind::Required,
            Some(SourceBodyKind::Constant) => AuthoredKind::Constant,
            Some(SourceBodyKind::StaticInitializer) => AuthoredKind::StaticInitializer,
            None => AuthoredKind::Declaration,
        };
        let module = index.definitions().resolve(record.declaration)?.module();
        let document = &mut anchors[record.document];
        if !document.modules.contains(module) {
            document.modules.push(module.clone());
        }
        declarations.push(AuthoredDeclaration {
            declaration: record.declaration,
            kind,
            document: u32::try_from(record.document)
                .map_err(|_| DefinitionMappingError::LimitExceeded)?,
            range: record.range,
            fingerprint: checked_text_fingerprint(
                &index.records().documents[record.document].text()
                    [record.range.start..record.range.end],
                cancel,
            )?,
        });
    }
    let manifest = SourceManifest {
        documents: anchors,
        declarations,
    };
    manifest.validate(index.definitions(), cancel)?;
    let mut documents = Vec::new();
    if retain_text {
        for (anchor, source) in manifest.documents.iter().zip(&index.records().documents) {
            check_cancel(cancel)?;
            documents.push(SourceDocument {
                modules: anchor.modules.clone(),
                uri: anchor.uri.clone(),
                text: source.text().to_owned(),
                fingerprint: anchor.fingerprint,
            });
        }
    }
    Ok((
        DefinitionMetadata::checked(index.definitions().clone(), manifest, cancel)?,
        documents,
    ))
}

#[cfg(test)]
mod tests;
