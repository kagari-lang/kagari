//! Check patterns against expected types and publish local binding types.
//! Tuple, literal, struct and enum patterns recurse through stored PatternIds;
//! name resolution supplies identities while this stage validates type compatibility.

use crate::{
    hir::{
        expr::MatchArm,
        ids::{LocalId, PatternId},
        pattern::{PatternBound, PatternKind},
    },
    resolver::resolved::ResolvedName,
    typeck::{BodyTypeEnv, body::BodyChecker, scalar::ScalarValue, ty::display_type_id},
    types::{NominalType, TypeId},
};
use kagari_common::span::Span;
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{declaration::names::NameNamespace, scalar::BuiltinType};
use std::collections::{BTreeMap, HashMap, HashSet};

mod declarations;
mod enums;

impl<'a> BodyChecker<'a> {
    pub(super) fn infer_match_arm_type(
        &mut self,
        arm: &MatchArm,
        scrutinee_ty: &TypeId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
        coerce: bool,
    ) -> (TypeId, BodyTypeEnv) {
        let mut arm_env = env.clone();
        self.check_pattern(arm.pattern, scrutinee_ty, &mut arm_env);
        if let Some(guard) = arm.guard
            && self
                .check_condition_type(guard, "match guard", &mut arm_env)
                .is_err()
        {
            return (TypeId::Unknown, arm_env);
        }
        let found = if coerce {
            self.infer_expr_with_coercion(arm.expr, &mut arm_env, expected)
        } else {
            self.infer_expr_type_expected(arm.expr, &mut arm_env, expected)
        };
        (found, arm_env)
    }

