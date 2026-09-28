use crate::{
    hir::{LocalId, MatchArm, PatternId, PatternKind, pattern::PatternBound},
    resolver::ResolvedName,
    typeck::{BodyTypeEnv, ScalarValue, body::BodyChecker, ty::display_type_id},
    types::TypeId,
};
use kagari_abi::scalar::BuiltinType;
use kagari_common::{Diagnostic, DiagnosticKind, Span};
use std::collections::{BTreeMap, HashMap, HashSet};

impl<'a> BodyChecker<'a> {
    pub(super) fn infer_match_arm_type(
        &mut self,
        arm: &MatchArm,
        scrutinee_ty: &TypeId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let mut arm_env = env.clone();
        self.check_pattern(arm.pattern, scrutinee_ty, &mut arm_env);
        if let Some(guard) = arm.guard
            && self
                .check_condition_type(guard, "match guard", &mut arm_env)
                .is_err()
        {
            return TypeId::Unknown;
        }
        self.infer_expr_with_coercion(arm.expr, &mut arm_env, expected)
    }

    pub(super) fn check_pattern(
        &mut self,
        pattern: PatternId,
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) {
        let span = self.lowered.source_map.pattern_span(pattern);
        if self.check_standard_pattern(pattern, expected, env) {
            return;
        }
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
                            expected: display_type_id(expected),
                            found: "i32 range pattern".into(),
                        })
                        .with_span(span),
                    );
                }
                let start = self.resolve_pattern_bound(start, span);
                let end = self.resolve_pattern_bound(end, span);
                if let (Some(start), Some(end)) = (start, end) {
                    if !matches!(start, ScalarValue::I32(_)) || !matches!(end, ScalarValue::I32(_))
                    {
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
                                expected: display_type_id(expected),
                                found: display_type_id(&value.ty()),
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
                let TypeId::Tuple(types) = expected else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: "tuple pattern".into(),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if elements.len() != types.len() {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
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
            PatternKind::Struct { path, fields } => {
                let path = path.clone();
                let fields = fields.clone();
                let TypeId::Struct(owner) = expected else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("struct pattern `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if self.resolve_struct_id(&path).as_ref() != Some(&owner.declaration) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
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
                                expected: display_type_id(expected),
                                found: format!("unknown field `{}`", field.name),
                            })
                            .with_span(self.lowered.source_map.pattern_span(field.pattern)),
                        );
                        continue;
                    };
                    identities.push(signature.id);
                    self.check_pattern(field.pattern, &signature.ty, env);
                }
                self.type_table.insert_pattern_fields(pattern, identities);
            }
            PatternKind::EnumVariant { path, fields } => {
                let path = path.clone();
                let fields = fields.clone();
                let TypeId::Enum(owner) = expected else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("enum variant `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
                let Some((owner_path, variant_name)) = path.rsplit_once("::") else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("enum variant `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
                if self.resolve_enum_id(owner_path).as_ref() != Some(&owner.declaration) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("enum variant `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                }
                let Some(enumeration) = self.aggregates.enumeration(&owner.declaration) else {
                    return;
                };
                let Some(variant) = enumeration
                    .variants
                    .iter()
                    .find(|variant| variant.name == variant_name)
                    .cloned()
                else {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: display_type_id(expected),
                            found: format!("unknown enum variant `{path}`"),
                        })
                        .with_span(span),
                    );
                    return;
                };
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
                    .zip(owner.arguments.iter().cloned())
                    .collect();
                self.type_table.insert_pattern_variant(pattern, variant.id);
                for (field, ty) in fields.into_iter().zip(variant.payload) {
                    self.check_pattern(field, &ty.instantiate(&substitution), env);
                }
            }
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
                .lookup(path)
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
