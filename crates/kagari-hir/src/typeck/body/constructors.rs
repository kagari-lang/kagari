//! Check struct fields and enum payload applications against aggregate declarations.
//! Records selected constructor/field identities and instantiated result types;
//! missing, duplicate and incompatible inputs remain source diagnostics.

use crate::{
    declarations::DeclarationId,
    hir::{
        expr::{ExprKind, FieldInit},
        ids::{ExprId, TypeRefId},
    },
    imports::SourceItem,
    resolver::resolved::ResolvedName,
    typeck::{
        BodyTypeEnv, applications,
        body::BodyChecker,
        check, completion, inference,
        table::{ResolvedEnumConstructor, ResolvedStructInit},
        ty::{TypeContext, display_type, display_type_id, resolve_type_in},
    },
    types::{NominalType, TypeId, TypeSubstitution},
};
use std::collections::HashSet;
use {
    kagari_common::identity::table::DefinitionId,
    kagari_source::diagnostic::{Diagnostic, DiagnosticKind},
};

impl<'a> BodyChecker<'a> {
    pub(super) fn enum_member(&self, expr: ExprId) -> Option<(DefinitionId, String)> {
        if let Some(variant) = self
            .names
            .expr_resolution(expr)
            .and_then(|name| self.declarations.resolved_variant(name))
        {
            let DeclarationId::Definition(id) = &variant.id else {
                return None;
            };
            let variant = self.aggregates.variant(id)?;
            return Some((variant.owner, variant.name.clone()));
        }
        let member = self.names.qualified_member(expr)?;
        if let ResolvedName::Enum(id) = member.owner {
            return self
                .declarations
                .definition(ResolvedName::Enum(id))
                .map(|owner| (*owner, member.name.clone()));
        }
        let imported = self
            .declarations
            .imported_types()
            .resolved(member.owner.clone())?;
        if !matches!(imported.id.item, SourceItem::Enum(_)) {
            return None;
        }
        let DeclarationId::Definition(id) = &imported.declaration.id else {
            return None;
        };
        Some((*id, member.name.clone()))
    }

