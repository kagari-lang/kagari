//! Collapse consecutive checked Host fields into a single adapter operation. A
//! script projection (including an index producing a Host) remains part of the root.

use super::Builder;
use crate::source::elaborate::{ElaborationError, model::HostFields};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_hir::{
    hir::{
        expr::ExprKind,
        ids::{ExprId, PlaceId},
        place::PlaceKind,
    },
    types::TypeId,
};

impl Builder<'_, '_> {
    pub(super) fn host_read(
        &self,
        id: ExprId,
        span: Span,
    ) -> Result<Option<HostFields<ExprId>>, ElaborationError> {
        self.host_fields(
            id,
            span,
            |node| match self.input.expression(node)?.kind {
                ExprKind::Field { receiver, .. } => Some(receiver),
                _ => None,
            },
            |node| {
                self.input
                    .expression_facts(node)
                    .map(|f| (f.field.copied(), f.ty))
            },
        )
    }

    pub(super) fn host_place(
        &self,
        id: PlaceId,
        span: Span,
    ) -> Result<Option<HostFields<PlaceId>>, ElaborationError> {
        self.host_fields(
            id,
            span,
            |node| match self.input.place(node)?.kind {
                PlaceKind::Field { base, .. } => Some(base),
                _ => None,
            },
            |node| {
                self.input
                    .place_facts(node)
                    .map(|f| (f.field.copied(), f.ty))
            },
        )
    }

    fn host_fields<N: Copy>(
        &self,
        mut node: N,
        span: Span,
        base: impl Fn(N) -> Option<N>,
        facts: impl Fn(N) -> Option<(Option<DefinitionId>, Option<TypeId>)>,
    ) -> Result<Option<HostFields<N>>, ElaborationError> {
        let mut owner = None;
        let mut fields = Vec::new();
        while let Some(receiver) = base(node) {
            self.check()?;
            let Some(TypeId::Host(id)) = facts(receiver).and_then(|(_, ty)| ty) else {
                break;
            };
            let Some((Some(field), Some(ty))) = facts(node) else {
                return Err(self.error(Some(span), "missing checked Host field selection"));
            };
            fields.push((field, self.valid_type(ty, Some(span))?));
            owner = Some(id);
            node = receiver;
        }
        Ok(owner.map(|owner| {
            fields.reverse();
            HostFields {
                root: node,
                owner,
                fields,
            }
        }))
    }
}
