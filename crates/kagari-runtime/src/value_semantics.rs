//! Script equality is independent of Rust's structural Value comparisons.
use crate::{
    RuntimeError, RuntimeErrorKind,
    gc::{GcHeap, GcObjectKind},
    value::Value,
};

pub fn script_equal(gc: &GcHeap, lhs: &Value, rhs: &Value) -> Result<bool, RuntimeError> {
    use Value::*;
    let invalid = || {
        RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "invalid heap handle in equality",
        )
    };
    if !gc.validate_value(lhs) || !gc.validate_value(rhs) {
        return Err(invalid());
    }
    Ok(match (lhs, rhs) {
        (Interface(_) | HostRoot(_) | HostPathView(_) | Ephemeral(_), _)
        | (_, Interface(_) | HostRoot(_) | HostPathView(_) | Ephemeral(_)) => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "value category does not support general equality",
            ));
        }
        (Unit, Unit) => true,
        (Bool(a), Bool(b)) => a == b,
        (I32(a), I32(b)) => a == b,
        (I64(a), I64(b)) => a == b,
        (F32(a), F32(b)) => a == b,
        (F64(a), F64(b)) => a == b,
        (Str(a), Str(b)) => a == b,
        (Tuple(a), Tuple(b)) => members_equal(gc, a, b)?,
        (Enum(a), Enum(b)) => {
            let a = gc.enum_snapshot(*a).ok_or_else(invalid)?;
            let b = gc.enum_snapshot(*b).ok_or_else(invalid)?;
            a.name == b.name && a.variant == b.variant && members_equal(gc, &a.fields, &b.fields)?
        }
        (Array(a), Array(b)) | (Map(a), Map(b)) | (Set(a), Set(b)) | (Struct(a), Struct(b)) => {
            let kind = match lhs {
                Array(_) => GcObjectKind::Array,
                Map(_) => GcObjectKind::Map,
                Set(_) => GcObjectKind::Set,
                _ => GcObjectKind::Struct,
            };
            if gc.object_kind(*a) != Some(kind) || gc.object_kind(*b) != Some(kind) {
                return Err(invalid());
            }
            a == b
        }
        (GcHandle(_), _) | (_, GcHandle(_)) => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "untyped GC handle does not support general equality",
            ));
        }
        _ => false,
    })
}

fn members_equal(gc: &GcHeap, lhs: &[Value], rhs: &[Value]) -> Result<bool, RuntimeError> {
    if lhs.len() != rhs.len() {
        return Ok(false);
    }
    for (lhs, rhs) in lhs.iter().zip(rhs) {
        if !script_equal(gc, lhs, rhs)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_members_use_script_semantics_including_identity_and_nan() {
        let gc = GcHeap::new(Default::default());
        let make = |value| {
            Value::Enum(
                gc.alloc_enum("Option".into(), "Some".into(), vec![value])
                    .unwrap(),
            )
        };
        let a = make(Value::I32(3));
        let b = make(Value::I32(3));
        assert_ne!(
            a, b,
            "Rust equality is deliberately not the script operation"
        );
        assert!(script_equal(&gc, &a, &b).unwrap());
        let array = Value::Array(gc.alloc_array(vec![Value::I32(3)]).unwrap());
        assert!(script_equal(&gc, &make(array.clone()), &make(array)).unwrap());
        let first = make(Value::Array(gc.alloc_array(vec![Value::I32(3)]).unwrap()));
        let second = make(Value::Array(gc.alloc_array(vec![Value::I32(3)]).unwrap()));
        assert!(!script_equal(&gc, &first, &second).unwrap());
        let nan = make(Value::F64(f64::NAN));
        assert!(!script_equal(&gc, &nan, &nan).unwrap());
        let interface = Value::Interface(crate::value::InterfaceObjectId(0));
        assert!(script_equal(&gc, &interface, &interface).is_err());
    }
}
