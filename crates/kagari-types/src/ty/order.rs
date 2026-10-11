//! Canonical semantic ordering, independent of definition-table allocation order.
use crate::ty::{NominalTy, Ty, substitution::TypeTransformError};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use std::cmp::Ordering;

impl Ty {
    /// Compare types using exact definition content rather than numeric handles.
    ///
    /// Both inputs must fit the type depth/node limits and refer exclusively to
    /// the supplied retained table. Invalid references, excess size and
    /// cancellation are errors, including when an early field differs.
    pub fn canonical_cmp(
        &self,
        other: &Self,
        table: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Ordering, TypeTransformError> {
        for ty in [self, other] {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if !ty.within_wire_limits() {
                return Err(TypeTransformError::LimitExceeded);
            }
            ty.visit_definition_ids(&mut |id| Ok(table.resolve(*id).map(|_| ())?), cancel)
                .map_err(|error| match error {
                    DefinitionMappingError::Cancelled => TypeTransformError::Cancelled,
                    _ => TypeTransformError::InvalidContract,
                })?;
        }
        // The bounded walks above validate the entire input, so recursive
        // comparison cannot exceed the same depth or encounter an invalid ID.
        let order = TypeOrder { table }.ty(self, other);
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        Ok(order)
    }
}

struct TypeOrder<'a> {
    table: &'a DefinitionTable,
}

impl TypeOrder<'_> {
    fn definition(&self, left: DefinitionId, right: DefinitionId) -> Ordering {
        self.table
            .compare(left, right)
            .expect("validated type scope")
    }

    fn sequence(&self, left: &[Ty], right: &[Ty]) -> Ordering {
        left.iter()
            .zip(right)
            .map(|(left, right)| self.ty(left, right))
            .find(|order| !order.is_eq())
            .unwrap_or_else(|| left.len().cmp(&right.len()))
    }

    fn nominal(&self, left: &NominalTy, right: &NominalTy) -> Ordering {
        self.definition(left.declaration, right.declaration)
            .then_with(|| self.sequence(&left.arguments, &right.arguments))
            .then_with(|| {
                let mut left: Vec<_> = left.associated_types.iter().collect();
                let mut right: Vec<_> = right.associated_types.iter().collect();
                for entries in [&mut left, &mut right] {
                    entries.sort_by(|(left, _), (right, _)| self.definition(**left, **right));
                }
                left.iter()
                    .zip(&right)
                    .map(|((lk, lv), (rk, rv))| {
                        self.definition(**lk, **rk).then_with(|| self.ty(lv, rv))
                    })
                    .find(|order| !order.is_eq())
                    .unwrap_or_else(|| left.len().cmp(&right.len()))
            })
    }

    fn ty(&self, left: &Ty, right: &Ty) -> Ordering {
        rank(left)
            .cmp(&rank(right))
            .then_with(|| match (left, right) {
                (
                    Ty::Projection {
                        arguments: la,
                        receiver: lr,
                        interface: li,
                        member: lm,
                    },
                    Ty::Projection {
                        arguments: ra,
                        receiver: rr,
                        interface: ri,
                        member: rm,
                    },
                ) => self
                    .sequence(la, ra)
                    .then_with(|| self.ty(lr, rr))
                    .then_with(|| self.nominal(li, ri))
                    .then_with(|| self.definition(*lm, *rm)),
                (Ty::Host(left), Ty::Host(right)) | (Ty::SelfType(left), Ty::SelfType(right)) => {
                    self.definition(*left, *right)
                }
                (
                    Ty::Parameter {
                        owner: lo,
                        position: lp,
                    },
                    Ty::Parameter {
                        owner: ro,
                        position: rp,
                    },
                ) => self.definition(*lo, *ro).then_with(|| lp.cmp(rp)),
                (Ty::Builtin(left), Ty::Builtin(right)) => left.cmp(right),
                (Ty::Tuple(left), Ty::Tuple(right)) => self.sequence(left, right),
                (
                    Ty::Function {
                        params: lp,
                        result: lr,
                    },
                    Ty::Function {
                        params: rp,
                        result: rr,
                    },
                ) => self.sequence(lp, rp).then_with(|| self.ty(lr, rr)),
                (Ty::Iter(left), Ty::Iter(right)) | (Ty::Array(left), Ty::Array(right)) => {
                    self.ty(left, right)
                }
                (Ty::Range(left, lk), Ty::Range(right, rk)) => {
                    self.ty(left, right).then_with(|| lk.cmp(rk))
                }
                (
                    Ty::Map {
                        key: lk,
                        value: lv,
                        access: la,
                    },
                    Ty::Map {
                        key: rk,
                        value: rv,
                        access: ra,
                    },
                ) => self
                    .ty(lk, rk)
                    .then_with(|| self.ty(lv, rv))
                    .then_with(|| la.cmp(ra)),
                (Ty::Set(left, la), Ty::Set(right, ra)) => {
                    self.ty(left, right).then_with(|| la.cmp(ra))
                }
                (Ty::Struct(left), Ty::Struct(right))
                | (Ty::NativeObject(left), Ty::NativeObject(right))
                | (Ty::Enum(left), Ty::Enum(right))
                | (Ty::Trait(left), Ty::Trait(right)) => self.nominal(left, right),
                _ => unreachable!("equal type ranks have the same variant"),
            })
    }
}

// Preserve the public semantic order of type variants and fields. Numeric ID
// ordering is suitable for equality-keyed storage, not canonical contracts.
fn rank(ty: &Ty) -> u8 {
    match ty {
        Ty::Projection { .. } => 0,
        Ty::Host(_) => 1,
        Ty::SelfType(_) => 2,
        Ty::Parameter { .. } => 3,
        Ty::Builtin(_) => 4,
        Ty::Tuple(_) => 5,
        Ty::Function { .. } => 6,
        Ty::Iter(_) => 7,
        Ty::Range(..) => 8,
        Ty::Array(_) => 9,
        Ty::Map { .. } => 10,
        Ty::Set(..) => 11,
        Ty::Struct(_) => 12,
        Ty::NativeObject(_) => 13,
        Ty::Enum(_) => 14,
        Ty::Trait(_) => 15,
    }
}
