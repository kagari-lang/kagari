use std::collections::HashMap;

use super::ScalarValue;
use crate::builtin::surface::StandardIntrinsic;
use crate::hir::{ExprId, LocalId, PatternId, PlaceId};
use crate::types::TypeId;

#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    exprs: HashMap<ExprId, TypeId>,
    locals: HashMap<LocalId, TypeId>,
    places: HashMap<PlaceId, TypeId>,
    standard_calls: HashMap<ExprId, StandardIntrinsic>,
    scalars: HashMap<ExprId, ScalarValue>,
    pattern_scalars: HashMap<PatternId, ScalarValue>,
}

impl TypeTable {
    pub(crate) fn restore_function(
        &mut self,
        old: &Self,
        old_map: &crate::source_map::SourceMap,
        new_map: &crate::source_map::SourceMap,
        old_span: kagari_common::Span,
        new_span: kagari_common::Span,
    ) -> bool {
        fn remap(
            old: &[kagari_common::Span],
            new: &[kagari_common::Span],
            old_span: kagari_common::Span,
            new_span: kagari_common::Span,
        ) -> Option<Vec<(usize, usize)>> {
            let relative = |spans: &[kagari_common::Span], owner: kagari_common::Span| {
                spans
                    .iter()
                    .enumerate()
                    .filter(|(_, span)| span.start >= owner.start && span.end <= owner.end)
                    .map(|(id, span)| (id, span.start - owner.start, span.end - owner.start))
                    .collect::<Vec<_>>()
            };
            let old = relative(old, old_span);
            let new = relative(new, new_span);
            if old.len() != new.len() {
                return None;
            }
            old.into_iter()
                .zip(new)
                .map(|((a, start, end), (b, ns, ne))| (start == ns && end == ne).then_some((a, b)))
                .collect()
        }
        let Some(exprs) = remap(
            old_map.expr_spans(),
            new_map.expr_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        let Some(locals) = remap(
            old_map.local_spans(),
            new_map.local_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        let Some(places) = remap(
            old_map.place_spans(),
            new_map.place_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        let Some(patterns) = remap(
            old_map.pattern_spans(),
            new_map.pattern_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        for (a, b) in patterns {
            if let Some(value) = old.pattern_scalars.get(&PatternId::new(a)) {
                self.pattern_scalars
                    .insert(PatternId::new(b), value.clone());
            }
        }
        for (a, b) in exprs {
            if let Some(value) = old.scalars.get(&ExprId::new(a)) {
                self.scalars.insert(ExprId::new(b), value.clone());
            }
            if let Some(ty) = old.exprs.get(&ExprId::new(a)) {
                self.exprs.insert(ExprId::new(b), ty.clone());
            }
            if let Some(intrinsic) = old.standard_calls.get(&ExprId::new(a)) {
                self.standard_calls.insert(ExprId::new(b), *intrinsic);
            }
        }
        for (a, b) in locals {
            if let Some(ty) = old.locals.get(&LocalId::new(a)) {
                self.locals.insert(LocalId::new(b), ty.clone());
            }
        }
        for (a, b) in places {
            if let Some(ty) = old.places.get(&PlaceId::new(a)) {
                self.places.insert(PlaceId::new(b), ty.clone());
            }
        }
        true
    }
    pub(crate) fn insert_expr(&mut self, id: ExprId, ty: TypeId) {
        self.exprs.insert(id, ty);
    }

    pub(crate) fn insert_scalar(&mut self, id: ExprId, value: ScalarValue) {
        self.scalars.insert(id, value);
    }
    pub(crate) fn insert_pattern_scalar(&mut self, id: PatternId, value: ScalarValue) {
        self.pattern_scalars.insert(id, value);
    }
    pub fn scalar_value(&self, id: ExprId) -> Option<&ScalarValue> {
        self.scalars.get(&id)
    }
    pub fn pattern_scalar_value(&self, id: PatternId) -> Option<&ScalarValue> {
        self.pattern_scalars.get(&id)
    }

    pub(crate) fn insert_local(&mut self, id: LocalId, ty: TypeId) {
        self.locals.insert(id, ty);
    }

    pub(crate) fn insert_place(&mut self, id: PlaceId, ty: TypeId) {
        self.places.insert(id, ty);
    }

    pub(crate) fn insert_standard_call(&mut self, id: ExprId, intrinsic: StandardIntrinsic) {
        self.standard_calls.insert(id, intrinsic);
    }

    pub fn expr_type(&self, id: ExprId) -> Option<TypeId> {
        self.exprs.get(&id).cloned()
    }

    pub fn local_type(&self, id: LocalId) -> Option<TypeId> {
        self.locals.get(&id).cloned()
    }

    pub fn place_type(&self, id: PlaceId) -> Option<TypeId> {
        self.places.get(&id).cloned()
    }

    pub fn standard_call_intrinsic(&self, id: ExprId) -> Option<StandardIntrinsic> {
        self.standard_calls.get(&id).copied()
    }
}
