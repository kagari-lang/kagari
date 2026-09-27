use crate::builtin::BuiltinFunction;
use crate::hir::ExprId;
use crate::hir::ExprKind;
use crate::hir::PlaceId;
use crate::hir::PlaceKind;
use crate::host;
use crate::host::HostSourcePathStep;
use crate::resolver::ResolvedName;
use crate::typeck::BodyTypeEnv;
use crate::typeck::CallTarget;
use crate::typeck::ResolvedHostPath;
use crate::typeck::ResolvedHostPlacePath;
use crate::typeck::body::BodyChecker;
use crate::typeck::body::HostPathNode;
use crate::typeck::completion;
use crate::typeck::ty::display_type_id;
use crate::types::BuiltinType;
use crate::types::TypeId;
use kagari_common::Diagnostic;
use kagari_common::DiagnosticKind;
use kagari_common::cancellation::Cancelled;
use kagari_common::collection::CollectionAccess;
use kagari_common::host_interface;
use kagari_common::host_interface::HostFieldDeclaration;
use kagari_common::host_interface::HostFunctionDeclaration;
use kagari_common::host_interface::HostPathSegmentDeclaration;
use kagari_common::host_interface::PathAccess;

impl<'a> BodyChecker<'a> {
    pub(super) fn infer_host_call_type(
        &mut self,
        call: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let ResolvedName::HostFunction(id) = self.names.expr_resolution(callee)? else {
            return None;
        };
        let declaration = self.names.hosts.function(id)?.clone();
        self.type_table
            .insert_call(call, CallTarget::HostFunction(id), None);
        Some(self.infer_host_signature(&declaration, &declaration.symbol, callee, args, env))
    }

    pub(super) fn infer_host_method_call(
        &mut self,
        call: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let ExprKind::Field { receiver, name } = &self.lowered.module.expr(callee).kind else {
            return None;
        };
        let receiver = *receiver;
        let ty = self.infer_expr_type(receiver, env);
        let TypeId::Host(owner) = ty else {
            return None;
        };
        let id = self.names.hosts.method(&owner, name)?;
        let declaration = self.names.hosts.function(id)?.clone();
        self.type_table
            .insert_call(call, CallTarget::HostFunction(id), Some(receiver));
        let mut operands = vec![receiver];
        operands.extend_from_slice(args);
        Some(self.infer_host_signature(&declaration, &declaration.symbol, callee, &operands, env))
    }

    pub(super) fn infer_host_signature(
        &mut self,
        declaration: &HostFunctionDeclaration,
        name: &str,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        let args = self.infer_typed_args(
            args,
            declaration
                .params
                .iter()
                .map(|parameter| host::signature_type(&parameter.ty)),
            env,
        );
        if args.len() != declaration.params.len() {
            let implicit = usize::from(declaration.method_owner().is_some());
            self.check_builtin_arity(
                name,
                declaration.params.len() - implicit,
                args.len().saturating_sub(implicit),
                callee,
            );
        }
        for (index, parameter) in declaration.params.iter().enumerate() {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            self.check_arg_type(
                name,
                &parameter.name,
                host::signature_type(&parameter.ty),
                index,
                &args,
            );
        }
        host::signature_type(&declaration.return_type)
    }