    pub(super) fn resolve_constructor_type(&mut self, ty: TypeRefId, env: &BodyTypeEnv) -> TypeId {
        self.prepare_annotation_holes(ty);
        let resolved = resolve_type_in(
            &self.lowered.module,
            ty,
            TypeContext {
                declarations: self.declarations,
                generics: &env.generics,
                self_type: None,
                implementation: None,
            },
            self.type_table,
            self.cancel,
        );
        if resolved.is_unresolved() {
            // Recovery may discard the constructor's result type. Check known
            // parts of its explicit application before those facts are lost.
            applications::validate(
                applications::ApplicationContext {
                    definitions: self.declarations.definitions(),
                    catalog: self.aggregates,
                    receiver: None,
                },
                &resolved,
                &env.generic_bounds,
                self.lowered.source_map.type_span(ty),
                self.diagnostics,
                self.cancel,
            );
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                    type_name: display_type(&self.lowered.module, ty),
                })
                .with_span(self.lowered.source_map.type_span(ty)),
            );
        }
        check::validate_reference_position(
            &resolved,
            self.lowered.source_map.type_span(ty),
            self.diagnostics,
        );
        resolved
    }

    pub(super) fn infer_enum_constructor(
        &mut self,
        expression: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        if self.local_function_id(callee).is_some() {
            return None;
        }
        let explicit = match &self.lowered.module.expr(callee).kind {
            ExprKind::Name { explicit_type, .. } => {
                explicit_type.map(|ty| self.resolve_constructor_type(ty, env))
            }
            _ => None,
        };
        let (enumeration, member) = match self.enum_member(callee) {
            Some(owner) => owner,
            None if explicit.is_some() => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidValueTarget {
                        name: "explicit enum constructor".into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
                self.infer_call_args(args, env);
                return Some(TypeId::Error);
            }
            None => return None,
        };
        let expected = explicit.as_ref().or(expected);
        let signature = self
            .aggregates
            .enumeration(&enumeration)
            .expect("enum signature catalog");
        let variant = signature
            .variants
            .iter()
            .find(|variant| variant.name == member)
            .cloned();
        let name = format!("{}::{}", signature.declaration.name, member);
        let generic_params = signature.generic_params.clone();
        let target = ResolvedEnumConstructor {
            enumeration,
            variant: variant.as_ref().map(|variant| variant.id),
        };
        self.type_table
            .insert_enum_constructor(callee, target.clone());
        self.type_table.insert_enum_constructor(expression, target);
        // Every argument is checked once, even when the variant is absent or its
        // signature is erroneous. Known target facts survive argument failures.
        let mut substitution = TypeSubstitution::default();
        let expected_arguments = match expected {
            Some(TypeId::Enum(nominal)) if nominal.declaration == enumeration => {
                Some(&nominal.arguments)
            }
            _ => None,
        };
        if let Some(arguments) = expected_arguments
            && arguments.len() == generic_params.len()
        {
            substitution.extend(
                generic_params
                    .iter()
                    .cloned()
                    .zip(arguments.iter().cloned()),
            );
        }
        self.seed_explicit_arguments(callee, &generic_params, &mut substitution);
        let actual = self.infer_generic_args(
            args,
            variant
                .iter()
                .flat_map(|variant| variant.payload.iter().cloned()),
            &generic_params,
            &mut substitution,
            env,
        );
        if self.cancel.check().is_err() {
            return Some(TypeId::Unknown);
        }
        let Ok(completes) = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            expression,
            self.cancel,
        ) else {
            return Some(TypeId::Unknown);
        };
        let arguments = self.finish_inferred_arguments(
            &mut substitution,
            &generic_params,
            &name,
            callee,
            !completes,
        );
        let result = TypeId::Enum(NominalType {
            associated_types: Default::default(),
            declaration: enumeration,
            arguments,
        });
        self.type_table.insert_expr(callee, result.clone());
        env.exprs.insert(callee, result.clone());
        if let Some(variant) = variant {
            if variant.payload.is_empty() && expression != callee {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                        type_name: format!("{name} (unit variant; omit parentheses)"),
                    })
                    .with_span(self.lowered.source_map.expr_span(expression)),
                );
            }
            if actual.len() != variant.payload.len() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CallArityMismatch {
                        function_name: name.clone(),
                        expected: variant.payload.len(),
                        found: actual.len(),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
            }
            for (index, expected) in variant.payload.iter().enumerate() {
                if self.cancel.check().is_err() {
                    return Some(TypeId::Unknown);
                }
                self.check_arg_type(
                    &name,
                    &format!("payload[{index}]"),
                    expected.instantiate(&substitution),
                    index,
                    &actual,
                );
            }
        } else {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownName { name })
                    .with_span(self.lowered.source_map.expr_span(callee)),
            );
        }
        Some(result)
    }

    pub(super) fn infer_struct_init_type(
        &mut self,
        path: &str,
        fields: &[FieldInit],
        base: Option<ExprId>,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let Some(struct_def) = self
            .resolve_struct_id(path)
            .and_then(|id| self.aggregates.structure(&id))
            .cloned()
        else {
            for field in fields {
                self.infer_expr_type(field.value, env);
            }
            if let Some(base) = base {
                self.infer_expr_type(base, env);
            }
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                    struct_name: path.to_owned(),
                    reason: "unknown struct".to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
            return TypeId::Error;
        };

        let mut substitution = TypeSubstitution::default();
        if let Some(TypeId::Struct(nominal)) = expected
            && nominal.declaration == struct_def.id
            && nominal.arguments.len() == struct_def.generic_params.len()
        {
            substitution.extend(
                struct_def
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(nominal.arguments.iter().cloned()),
            );
        }
        // Update operands share persistent slots before finalization, so a typed
        // base can refine an earlier factory field on the next bounded body round.
        if base.is_some() && self.body_inference {
            for (index, parameter) in struct_def.generic_params.iter().enumerate() {
                let variable = self.inference_variable(expr_id, index + 1024);
                if let Some(seeded) = substitution.get(parameter) {
                    let _ = self.solver.constrain(&variable, seeded, self.cancel);
                }
                substitution.entry(parameter.clone()).or_insert(variable);
            }
        }
        let mut field_tys = Vec::with_capacity(fields.len());
        let mut completes = true;
        for field in fields {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            let parameter = struct_def.fields.iter().find(|member| {
                member.name == field.name
                    && member.visibility.allows(
                        self.declarations
                            .definitions()
                            .resolve(member.owner)
                            .expect("scoped member owner")
                            .module(),
                        self.lowered.source.module_identity(),
                    )
            });
            let expected = parameter.map(|member| {
                self.solver.resolve(
                    &member
                        .ty
                        .argument_context(&substitution, &struct_def.generic_params),
                )
            });
            let actual = self.infer_expr_with_coercion(field.value, env, expected.as_ref());
            let Ok(field_completes) = completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                field.value,
                self.cancel,
            ) else {
                return TypeId::Unknown;
            };
            completes &= field_completes;
            if base.is_some()
                && field_completes
                && let Some(expected) = &expected
            {
                let _ = self.solver.constrain(expected, &actual, self.cancel);
            }
            if field_completes
                && let Some(parameter) = parameter
                && inference::infer(
                    &parameter.ty,
                    &actual,
                    &struct_def.generic_params,
                    &mut substitution,
                    self.cancel,
                    Some(inference::Context {
                        definitions: self.declarations.definitions(),
                        aggregates: self.aggregates,
                        assumptions: &env.generic_bounds,
                    }),
                )
                .is_err()
            {
                return TypeId::Unknown;
            }
            field_tys.push((field.name.as_str(), field.value, actual, field_completes));
        }
        let template = TypeId::Struct(NominalType {
            associated_types: Default::default(),
            declaration: struct_def.id,
            arguments: struct_def
                .generic_params
                .iter()
                .cloned()
                .map(TypeId::Generic)
                .collect(),
        });
        let base_ty = if let Some(base) = base {
            let expected = self.aggregates.normalize_type(
                self.declarations.definitions(),
                &self
                    .solver
                    .resolve(&template.argument_context(&substitution, &struct_def.generic_params)),
            );
            let actual = self.infer_expr_type_expected(base, env, Some(&expected));
            let actual = self
                .aggregates
                .normalize_type(self.declarations.definitions(), &actual);
            let Ok(base_completes) = completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                base,
                self.cancel,
            ) else {
                return TypeId::Unknown;
            };
            completes &= base_completes;
            if base_completes {
                let _ = self.solver.constrain(&expected, &actual, self.cancel);
                if inference::infer(
                    &template,
                    &actual,
                    &struct_def.generic_params,
                    &mut substitution,
                    self.cancel,
                    None,
                )
                .is_err()
                {
                    return TypeId::Unknown;
                }
            }
            Some((actual, base_completes))
        } else {
            None
        };
        if base.is_some() && !completes {
            // An early update slot is not a produced argument. Preserve known
            // members, but let the existing termination suppression handle an
            // entirely unconstrained binder as it does without update slots.
            substitution.retain(|_, ty| !matches!(self.solver.resolve(ty), TypeId::Inference(_)));
        }
        let arguments = self.finish_inferred_arguments(
            &mut substitution,
            &struct_def.generic_params,
            path,
            expr_id,
            !completes,
        );
        let result = TypeId::Struct(NominalType {
            associated_types: Default::default(),
            declaration: struct_def.id,
            arguments,
        });
        let result = if base.is_some() {
            self.aggregates.normalize_type(
                self.declarations.definitions(),
                &self.solver.resolve(&result),
            )
        } else {
            result
        };
        if let (Some(base), Some((actual, true))) = (base, &base_ty) {
            self.check_struct_update_base(path, base, &result, actual);
        }
        let mut seen = HashSet::new();
        let mut resolved = ResolvedStructInit {
            structure: struct_def.id,
            base_fields: Vec::new(),
            fields: fields
                .iter()
                .map(|init| {
                    struct_def
                        .fields
                        .iter()
                        .find(|field| {
                            field.name == init.name
                                && field.visibility.allows(
                                    self.declarations
                                        .definitions()
                                        .resolve(field.owner)
                                        .expect("scoped member owner")
                                        .module(),
                                    self.lowered.source.module_identity(),
                                )
                        })
                        .map(|field| field.id)
                })
                .collect(),
        };
        for ((name, value_expr, value_ty, field_completes), target) in
            field_tys.iter().zip(&resolved.fields)
        {
            if !seen.insert((*name).to_owned()) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                        struct_name: path.to_owned(),
                        reason: format!("duplicate field `{name}`"),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
                continue;
            }

            let Some(field) = target else {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                        struct_name: path.to_owned(),
                        reason: format!("unknown field `{name}`"),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
                continue;
            };

            let Some(expected) = self.aggregates.field(field).map(|field| {
                self.aggregates.normalize_type(
                    self.declarations.definitions(),
                    &field.ty.instantiate(&substitution),
                )
            }) else {
                continue;
            };
            if *field_completes {
                let _ = self.solver.constrain(&expected, value_ty, self.cancel);
            }
            if *field_completes && expected.conflicts_with(value_ty) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                        expected: display_type_id(self.declarations.definitions(), &expected),
                        found: display_type_id(self.declarations.definitions(), value_ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
            }
        }

        let explicit: HashSet<_> = resolved.fields.iter().flatten().copied().collect();
        for field in &struct_def.fields {
            if base.is_some() {
                if !field.visibility.allows(
                    self.declarations
                        .definitions()
                        .resolve(field.owner)
                        .expect("scoped member owner")
                        .module(),
                    self.lowered.source.module_identity(),
                ) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                            struct_name: path.to_owned(),
                            reason: format!(
                                "field `{}` is not accessible for struct update",
                                field.name
                            ),
                        })
                        .with_span(self.lowered.source_map.expr_span(expr_id)),
                    );
                } else if !explicit.contains(&field.id) {
                    resolved.base_fields.push(field.id);
                }
            } else if !seen.contains(&field.name) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                        struct_name: path.to_owned(),
                        reason: format!("missing field `{}`", field.name),
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
            }
        }

        self.type_table.insert_struct_init(expr_id, resolved);
        result
    }

    fn check_struct_update_base(
        &mut self,
        path: &str,
        base: ExprId,
        expected: &TypeId,
        actual: &TypeId,
    ) {
        let expected = self.aggregates.normalize_type(
            self.declarations.definitions(),
            &self.solver.resolve(expected),
        );
        let actual = self.aggregates.normalize_type(
            self.declarations.definitions(),
            &self.solver.resolve(actual),
        );
        if matches!(
            actual,
            TypeId::Unknown | TypeId::Error | TypeId::Inference(_)
        ) {
            return;
        }
        // Incomplete arguments do not hide an independently wrong base category
        // or owner, including after an earlier field terminates construction.
        let same_owner = matches!((&expected, &actual), (TypeId::Struct(expected), TypeId::Struct(actual))
            if expected.declaration == actual.declaration && expected.arguments.len() == actual.arguments.len());
        // No coercion, structural lookalike or type-changing update: even an
        // all-overridden base must have exactly the constructed owned type.
        if !same_owner
            || expected.conflicts_with(&actual)
            || (!actual.is_unresolved() && !expected.is_unresolved() && actual != expected)
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                    struct_name: path.to_owned(),
                    reason: format!(
                        "update base must have type `{}`, found `{}`",
                        display_type_id(self.declarations.definitions(), &expected),
                        display_type_id(self.declarations.definitions(), &actual)
                    ),
                })
                .with_span(self.lowered.source_map.expr_span(base)),
            );
        }
    }
}
