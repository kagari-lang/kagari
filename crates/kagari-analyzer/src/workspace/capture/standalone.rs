//! Standalone recovery reuses declared-module discovery without inventing imports.
use crate::{
    documents::DocumentInput,
    workspace::{
        CaptureFailure, CaptureIssue, Origin,
        capture::{StagedContext, package_failure},
        origins::{input_error, issue, native_key},
        overlay::OverlayInput,
        relations::ContextRelations,
    },
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_package::{
    error::PackageError,
    modules::{ModuleDiscovery, ModuleLimits, SourceInput, SourceText},
    tooling::ToolingInputs,
};
use std::path::Path;

struct SingleInput<'a> {
    document: &'a DocumentInput,
    key: &'a str,
    conflicting: bool,
}

impl SourceInput for SingleInput<'_> {
    fn read(
        &self,
        path: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<SourceText>, PackageError> {
        cancel.check()?;
        if path != "buffer.kgr" {
            return Ok(None);
        }
        if self.conflicting {
            return Err(input_error(
                self.key,
                "open URI aliases have conflicting source text",
            ));
        }
        let source = self.document.source.as_ref().ok_or_else(|| {
            input_error(
                self.document.uri.as_str(),
                "document synchronization is unavailable",
            )
        })?;
        if source.text().len() > max_bytes {
            return Err(input_error(
                self.document.uri.as_str(),
                "source exceeds byte limit",
            ));
        }
        Ok(Some(SourceText {
            key: self.key.into(),
            origin: self.document.uri.as_str().into(),
            source: source.clone(),
        }))
    }
}

pub(super) fn stage(
    document: &DocumentInput,
    physical_key: &str,
    conflicting: bool,
    selected: Option<(usize, &OverlayInput)>,
    unresolved_project: bool,
    cancel: &CancellationToken,
) -> Result<StagedContext, CaptureFailure> {
    let (discovery, workspace, root_identity, order) =
        if let Some((order, input)) = selected.filter(|_| !unresolved_project) {
            let relative = input
                .canonical_relative(&document.source_name)
                .expect("selected root contains document");
            let physical = native_key(&input.approved(&relative).map_err(package_failure)?)
                .map_err(CaptureFailure::InvalidInput)?;
            (
                ModuleDiscovery::load(input, &relative, ModuleLimits::default(), cancel)
                    .map_err(package_failure)?,
                input.canonical_root().map_err(package_failure)?,
                ModuleIdentity::single_file(physical),
                order,
            )
        } else {
            (
                ModuleDiscovery::load(
                    &SingleInput {
                        document,
                        key: physical_key,
                        conflicting,
                    },
                    "buffer.kgr",
                    ModuleLimits::default(),
                    cancel,
                )
                .map_err(package_failure)?,
                native_key(Path::new(physical_key)).map_err(CaptureFailure::InvalidInput)?,
                ModuleIdentity::single_file(
                    native_key(Path::new(physical_key)).map_err(CaptureFailure::InvalidInput)?,
                ),
                usize::MAX,
            )
        };
    let inputs =
        ToolingInputs::from_modules(&discovery, root_identity, cancel).map_err(package_failure)?;
    let mut errors = inputs.errors().iter().map(issue).collect::<Vec<_>>();
    if unresolved_project {
        errors.push(CaptureIssue {
            uri: Some(document.uri.clone()),
            range: None,
            message:
                "project resolution is unavailable; retained text supports document-only recovery"
                    .into(),
        });
    }
    let origins = inputs
        .sources()
        .iter()
        .map(|unit| {
            let uri = selected
                .and_then(|(_, input)| input.uri(&unit.source.key))
                .or_else(|| Some(document.uri.clone()));
            (
                unit.module.clone(),
                Origin {
                    uri,
                    module: unit.module.clone(),
                    provider_key: unit.source.key.clone(),
                    read_only: false,
                },
            )
        })
        .collect();
    let conflict = selected.is_some_and(|(_, input)| inputs.errors().iter().any(|error| matches!(error, PackageError::Input { origin, .. } if input.conflicting(origin))));
    Ok(StagedContext {
        workspace,
        origins,
        semantic_available: !unresolved_project
            && !document.unavailable
            && !conflict
            && !conflicting,
        inputs,
        errors,
        root_order: order,
        relations: ContextRelations {
            folders: selected
                .map(|(_, input)| input.canonical_root())
                .transpose()
                .map_err(package_failure)?
                .into_iter()
                .collect(),
            ..ContextRelations::default()
        },
    })
}
