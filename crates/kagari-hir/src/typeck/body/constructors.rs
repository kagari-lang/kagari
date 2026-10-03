use crate::{
    declarations::DeclarationId,
    hir::{
        expr::{ExprKind, FieldInit},
        ids::{ExprId, TypeRefId},
        item::storage::ExportItem,
    },
    native::NativeTypeKind,
    resolver::resolved::ResolvedName,
    typeck::{
        BodyTypeEnv,
        body::BodyChecker,
        check, completion, inference,
        table::{ResolvedEnumConstructor, ResolvedStructInit},
        ty::{TypeContext, display_type, display_type_id, resolve_type_in},
    },
    types::{NominalType, TypeId, TypeSubstitution},
};
use std::collections::HashSet;
use {
    kagari_common::identity::DefinitionPath,
    kagari_source::diagnostic::{Diagnostic, DiagnosticKind},
};

impl<'a> BodyChecker<'a> {
    pub(super) fn enum_member(&self, expr: ExprId) -> Option<(DefinitionPath, String)> {
        if let Some(variant) = self
            .names
            .expr_resolution(expr)
            .and_then(|name| self.declarations.imported_types().variant(name))
        {
            let DeclarationId::Definition(id) = &variant.id else {
                return None;
            };
            let variant = self.aggregates.variant(id)?;
            return Some((variant.owner.clone(), variant.name.clone()));
        }
        let member = self.names.qualified_member(expr)?;
        if let ResolvedName::Enum(id) = member.owner {
            return self
                .declarations
                .definition(ResolvedName::Enum(id))
                .map(|owner| (owner.clone(), member.name.clone()));
        }
        let imported = self.declarations.imported_types().resolved(member.owner)?;
        if !matches!(imported.id.item, ExportItem::Enum(_)) {
            return None;
        }
        let DeclarationId::Definition(id) = &imported.declaration.id else {
            return None;
        };
        Some((id.clone(), member.name.clone()))
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
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                    type_name: display_type(&self.lowered.module, ty),
                })
                .with_span(self.lowered.source_map.type_span(ty)),
            );
        }
        check::validate_standard_type_constraints(
            &resolved,
            &env.generic_bounds,
            self.lowered.source_map.type_span(ty),
            self.diagnostics,
            self.cancel,
            Some(self.aggregates),
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
        let native_type = signature.native_type.clone();
        let generic_params = signature.generic_params.clone();
        let target = ResolvedEnumConstructor {
            enumeration: enumeration.clone(),
            variant: variant.as_ref().map(|variant| variant.id.clone()),
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
            Some(TypeId::StandardEnum { kind, args })
                if native_type == Some(NativeTypeKind::Enum(*kind)) =>
            {
                Some(args)
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
        let result = match native_type {
            Some(ref kind) => kind
                .apply(&arguments)
                .expect("checked native enum arguments"),
            None => TypeId::Enum(NominalType {
                associated_types: Default::default(),
                declaration: enumeration,
                arguments,
            }),
        };
        self.type_table.insert_expr(callee, result.clone());
        env.exprs.insert(callee, result.clone());
        if let Some(variant) = variant {
            if native_type.is_some() && variant.payload.is_empty() && expression != callee {
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
        let mut field_tys = Vec::with_capacity(fields.len());
        let mut completes = true;
        for field in fields {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            let parameter = struct_def.fields.iter().find(|member| {
                member.name == field.name
                    && member
                        .visibility
                        .allows(&member.owner.module, self.lowered.source.module_identity())
            });
            let expected = parameter.map(|member| {
                member
                    .ty
                    .argument_context(&substitution, &struct_def.generic_params)
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
            if field_completes
                && let Some(parameter) = parameter
                && inference::infer(
                    &parameter.ty,
                    &actual,
                    &struct_def.generic_params,
                    &mut substitution,
                    self.cancel,
                    Some(self.aggregates),
                )
                .is_err()
            {
                return TypeId::Unknown;
            }
            field_tys.push((field.name.as_str(), field.value, actual, field_completes));
        }
        let arguments = self.finish_inferred_arguments(
            &mut substitution,
            &struct_def.generic_params,
            path,
            expr_id,
            !completes,
        );
        let mut seen = HashSet::new();
        let resolved = ResolvedStructInit {
            structure: struct_def.id.clone(),
            fields: fields
                .iter()
                .map(|init| {
                    struct_def
                        .fields
                        .iter()
                        .find(|field| {
                            field.name == init.name
                                && field.visibility.allows(
                                    &field.owner.module,
                                    self.lowered.source.module_identity(),
                                )
                        })
                        .map(|field| field.id.clone())
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
                self.aggregates
                    .normalize_type(&field.ty.instantiate(&substitution))
            }) else {
                continue;
            };
            if *field_completes {
                let _ = self.solver.constrain(&expected, value_ty, self.cancel);
            }
            if *field_completes && expected.conflicts_with(value_ty) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                        expected: display_type_id(&expected),
                        found: display_type_id(value_ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(*value_expr)),
                );
            }
        }

        self.type_table.insert_struct_init(expr_id, resolved);
        for field in &struct_def.fields {
            if !seen.contains(&field.name) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidStructInitializer {
                        struct_name: path.to_owned(),
                        reason: format!("missing field `{}`", field.name),
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
            }
        }

        TypeId::Struct(NominalType {
            associated_types: Default::default(),
            declaration: struct_def.id,
            arguments,
        })
    }
}
