use crate::{
    declarations::DeclarationId,
    hir::ids::PatternId,
    typeck::{BodyTypeEnv, body::BodyChecker},
    types::TypeId,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

impl BodyChecker<'_> {
    pub(super) fn check_enum_pattern(
        &mut self,
        pattern: PatternId,
        path: &str,
        fields: &[PatternId],
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) {
        let imported = self
            .names
            .pattern_variants
            .get(&pattern)
            .and_then(|name| self.declarations.imported_types().variant(*name))
            .and_then(|declaration| match &declaration.id {
                DeclarationId::Definition(id) => self.aggregates.variant(id),
                _ => None,
            });
        let variant = imported
            .or_else(|| {
                let (owner, name) = path.rsplit_once("::")?;
                let owner = self.resolve_enum_id(owner)?;
                self.aggregates
                    .enumeration(&owner)?
                    .variants
                    .iter()
                    .find(|variant| variant.name == name)
            })
            .cloned();
        let span = self.lowered.source_map.pattern_span(pattern);
        let mismatch = || {
            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                expected: expected.display_name(),
                found: format!("enum variant `{path}`"),
            })
            .with_span(span)
        };
        let Some(variant) = variant else {
            self.diagnostics.push(mismatch());
            return;
        };
        let enumeration = self
            .aggregates
            .enumeration(&variant.owner)
            .expect("checked variant owner");
        let arguments = match expected {
            TypeId::Enum(nominal) if nominal.declaration == variant.owner => &nominal.arguments,
            _ => {
                self.diagnostics.push(mismatch());
                return;
            }
        };
        if arguments.len() != enumeration.generic_params.len() {
            self.diagnostics.push(mismatch());
            return;
        }
        if variant.payload.len() != fields.len() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: format!("{} payload fields", variant.payload.len()),
                    found: format!("{} payload fields", fields.len()),
                })
                .with_span(span),
            );
            return;
        }
        let substitution = enumeration
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        self.type_table.insert_pattern_variant(pattern, variant.id);
        for (field, ty) in fields.iter().copied().zip(variant.payload) {
            self.check_pattern(field, &ty.instantiate(&substitution), env);
        }
    }
}
