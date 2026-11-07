//! Standalone recovery reuses declared-module discovery without inventing imports.
use crate::{
    documents::DocumentInput,
    workspace::{
        CaptureFailure, CaptureIssue, Origin,
        capture::{StagedContext, package_failure},
        origins::{input_error, issue, native_key},
        overlay::OverlayInput,
    },
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_package::{
    error::PackageError,
    modules::{ModuleDiscovery, ModuleLimits, SourceInput, SourceText},
    tooling::ToolingInputs,
};

struct SingleInput<'a>(&'a DocumentInput);

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
        let source = self.0.source.as_ref().ok_or_else(|| {
            input_error(
                self.0.uri.as_str(),
                "document synchronization is unavailable",
            )
        })?;
        if source.text().len() > max_bytes {
            return Err(input_error(
                self.0.uri.as_str(),
                "source exceeds byte limit",
            ));
        }
        Ok(Some(SourceText {
            key: self.0.source_name.clone(),
            origin: self.0.uri.as_str().into(),
            source: source.clone(),
        }))
    }
}

pub(super) fn stage(
    document: &DocumentInput,
    selected: Option<(usize, &OverlayInput)>,
    unresolved_project: bool,
    cancel: &CancellationToken,
) -> Result<StagedContext, CaptureFailure> {
    let (discovery, workspace, root_identity, order) =
        if let Some((order, input)) = selected.filter(|_| !unresolved_project) {
            let relative = input
                .relative(&document.source_name)
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
                    &SingleInput(document),
                    "buffer.kgr",
                    ModuleLimits::default(),
                    cancel,
                )
                .map_err(package_failure)?,
                document.source_name.clone(),
                ModuleIdentity::single_file(document.source_name.clone()),
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
        semantic_available: !unresolved_project && !document.unavailable && !conflict,
        inputs,
        errors,
        root_order: order,
    })
}
