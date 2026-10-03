//! Immutable integer range values; cursor state belongs to each iterator.

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::GcHeap,
    numeric,
    value::{EnumTag, Value},
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};

use kagari_abi::{operations, scalar::BuiltinType, types::AbiType};
use kagari_common::{integer, range::RangeKind};
use std::ops::Bound;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RangeValue {
    pub(crate) kind: RangeKind,
    pub(crate) item: BuiltinType,
    // Presence is encoded by kind; signed endpoints retain their two's-complement bits.
    start: u64,
    end: u64,
}

fn invalid() -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid range value")
}

impl RangeValue {
    pub fn new(
        ty: &AbiType<DefinitionId>,
        start: Option<&Value>,
        end: Option<&Value>,
    ) -> Result<Self, RuntimeError> {
        let AbiType::Range(element, kind) = ty else {
            return Err(invalid());
        };
        let AbiType::Builtin(item) = element.as_ref() else {
            return Err(invalid());
        };
        if kind.has_start() != start.is_some()
            || kind.has_end() != end.is_some()
            || (*kind == RangeKind::Full && *item != BuiltinType::Unit)
            || (*kind != RangeKind::Full && item.integer_layout().is_none())
        {
            return Err(invalid());
        }
        Ok(Self {
            kind: *kind,
            item: *item,
            start: start
                .map(|v| numeric::read_integer(*item, v))
                .transpose()?
                .unwrap_or(0) as u64,
            end: end
                .map(|v| numeric::read_integer(*item, v))
                .transpose()?
                .unwrap_or(0) as u64,
        })
    }

    pub fn bound(
        &self,
        gc: &GcHeap,
        definitions: &DefinitionTable,
        range: &AbiType<DefinitionId>,
        bound: &AbiType<DefinitionId>,
        upper: bool,
    ) -> Result<Value, RuntimeError> {
        if !self.matches(range)
            || !operations::range_bound_valid_in(range, bound, Some(definitions))
        {
            return Err(invalid());
        }
        let value = if upper {
            self.kind.has_end().then(|| self.endpoint(self.end))
        } else {
            self.kind.has_start().then(|| self.endpoint(self.start))
        };

        let tag = match value {
            None => EnumTag::BoundUnbounded,
            Some(_) if upper && !self.kind.inclusive() => EnumTag::BoundExcluded,
            Some(_) => EnumTag::BoundIncluded,
        };
        gc.alloc_enum(
            tag,
            value
                .map(|n| integer_value(self.item, n))
                .into_iter()
                .collect(),
        )
        .map(Value::Enum)
    }

    pub(crate) fn matches(&self, ty: &AbiType<DefinitionId>) -> bool {
        matches!(ty, AbiType::Range(item, kind) if *kind == self.kind && **item == AbiType::Builtin(self.item))
    }

    fn endpoint(&self, bits: u64) -> i128 {
        if self.item.integer_layout().is_some_and(|(_, signed)| signed) {
            bits as i64 as i128
        } else {
            bits as i128
        }
    }

    pub(crate) fn at(&self, offset: u128) -> Result<Option<Value>, RuntimeError> {
        if !self.kind.has_start() {
            return Err(invalid());
        }
        let start = self.endpoint(self.start);
        let offset = i128::try_from(offset).map_err(|_| invalid())?;
        let n = start.checked_add(offset).ok_or_else(invalid)?;
        let end = self.endpoint(self.end);
        if self.kind.has_end() && (n > end || n == end && !self.kind.inclusive()) {
            return Ok(None);
        }
        let (bits, signed) = self.item.integer_layout().ok_or_else(invalid)?;
        let (min, max) = integer::bounds(bits, signed);
        if n < min || n > max {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "integer overflow",
            ));
        }
        Ok(Some(integer_value(self.item, n)))
    }
}

pub(crate) fn integer_value(ty: BuiltinType, n: i128) -> Value {
    match ty {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => Value::I32(n as i32),
        BuiltinType::U64 | BuiltinType::USize => Value::U64(n as u64),
        _ => Value::I64(n as i64),
    }
}

pub fn index_bound(gc: &GcHeap, value: &Value) -> Result<Bound<usize>, RuntimeError> {
    let Value::Enum(id) = value else {
        return Err(invalid());
    };
    let value = gc.enum_snapshot(*id).ok_or_else(invalid)?;
    if value.tag == EnumTag::BoundUnbounded && value.fields.is_empty() {
        return Ok(Bound::Unbounded);
    }
    let [Value::U64(n)] = value.fields.as_slice() else {
        return Err(invalid());
    };
    let n = usize::try_from(*n).map_err(|_| invalid())?;
    match value.tag {
        EnumTag::BoundIncluded => Ok(Bound::Included(n)),
        EnumTag::BoundExcluded => Ok(Bound::Excluded(n)),
        _ => Err(invalid()),
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
                Value::I32(-128),
                Value::I32(-126),
                vec![Value::I32(-128), Value::I32(-127), Value::I32(-126)],
            ),
            (
                BuiltinType::U64,
                Value::U64(u64::MAX - 1),
                Value::U64(u64::MAX),
                vec![Value::U64(u64::MAX - 1), Value::U64(u64::MAX)],
            ),
        ] {
            let ty = AbiType::Range(Box::new(AbiType::Builtin(item)), RangeKind::Inclusive);
            let range = RangeValue::new(&ty, Some(&start), Some(&end)).unwrap();
            for (offset, value) in expected.iter().enumerate() {
                assert_eq!(range.at(offset as u128).unwrap().as_ref(), Some(value));
            }
            assert_eq!(range.at(expected.len() as u128).unwrap(), None);
            assert!(RangeValue::new(&ty, None, Some(&end)).is_err());
        }
        let ty = AbiType::Range(
            Box::new(AbiType::Builtin(BuiltinType::I64)),
            RangeKind::Inclusive,
        );
        let range = RangeValue::new(
            &ty,
            Some(&Value::I64(i64::MIN)),
            Some(&Value::I64(i64::MAX)),
        )
        .unwrap();
        assert_eq!(
            range.at(u64::MAX as u128).unwrap(),
            Some(Value::I64(i64::MAX))
        );
        assert_eq!(range.at(u64::MAX as u128 + 1).unwrap(), None);
        let ty = AbiType::Range(Box::new(AbiType::Builtin(BuiltinType::U8)), RangeKind::From);
        let range = RangeValue::new(&ty, Some(&Value::I64(255)), None).unwrap();
        assert_eq!(range.at(0).unwrap(), Some(Value::I64(255)));
        assert!(range.at(1).is_err());
        assert!(RangeValue::new(&ty, Some(&Value::I64(256)), None).is_err());
    }
}
