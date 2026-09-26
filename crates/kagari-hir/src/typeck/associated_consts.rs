//! Scalar associated constants share the ordinary const-safe evaluator.
use super::{
    TypeTable,
    ty::{TypeContext, resolve_type_in},
};
use crate::{
    aggregates::AggregateCatalog,
    declarations::Declarations,
    lower::LoweredModule,
    types::{BuiltinType, TypeId, associated_const_id},
};
use kagari_common::{Diagnostic, DiagnosticKind, Span, cancellation::CancellationToken};
use std::collections::HashSet;

fn error(name: &str, reason: &str, span: Span) -> Diagnostic {
    Diagnostic::error(DiagnosticKind::InvalidAssociatedConst {
        name: name.into(),
        reason: reason.into(),
    })
    .with_span(span)
}

pub(crate) fn scalar_type(ty: &TypeId) -> bool {
    matches!(
        ty,
        TypeId::Builtin(
            BuiltinType::Unit | BuiltinType::Bool | BuiltinType::I32 | BuiltinType::F32
        )
    )
}

pub(super) fn prepare(
    lowered: &LoweredModule,
    declarations: &Declarations,
    table: &mut TypeTable,
    diagnostics: &mut crate::DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let owners = lowered
        .module
        .traits
        .iter()
        .map(|item| {
            (
                &item.associated_consts,
                &item.generic_params,
                Some(item.id),
                None,
            )
        })
        .chain(lowered.module.impls.iter().map(|item| {
            (
                &item.associated_consts,
                &item.generic_params,
                None,
                Some(item.id),
            )
        }));
    for (members, generics, self_type, implementation) in owners {
        let mut names = HashSet::new();
        for member in members {
            if cancel.check().is_err() {
                return;
            }
            let span = lowered.source_map.type_span(member.name_ref);
            if !names.insert(&member.name) {
                diagnostics.push(error(&member.name, "duplicate declaration", span));
            }
            let ty = resolve_type_in(
                &lowered.module,
                member.ty,
                TypeContext {
                    declarations,
                    generics,
                    self_type,
                    implementation,
                },
                table,
                cancel,
            );
            if !scalar_type(&ty) {
                diagnostics.push(error(
                    &member.name,
                    "requires a v1 const-safe scalar type",
                    span,
                ));
            }
            if implementation.is_some() && member.initializer.is_none() {
                diagnostics.push(error(
                    &member.name,
                    "impl constants require an initializer",
                    span,
                ));
            }
        }
    }
}

pub(crate) fn validate(
    lowered: &LoweredModule,
    declarations: &Declarations,
    catalog: &AggregateCatalog,
    table: &TypeTable,
    diagnostics: &mut crate::DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    for item in &lowered.module.impls {
        if cancel.check().is_err() {
            return;
        }
        let contract = declarations
            .impl_identity(item.id)
            .and_then(|id| catalog.implementation_signature(id))
            .and_then(|signature| catalog.trait_(&signature.trait_type.declaration));
        let mut defined = HashSet::new();
        for member in &item.associated_consts {
            let id = contract.map(|owner| associated_const_id(&owner.id, &member.name));
            let expected = id
                .as_ref()
                .and_then(|id| contract?.associated_consts.get(id));
            let span = lowered.source_map.type_span(member.name_ref);
            let Some(expected) = expected else {
                diagnostics.push(error(
                    &member.name,
                    "expected a constant declared by the implemented trait",
                    span,
                ));
                continue;
            };
            defined.insert(id.expect("constant identity"));
            if table
                .type_ref(member.ty)
                .is_none_or(|ty| ty.ty != expected.ty)
            {
                diagnostics.push(error(
                    &member.name,
                    "type differs from the trait declaration",
                    span,
                ));
            }
        }
        if let Some(contract) = contract {
            for (id, member) in &contract.associated_consts {
                if member.initializer.is_none() && !defined.contains(id) {
                    diagnostics.push(error(
                        &id.path.last().expect("member").name,
                        "missing definition in trait implementation",
                        lowered.source_map.impl_span(item.id),
                    ));
                }
            }
        }
    }
}
