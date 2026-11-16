//! Normalize non-syntactic member contracts onto actual declaration records.

use crate::{
    analysis::{error::AnalysisError, external::ExternalModule},
    hir::item::module::ImportLeaf,
    lower::LoweredModule,
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::declaration::module::DeclarationError;

impl ExternalModule {
    pub(super) fn attach_members(
        &self,
        lowered: &mut LoweredModule,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisError> {
        for (owner, name) in &self.enum_failures {
            cancel.check()?;
            let mut matching = lowered
                .module
                .enums
                .iter_mut()
                .filter(|item| &item.name == owner)
                .flat_map(|item| &mut item.variants)
                .filter(|variant| &variant.name == name);
            let variant = matching.next().ok_or_else(|| {
                DeclarationError(format!("missing enum failure target {owner}::{name}"))
            })?;
            if matching.next().is_some() {
                return Err(DeclarationError(format!(
                    "ambiguous enum failure target {owner}::{name}"
                ))
                .into());
            }
            variant.reports_failure = true;
        }
        for ((path, alias), namespace) in &self.import_namespaces {
            cancel.check()?;
            let mut matching = lowered.module.imports.iter_mut().filter(|import| import.segments == *path && matches!(&import.kind, ImportLeaf::Named { alias: actual } if actual == alias));
            let import = matching.next().ok_or_else(|| {
                DeclarationError(format!("missing namespace-qualified import {path:?}"))
            })?;
            if matching.next().is_some() {
                return Err(DeclarationError(format!(
                    "ambiguous namespace-qualified import {path:?}"
                ))
                .into());
            }
            import.namespace = Some(*namespace);
        }
        Ok(())
    }
}