    pub(super) fn check_pattern(
        &mut self,
        pattern: PatternId,
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) {
        let context = self.diverging_pattern_context(pattern, expected);
        let expected = context.as_ref().unwrap_or(expected);
        let span = self.lowered.source_map.pattern_span(pattern);
        match &self.lowered.module.pattern(pattern).kind {
            PatternKind::Wildcard => {}
            PatternKind::Or(alternatives) => {
                let original = env.clone();
                let mut canonical = None;
                for (index, alternative) in alternatives.iter().copied().enumerate() {
                    let mut branch = original.clone();
                    self.check_pattern(alternative, expected, &mut branch);
                    let (binding_ids, duplicate) = self.pattern_binding_ids(alternative);
                    let names = binding_ids
                        .into_iter()
                        .filter_map(|(name, local)| {
                            branch.locals.get(&local).cloned().map(|ty| (name, ty))
                        })
                        .collect::<BTreeMap<_, _>>();
                    if duplicate {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: "each name bound once per alternative".into(),
                                found: format!("bindings {names:?}"),
                            })
                            .with_span(self.lowered.source_map.pattern_span(alternative)),
                        );
                    }
                    if let Some(first) = &canonical
                        && (first != &names)
                    {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: "the same bindings in every alternative".into(),
                                found: format!("bindings {names:?}"),
                            })
                            .with_span(self.lowered.source_map.pattern_span(alternative)),
                        );
                    }
                    if index == 0 {
                        *env = branch;
                        canonical = Some(names);
                    }
                }
            }
            PatternKind::Range { start, end, .. } => {
                let integer = TypeId::Builtin(BuiltinType::I32);
                if expected.conflicts_with(&integer) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(self.declarations.definitions(), expected),
                            found: "i32 range pattern".into(),
                        })
                        .with_span(span),
                    );
                }
                let start = self.resolve_pattern_bound(start, span);
                let end = self.resolve_pattern_bound(end, span);
                if let (Some(start), Some(end)) = (start, end) {
                    if start.ty() != integer || end.ty() != integer {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: "i32 bounds".into(),
                                found: "non-integer range bound".into(),
                            })
                            .with_span(span),
                        );
                    } else {
                        self.type_table.insert_pattern_range(pattern, start, end);
                    }
                }
            }
            PatternKind::Name { name, .. }
                if self.names.pattern_variants.contains_key(&pattern) =>
            {
                let name = name.clone();
                self.check_enum_pattern(pattern, &name, &[], expected, env);
            }
            PatternKind::Name { local, .. } => {
                env.locals.insert(*local, expected.clone());
                self.type_table.insert_local(*local, expected.clone());
            }
            PatternKind::Literal(literal) => match ScalarValue::parse_expected(
                literal,
                match expected {
                    TypeId::Builtin(ty) => Some(*ty),
                    _ => None,
                },
                false,
            ) {
                Ok(value) => {
                    if value.ty().conflicts_with(expected) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: display_type_id(
                                    self.declarations.definitions(),
                                    expected,
                                ),
                                found: display_type_id(
                                    self.declarations.definitions(),
                                    &value.ty(),
                                ),
                            })
                            .with_span(span),
                        );
                    }
                    self.type_table.insert_pattern_scalar(pattern, value);
                }
                Err(reason) => self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidLiteral {
                        reason: reason.to_owned(),
                    })
                    .with_span(span),
                ),
            },
            PatternKind::Tuple(elements) => {
                if elements.is_empty() && *expected == TypeId::Builtin(BuiltinType::Unit) {
                    return;
                }
                let TypeId::Tuple(types) = expected else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(self.declarations.definitions(), expected),
                            found: "tuple pattern".into(),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if elements.len() != types.len() {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(self.declarations.definitions(), expected),
                            found: format!("tuple of {} elements", elements.len()),
                        })
                        .with_span(span),
                    );
                    return;
                }
                let elements = elements.clone();
                let types = types.clone();
                for (element, ty) in elements.into_iter().zip(types.iter()) {
                    self.check_pattern(element, ty, env);
                }
            }
            PatternKind::Struct {
                path,
                fields,
                has_rest,
            } => {
                let path = path.clone();
                let fields = fields.clone();
                let has_rest = *has_rest;
                let TypeId::Struct(owner) = expected else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(self.declarations.definitions(), expected),
                            found: format!("struct pattern `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if self.resolve_struct_id(&path).as_ref() != Some(&owner.declaration) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(self.declarations.definitions(), expected),
                            found: format!("struct pattern `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                }
                let mut identities = Vec::new();
                let mut seen = HashSet::new();
                for field in fields {
                    if !seen.insert(field.name.clone()) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: "distinct struct fields".into(),
                                found: format!("duplicate field `{}`", field.name),
                            })
                            .with_span(self.lowered.source_map.pattern_span(field.pattern)),
                        );
                        continue;
                    }
                    let Some(signature) = self.resolve_field(expected, &field.name) else {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: display_type_id(
                                    self.declarations.definitions(),
                                    expected,
                                ),
                                found: format!("unknown field `{}`", field.name),
                            })
                            .with_span(self.lowered.source_map.pattern_span(field.pattern)),
                        );
                        continue;
                    };
                    identities.push(signature.id);
                    self.check_pattern(field.pattern, &signature.ty, env);
                }
                if !has_rest {
                    let resolved_fields = identities.iter().copied().collect::<HashSet<_>>();
                    let missing = self
                        .aggregates
                        .structure(&owner.declaration)
                        .expect("struct signature catalog")
                        .fields
                        .iter()
                        .filter(|field| !resolved_fields.contains(&field.id))
                        .map(|field| format!("`{}`", field.name))
                        .collect::<Vec<_>>();
                    if !missing.is_empty() {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                                expected: format!("all fields of `{path}` or `..`"),
                                found: format!("missing fields: {}", missing.join(", ")),
                            })
                            .with_span(span),
                        );
                    }
                }
                self.type_table.insert_pattern_fields(pattern, identities);
            }
            PatternKind::EnumVariant { path, fields } => {
                let path = path.clone();
                let fields = fields.clone();
                self.check_enum_pattern(pattern, &path, &fields, expected, env);
            }
        }
    }

    /// A diverging input constrains no field type. Retain structural context so
    /// checking still validates nominal identity, access and completeness.
    /// Unconstrained leaves/arguments stay uninhabited, not fabricated runtime values.
    pub(super) fn diverging_pattern_context(
        &self,
        pattern: PatternId,
        expected: &TypeId,
    ) -> Option<TypeId> {
        if !expected.is_never() {
            return None;
        }
        match &self.lowered.module.pattern(pattern).kind {
            PatternKind::Tuple(elements) if elements.is_empty() => {
                Some(TypeId::Builtin(BuiltinType::Unit))
            }
            PatternKind::Tuple(elements) => Some(TypeId::Tuple(
                elements
                    .iter()
                    .map(|element| {
                        self.diverging_pattern_context(*element, expected)
                            .unwrap_or_else(|| expected.clone())
                    })
                    .collect(),
            )),
            PatternKind::Struct { path, .. } => {
                let declaration = self.resolve_struct_id(path)?;
                let structure = self.aggregates.structure(&declaration)?;
                Some(TypeId::Struct(NominalType {
                    declaration,
                    arguments: vec![expected.clone(); structure.generic_params.len()],
                    associated_types: Default::default(),
                }))
            }
            PatternKind::Or(alternatives) => alternatives
                .first()
                .and_then(|alternative| self.diverging_pattern_context(*alternative, expected)),
            _ => None,
        }
    }

    pub(super) fn pattern_binding_ids(
        &self,
        pattern: PatternId,
    ) -> (HashMap<String, LocalId>, bool) {
        let mut names = HashMap::new();
        let mut duplicate = false;
        let mut work = vec![pattern];
        while let Some(pattern) = work.pop() {
            match &self.lowered.module.pattern(pattern).kind {
                PatternKind::Name { name, local } => {
                    if self.names.pattern_variants.contains_key(&pattern) {
                        continue;
                    }
                    duplicate |= names.insert(name.clone(), *local).is_some();
                }
                PatternKind::Or(alternatives) => work.extend(alternatives.first().copied()),
                PatternKind::Tuple(alternatives)
                | PatternKind::EnumVariant {
                    fields: alternatives,
                    ..
                } => work.extend(alternatives.iter().copied()),
                PatternKind::Struct { fields, .. } => {
                    work.extend(fields.iter().map(|field| field.pattern));
                }
                PatternKind::Wildcard | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
            }
        }
        (names, duplicate)
    }

    pub(super) fn resolve_pattern_bound(
        &mut self,
        bound: &PatternBound,
        span: Span,
    ) -> Option<ScalarValue> {
        let value = match bound {
            PatternBound::Literal(literal) => ScalarValue::parse(literal).ok(),
            PatternBound::Path(path) => self
                .declarations
                .names
                .lookup(path, NameNamespace::Value)
                .and_then(|entry| entry.target())
                .and_then(|target| match target {
                    ResolvedName::Const(id) => self.const_values?.get(&id).cloned(),
                    _ => None,
                }),
        };
        if value.is_none() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: "scalar constant range bound".into(),
                    found: format!("{bound:?}"),
                })
                .with_span(span),
            );
        }
        value
    }
}
