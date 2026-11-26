//! Check enum pattern ownership, generic arguments and payload bindings.
//! Uses the resolved variant and expected enum type to assign child pattern types.

use crate::{
    aggregates::VariantSignature,
    declarations::DeclarationId,
    hir::{
        ids::PatternId,
        pattern::{PatternField, PositionalPatterns},
    },
    typeck::{BodyTypeEnv, body::BodyChecker},
    types::TypeId,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

impl BodyChecker<'_> {
    pub(super) fn pattern_variant_signature(
        &self,
        pattern: PatternId,
        path: &str,
    ) -> Option<&VariantSignature> {
        let imported = self
            .names
            .pattern_variants
            .get(&pattern)
            .and_then(|name| self.declarations.resolved_variant(*name))
            .and_then(|declaration| match &declaration.id {
                DeclarationId::Definition(id) => self.aggregates.variant(id),
                _ => None,
            });
        imported.or_else(|| {
            let (owner, name) = path.rsplit_once("::")?;
            let owner = self.resolve_enum_id(owner)?;
            self.aggregates
                .enumeration(&owner)?
                .variants
                .iter()
                .find(|variant| variant.name == name)
        })
    }

    pub(super) fn check_named_pattern(
        &mut self,
        pattern: PatternId,
        path: &str,
        fields: &[PatternField],
        rest: bool,
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) {
        let checked = (|| {
            let variant = self
                .pattern_variant_signature(pattern, path)
                .ok_or_else(|| format!("unknown variant `{path}`"))?;
            let TypeId::Enum(owner) = expected else {
                return Err(format!("expected enum for `{path}`"));
            };
            if owner.declaration != variant.owner
                || path
                    .rsplit_once("::")
                    .is_some_and(|(p, _)| !self.alias_pattern_matches(p, expected))
            {
                return Err(format!("wrong enum owner for `{path}`"));
            }
            let slots = variant.named_slots(fields.iter().map(|f| f.name.as_str()), rest)?;
            let enumeration = self
                .aggregates
                .enumeration(&owner.declaration)
                .expect("checked enum owner");
            let substitution = enumeration
                .generic_params
                .iter()
                .cloned()
                .zip(owner.arguments.iter().cloned())
                .collect();
            let types: Vec<_> = slots
                .iter()
                .map(|&slot| variant.payload[slot].instantiate(&substitution))
                .collect();
            Ok((variant.id, slots, types))
        })();
        match checked {
            Ok((variant, slots, types)) => {
                self.type_table.insert_pattern_variant(pattern, variant);
                self.type_table.insert_pattern_slots(pattern, slots);
                for (field, ty) in fields.iter().zip(types) {
                    self.check_pattern(field.pattern, &ty, env);
                }
            }
            Err(reason) => self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: expected.display_name(self.declarations.definitions()),
                    found: reason,
                })
                .with_span(
                    self.lowered
                        .module
                        .pattern(pattern)
                        .origin
                        .diagnostic_span(),
                ),
            ),
        }
    }

    pub(super) fn check_enum_pattern(
        &mut self,
        pattern: PatternId,
        path: &str,
        fields: &PositionalPatterns,
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) {
        let variant = self
            .pattern_variant_signature(pattern, path)
            .filter(|v| v.shape.labels().is_none())
            .cloned();
        let span = self
            .lowered
            .module
            .pattern(pattern)
            .origin
            .diagnostic_span();
        let mismatch = || {
            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                expected: expected.display_name(self.declarations.definitions()),
                found: format!("enum variant `{path}`"),
            })
            .with_span(span)
        };
        if path
            .rsplit_once("::")
            .is_some_and(|(owner, _)| !self.alias_pattern_matches(owner, expected))
        {
            self.diagnostics.push(mismatch());
            return;
        }
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
        let Some(slots) = fields.slots(variant.payload.len()) else {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: format!("{} payload fields", variant.payload.len()),
                    found: format!("{} payload fields", fields.elements.len()),
                })
                .with_span(span),
            );
            return;
        };
        let substitution = enumeration
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        self.type_table.insert_pattern_variant(pattern, variant.id);
        for (&field, &slot) in fields.elements.iter().zip(&slots) {
            self.check_pattern(
                field,
                &variant.payload[slot].instantiate(&substitution),
                env,
            );
        }
        self.type_table.insert_pattern_slots(pattern, slots);
    }
}