    pub(super) fn infer_runtime_helper_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let ResolvedName::RuntimeHelper(builtin) = self.names.expr_resolution(callee)? else {
            return None;
        };
        self.type_table
            .insert_call(call_expr, CallTarget::RuntimeHelper(builtin), None);
        let arity = match builtin {
            BuiltinFunction::TypeOf => 1,
            BuiltinFunction::Print => host_interface::standard_log().params.len(),
            BuiltinFunction::GetField => 2,
            BuiltinFunction::SetField | BuiltinFunction::SetIndex => 3,
        };
        if args.len() != arity {
            self.infer_call_args(args, env);
            self.check_builtin_arity("runtime helper", arity, args.len(), callee);
            return Some(TypeId::Error);
        }
        match builtin {
            BuiltinFunction::TypeOf => {
                let _ = self.infer_call_args(args, env);
                Some(TypeId::Builtin(BuiltinType::String))
            }
            BuiltinFunction::GetField => {
                let [base, field_name_expr] = args else {
                    return Some(TypeId::Error);
                };
                let Ok(base_ty) = self.infer_reflection_receiver(*base, env) else {
                    return Some(TypeId::Unknown);
                };
                let Some(field_name) =
                    self.checked_reflection_field_name(*field_name_expr, env, "get_field")
                else {
                    return Some(TypeId::Error);
                };
                Some(base_ty.map_or(TypeId::Unknown, |base_ty| {
                    self.checked_member_type(&base_ty, &field_name, *field_name_expr, false)
                }))
            }
            BuiltinFunction::SetField => {
                let [base, field_name_expr, value] = args else {
                    return Some(TypeId::Error);
                };
                let Ok(base_ty) = self.infer_reflection_receiver(*base, env) else {
                    return Some(TypeId::Unknown);
                };
                self.check_const_write(*base);
                let field_name =
                    self.checked_reflection_field_name(*field_name_expr, env, "set_field");
                let expected = field_name
                    .as_ref()
                    .zip(base_ty.as_ref())
                    .map(|(name, base_ty)| {
                        self.checked_member_type(base_ty, name, *field_name_expr, true)
                    });
                self.check_reflection_assignment_value(*value, expected.as_ref(), env);
                if field_name.is_none() {
                    return Some(TypeId::Error);
                }
                Some(base_ty.unwrap_or(TypeId::Unknown))
            }
            BuiltinFunction::SetIndex => {
                let [base, index, value] = args else {
                    return Some(TypeId::Error);
                };
                let Ok(base_ty) = self.infer_reflection_receiver(*base, env) else {
                    return Some(TypeId::Unknown);
                };
                self.check_const_write(*base);
                if base_ty
                    .as_ref()
                    .is_some_and(|ty| ty.collection_access() == Some(CollectionAccess::ReadOnly))
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidAssignmentTarget {
                            reason: "read-only collection requires writable access".into(),
                        })
                        .with_span(self.lowered.source_map.expr_span(*base)),
                    );
                }
                let index_ty = self.infer_expr_type(*index, env);
                let expected = base_ty.as_ref().and_then(|base_ty| {
                    self.checked_index_type(*index, base_ty, &index_ty, *index)
                });
                self.check_reflection_assignment_value(*value, expected.as_ref(), env);
                Some(base_ty.unwrap_or(TypeId::Unknown))
            }
            BuiltinFunction::Print => Some(self.infer_host_signature(
                &host_interface::standard_log(),
                "print",
                callee,
                args,
                env,
            )),
        }
    }

    pub(super) fn infer_reflection_receiver(
        &mut self,
        receiver: ExprId,
        env: &mut BodyTypeEnv,
    ) -> Result<Option<TypeId>, Cancelled> {
        let ty = self.infer_expr_type(receiver, env);
        let completes =
            completion::expr_can_complete(&self.lowered.module, self.names, receiver, self.cancel)?;
        Ok(completes.then_some(ty))
    }

    pub(super) fn check_reflection_assignment_value(
        &mut self,
        value: ExprId,
        expected: Option<&TypeId>,
        env: &mut BodyTypeEnv,
    ) {
        let found = self.infer_expr_with_coercion(value, env, expected);
        let Ok(completes) =
            completion::expr_can_complete(&self.lowered.module, self.names, value, self.cancel)
        else {
            return;
        };
        if completes
            && let Some(expected) = expected
            && expected.conflicts_with(&found)
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                    expected: display_type_id(expected),
                    found: display_type_id(&found),
                })
                .with_span(self.lowered.source_map.expr_span(value)),
            );
        }
    }

    pub(super) fn checked_reflection_field_name(
        &mut self,
        expression: ExprId,
        env: &mut BodyTypeEnv,
        helper: &str,
    ) -> Option<String> {
        let ty = self.infer_expr_type(expression, env);
        let expected = TypeId::Builtin(BuiltinType::String);
        if ty.conflicts_with(&expected) {
            self.emit_arg_mismatch(helper, "field", &expected, &ty, expression);
            return None;
        }
        if ty.is_unresolved() {
            return None;
        }
        let name = self.string_literal_value(expression);
        if name.is_none() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ReflectionFieldNameNotConstant)
                    .with_span(self.lowered.source_map.expr_span(expression)),
            );
        }
        name
    }

    pub(super) fn resolve_host_declared_field(
        &self,
        owner: &TypeId,
        name: &str,
    ) -> Option<HostFieldDeclaration> {
        let TypeId::Host(id) = owner else {
            return None;
        };
        self.names
            .hosts
            .nominal_type(id)
            .and_then(|id| self.names.hosts.type_declaration(id))
            .and_then(|owner| owner.fields.iter().find(|field| field.name == name))
            .cloned()
    }

    pub(super) fn infer_host_path_read(
        &mut self,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let mut root = expr_id;
        let mut steps = Vec::new();
        loop {
            match &self.lowered.module.expr(root).kind {
                ExprKind::Field { receiver, name } => {
                    steps.push(HostPathNode::Member {
                        node: root,
                        name: name.clone(),
                    });
                    root = *receiver;
                }
                ExprKind::Index { receiver, index } => {
                    steps.push(HostPathNode::Index {
                        node: root,
                        argument: *index,
                    });
                    root = *receiver;
                }
                _ => break,
            }
        }
        if steps.is_empty() {
            return None;
        }
        steps.reverse();
        let mut root_ty = self.infer_expr_type(root, env);
        let mut prefix = 0;
        while !matches!(root_ty, TypeId::Host(_)) && prefix + 1 < steps.len() {
            root = steps[prefix].id();
            root_ty = self.infer_expr_type(root, env);
            prefix += 1;
        }
        steps.drain(..prefix);
        let TypeId::Host(owner) = root_ty else {
            return None;
        };
        let source = steps
            .iter()
            .map(|step| match step {
                HostPathNode::Member { name, .. } => HostSourcePathStep::Member(name.clone()),
                HostPathNode::Index { argument, .. } => {
                    HostSourcePathStep::Index(self.infer_expr_type(*argument, env))
                }
            })
            .collect::<Vec<_>>();
        match self.names.hosts.source_path(&owner, &source) {
            Ok((declaration, contract)) => {
                let mut dynamic_arguments = Vec::new();
                for ((step, declared), result) in steps
                    .iter()
                    .zip(&declaration.segments)
                    .zip(&contract.segments)
                {
                    if let (
                        HostPathNode::Member { node, .. },
                        HostPathSegmentDeclaration::Field(id),
                    ) = (step, declared)
                    {
                        self.type_table.insert_expr_field(*node, id.clone());
                    }
                    if let (
                        HostPathNode::Index { argument, .. },
                        HostPathSegmentDeclaration::Index(index),
                    ) = (step, declared)
                    {
                        dynamic_arguments.push((index.slot, *argument));
                    }
                    let ty = host::signature_type(&result.result);
                    self.type_table.insert_expr(step.id(), ty.clone());
                    env.exprs.insert(step.id(), ty);
                }
                let result = host::signature_type(&contract.result);
                self.type_table.insert_host_path(
                    expr_id,
                    ResolvedHostPath {
                        root,
                        dynamic_arguments,
                        declaration,
                        contract,
                    },
                );
                Some(result)
            }
            Err(reason) => {
                let mut current = TypeId::Host(owner);
                let mut recovered_all_members = true;
                for step in &steps {
                    match step {
                        HostPathNode::Member { node, name } => {
                            let Some(field) = self.resolve_host_declared_field(&current, name)
                            else {
                                recovered_all_members = false;
                                break;
                            };
                            self.type_table.insert_expr_field(*node, field.id.clone());
                            current = host::signature_type(&field.ty);
                        }
                        HostPathNode::Index { .. } => {
                            recovered_all_members = false;
                            if let TypeId::Array(element, _) = current {
                                current = *element;
                            } else {
                                break;
                            }
                        }
                    }
                    self.type_table.insert_expr(step.id(), current.clone());
                    env.exprs.insert(step.id(), current.clone());
                }
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidHostPath {
                        reason: reason.into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
                Some(if recovered_all_members {
                    current
                } else {
                    TypeId::Error
                })
            }
        }
    }

    pub(super) fn infer_host_path_write(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        let mut root = place_id;
        let mut steps = Vec::new();
        loop {
            match &self.lowered.module.place(root).kind {
                PlaceKind::Field { base, name } => {
                    steps.push(HostPathNode::Member {
                        node: root,
                        name: name.clone(),
                    });
                    root = *base;
                }
                PlaceKind::Index { base, index } => {
                    steps.push(HostPathNode::Index {
                        node: root,
                        argument: *index,
                    });
                    root = *base;
                }
                _ => break,
            }
        }
        if steps.is_empty() {
            return None;
        }
        steps.reverse();
        let mut root_ty = self.resolve_readable_place_type(root, env)?;
        let mut prefix = 0;
        while !matches!(root_ty, TypeId::Host(_)) && prefix + 1 < steps.len() {
            root = steps[prefix].id();
            root_ty = self.resolve_readable_place_type(root, env)?;
            prefix += 1;
        }
        steps.drain(..prefix);
        let TypeId::Host(owner) = root_ty else {
            return None;
        };
        let source = steps
            .iter()
            .map(|step| match step {
                HostPathNode::Member { name, .. } => HostSourcePathStep::Member(name.clone()),
                HostPathNode::Index { argument, .. } => {
                    HostSourcePathStep::Index(self.infer_expr_type(*argument, env))
                }
            })
            .collect::<Vec<_>>();
        match self.names.hosts.source_path(&owner, &source) {
            Ok((declaration, contract)) => {
                let mut dynamic_arguments = Vec::new();
                for ((step, declared), result) in steps
                    .iter()
                    .zip(&declaration.segments)
                    .zip(&contract.segments)
                {
                    if let (
                        HostPathNode::Member { node, .. },
                        HostPathSegmentDeclaration::Field(id),
                    ) = (step, declared)
                    {
                        self.type_table.insert_place_field(*node, id.clone());
                    }
                    if let (
                        HostPathNode::Index { argument, .. },
                        HostPathSegmentDeclaration::Index(index),
                    ) = (step, declared)
                    {
                        dynamic_arguments.push((index.slot, *argument));
                    }
                    self.type_table
                        .insert_place(step.id(), host::signature_type(&result.result));
                }
                let result = host::signature_type(&contract.result);
                if declaration.access != PathAccess::ReadWrite {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidHostPath {
                            reason: "host path is read-only".into(),
                        })
                        .with_span(self.lowered.source_map.place_span(place_id)),
                    );
                } else {
                    self.type_table.insert_host_place_path(
                        place_id,
                        ResolvedHostPlacePath {
                            root,
                            dynamic_arguments,
                            declaration,
                            contract,
                        },
                    );
                }
                Some(result)
            }
            Err(reason) => {
                let mut current = TypeId::Host(owner);
                let mut recovered_all_members = true;
                for step in &steps {
                    match step {
                        HostPathNode::Member { node, name } => {
                            let Some(field) = self.resolve_host_declared_field(&current, name)
                            else {
                                recovered_all_members = false;
                                break;
                            };
                            self.type_table.insert_place_field(*node, field.id.clone());
                            current = host::signature_type(&field.ty);
                        }
                        HostPathNode::Index { .. } => {
                            recovered_all_members = false;
                            if let TypeId::Array(element, _) = current {
                                current = *element;
                            } else {
                                break;
                            }
                        }
                    }
                    self.type_table.insert_place(step.id(), current.clone());
                }
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidHostPath {
                        reason: reason.into(),
                    })
                    .with_span(self.lowered.source_map.place_span(place_id)),
                );
                Some(if recovered_all_members {
                    current
                } else {
                    TypeId::Error
                })
            }
        }
    }
}
