//! Immutable integer range values; cursor state belongs to each iterator.

use crate::value::{
    carriers::{ManagedRepresentation, ScopedValue},
    kind::ObjectKind,
    scalar,
};

use kagari_common::identity::table::DefinitionTable;
use kagari_contract::operations;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::{
    range::RangeKind,
    scalar::{BuiltinType, error::ScalarError},
    ty::Ty,
};
use std::ops::Bound;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RangeValue {
    pub(crate) kind: RangeKind,
    pub(crate) item: BuiltinType,
    // Presence is encoded by kind; signed endpoints retain their two's-complement bits.
    start: u64,
    end: u64,
}

/// Physical range admission keeps shape failures distinct from scalar-domain
/// failures. The Runtime boundary attaches its existing typed reason/context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RangeError {
    InvalidShape,
    Endpoint(ScalarError),
}

impl RangeValue {
    pub(crate) fn new(
        ty: &Ty,
        start: Option<ScopedValue<'_>>,
        end: Option<ScopedValue<'_>>,
    ) -> Result<Self, RangeError> {
        let Ty::Range(element, kind) = ty else {
            return Err(RangeError::InvalidShape);
        };
        let Ty::Builtin(item) = element.as_ref() else {
            return Err(RangeError::InvalidShape);
        };
        if kind.has_start() != start.is_some()
            || kind.has_end() != end.is_some()
            || (*kind == RangeKind::Full && *item != BuiltinType::Unit)
            || (*kind != RangeKind::Full && item.integer_layout().is_none())
        {
            return Err(RangeError::InvalidShape);
        }
        Ok(Self {
            kind: *kind,
            item: *item,
            start: start
                .map(|v| scalar::read_integer(*item, v).map_err(RangeError::Endpoint))
                .transpose()?
                .unwrap_or(0) as u64,
            end: end
                .map(|v| scalar::read_integer(*item, v).map_err(RangeError::Endpoint))
                .transpose()?
                .unwrap_or(0) as u64,
        })
    }

    pub(crate) fn bound<'gc>(
        &self,
        definitions: &DefinitionTable,
        range: &Ty,
        bound: &Ty,
        upper: bool,
    ) -> Result<Bound<ScopedValue<'gc>>, RangeError> {
        if !self.matches(range) || !operations::range_bound_valid(range, bound, definitions) {
            return Err(RangeError::InvalidShape);
        }
        let value = if upper {
            self.kind.has_end().then(|| self.endpoint(self.end))
        } else {
            self.kind.has_start().then(|| self.endpoint(self.start))
        };

        Ok(match value.map(|n| integer_value(self.item, n)) {
            None => Bound::Unbounded,
            Some(value) if upper && !self.kind.inclusive() => Bound::Excluded(value),
            Some(value) => Bound::Included(value),
        })
    }

    pub(crate) fn matches(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Range(item, kind) if *kind == self.kind && **item == Ty::Builtin(self.item))
    }

    fn endpoint(&self, bits: u64) -> i128 {
        if self.item.integer_layout().is_some_and(|(_, signed)| signed) {
            bits as i64 as i128
        } else {
            bits as i128
        }
    }
}

fn integer_value<'gc>(ty: BuiltinType, n: i128) -> ScopedValue<'gc> {
    match ty {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => ScopedValue::from(n as i32),
        BuiltinType::U64 => ScopedValue::from(n as u64),
        _ => ScopedValue::from(n as i64),
    }
}

impl RangeValue {
    /// Immutable scalar endpoints have no managed edges or indirect backing.
    pub(crate) fn alloc<'gc>(self, cx: &mut Scope<'gc>) -> Result<Gc<'gc, Self>, GcError> {
        cx.alloc_leaf(self)
    }
}

impl ManagedRepresentation for RangeValue {
    const KIND: ObjectKind = ObjectKind::Range;
}

// SAFETY: immutable validated integer bits, no managed edges, indirect backing or
// peer-aware Drop. Iterator cursor state belongs to its separate consuming object.
unsafe impl GcObject for RangeValue {
    type ScanState = ();

    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_endpoints_preserve_full_domain_and_validate_shapes() {
        // Range storage must not enlarge every VM value to hold optional i128s.
        assert!(std::mem::size_of::<RangeValue>() <= 24);
        for (item, start, end, expected) in [
            (
                BuiltinType::I8,
                ScopedValue::from(-128i32),
                ScopedValue::from(-126i32),
                vec![
                    ScopedValue::from(-128i32),
                    ScopedValue::from(-127i32),
                    ScopedValue::from(-126i32),
                ],
            ),
            (
                BuiltinType::U64,
                ScopedValue::from(u64::MAX - 1),
                ScopedValue::from(u64::MAX),
                vec![ScopedValue::from(u64::MAX - 1), ScopedValue::from(u64::MAX)],
            ),
        ] {
            let ty = Ty::Range(Box::new(Ty::Builtin(item)), RangeKind::Inclusive);
            let range = RangeValue::new(&ty, Some(start), Some(end)).unwrap();
            assert!(range.matches(&ty));
            assert_eq!(
                integer_value(range.item, range.endpoint(range.start)).scalar_payload(),
                expected.first().unwrap().scalar_payload()
            );
            assert_eq!(
                integer_value(range.item, range.endpoint(range.end)).scalar_payload(),
                expected.last().unwrap().scalar_payload()
            );
            assert_eq!(
                RangeValue::new(&ty, None, Some(end)),
                Err(RangeError::InvalidShape)
            );
        }
        let ty = Ty::Range(
            Box::new(Ty::Builtin(BuiltinType::I64)),
            RangeKind::Inclusive,
        );
        let range = RangeValue::new(
            &ty,
            Some(ScopedValue::from(i64::MIN)),
            Some(ScopedValue::from(i64::MAX)),
        )
        .unwrap();
        assert_eq!(range.endpoint(range.start), i64::MIN as i128);
        assert_eq!(range.endpoint(range.end), i64::MAX as i128);
        let ty = Ty::Range(Box::new(Ty::Builtin(BuiltinType::U8)), RangeKind::From);
        let range = RangeValue::new(&ty, Some(ScopedValue::from(255i64)), None).unwrap();
        assert_eq!(range.endpoint(range.start), 255);
        assert_eq!(
            RangeValue::new(&ty, Some(ScopedValue::from(256i64)), None),
            Err(RangeError::Endpoint(ScalarError::InvalidOperandDomain))
        );
    }
}
