//! Check typed declared host field/index paths.
//! Records declared access contracts and path selections for lowering; HIR analysis
//! does not borrow host state or invoke the registered Rust implementation.

use crate::{
    hir::{
        expr::ExprKind,
        ids::{ExprId, PlaceId},
        place::PlaceKind,
    },
    native::access::SourcePathStep,
    typeck::{
        BodyTypeEnv,
        body::{BodyChecker, HostPathNode},
        table::{ResolvedHostPath, ResolvedHostPlacePath},
    },
    types::{TypeId, semantic::raise_type},
};
use kagari_common::identity::{DefinitionKind, table::DefinitionId};

use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::declaration::{host::PathAccess, path::HostPathSegmentDeclaration};

impl<'a> BodyChecker<'a> {
    pub(super) fn resolve_host_declared_field(
        &self,
        owner: &TypeId,
        name: &str,
    ) -> Option<(DefinitionId, TypeId)> {
        let TypeId::Host(id) = owner else {
            return None;
        };
        let owner = self.declarations.external_type(*id)?;
        let field = owner.fields.iter().find(|field| field.name == name)?;
        let field_id =
            self.declarations
                .definitions()
                .lookup_child(*id, DefinitionKind::Field, name, 0)?;
        Some((field_id, raise_type(&field.ty)))
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
                HostPathNode::Member { name, .. } => SourcePathStep::Member(name.clone()),
                HostPathNode::Index { argument, .. } => {
                    SourcePathStep::Index(self.infer_expr_type(*argument, env))
                }
            })
            .collect::<Vec<_>>();
        match self.declarations.source_host_path(owner, &source) {
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
                        self.type_table.insert_expr_field(*node, *id);
                    }
                    if let (
                        HostPathNode::Index { argument, .. },
                        HostPathSegmentDeclaration::Index(index),
                    ) = (step, declared)
                    {
                        dynamic_arguments.push((index.slot, *argument));
                    }
                    let ty = raise_type(&result.result);
                    self.type_table.insert_expr(step.id(), ty.clone());
                    env.exprs.insert(step.id(), ty);
                }
                let result = raise_type(&contract.result);
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
                            let Some((field, ty)) =
                                self.resolve_host_declared_field(&current, name)
                            else {
                                recovered_all_members = false;
                                break;
                            };
                            self.type_table.insert_expr_field(*node, field);
                            current = ty;
                        }
                        HostPathNode::Index { .. } => {
                            recovered_all_members = false;
                            if let TypeId::Array(element) = current {
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
                    .with_span(self.lowered.module.expr(expr_id).origin.diagnostic_span()),
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
                HostPathNode::Member { name, .. } => SourcePathStep::Member(name.clone()),
                HostPathNode::Index { argument, .. } => {
                    SourcePathStep::Index(self.infer_expr_type(*argument, env))
                }
            })
            .collect::<Vec<_>>();
        match self.declarations.source_host_path(owner, &source) {
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
                        self.type_table.insert_place_field(*node, *id);
                    }
                    if let (
                        HostPathNode::Index { argument, .. },
                        HostPathSegmentDeclaration::Index(index),
                    ) = (step, declared)
                    {
                        dynamic_arguments.push((index.slot, *argument));
                    }
                    self.type_table
                        .insert_place(step.id(), raise_type(&result.result));
                }
                let result = raise_type(&contract.result);
                if declaration.access != PathAccess::ReadWrite {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidHostPath {
                            reason: "host path is read-only".into(),
                        })
                        .with_span(self.lowered.module.place(place_id).origin.diagnostic_span()),
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
                            let Some((field, ty)) =
                                self.resolve_host_declared_field(&current, name)
                            else {
                                recovered_all_members = false;
                                break;
                            };
                            self.type_table.insert_place_field(*node, field);
                            current = ty;
                        }
                        HostPathNode::Index { .. } => {
                            recovered_all_members = false;
                            if let TypeId::Array(element) = current {
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
                    .with_span(self.lowered.module.place(place_id).origin.diagnostic_span()),
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
