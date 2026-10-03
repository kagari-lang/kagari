//! Script equality is independent of Rust's structural Value comparisons.

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, GcObjectKind},
    value::{EnumTag, Value},
};

use kagari_abi::types as abi;
use std::{
    cmp::Ordering,
    fmt::{self, Error, Write},
};

/// Collection interface boxes preserve the identity of their underlying object.
pub(crate) fn collection_data(gc: &GcHeap, value: &Value) -> Option<Value> {
    let Value::Interface(id) = value else {
        return None;
    };
    let snapshot = gc.interface_snapshot(*id)?;
    abi::is_collection_interface(&snapshot.interface_type.declaration)
        .then(|| snapshot.data.clone())
}

/// Identity is available only for script object categories, never allocation
/// details of values such as strings, tuples or enums.
pub fn identity_equal(gc: &GcHeap, lhs: &Value, rhs: &Value) -> Result<bool, RuntimeError> {
    let left = collection_data(gc, lhs);
    let right = collection_data(gc, rhs);
    let (lhs, rhs) = (left.as_ref().unwrap_or(lhs), right.as_ref().unwrap_or(rhs));
    let object = |value: &Value| match value {
        Value::Struct(id) => Some((*id, GcObjectKind::Struct)),
        Value::Array(id) => Some((*id, GcObjectKind::Array)),
        Value::Map(id) => Some((*id, GcObjectKind::Map)),
        Value::Set(id) => Some((*id, GcObjectKind::Set)),
        Value::GcHandle(id) if gc.object_kind(*id) == Some(GcObjectKind::Native) => {
            Some((*id, GcObjectKind::Native))
        }
        _ => None,
    };
    let (Some((a, a_kind)), Some((b, b_kind))) = (object(lhs), object(rhs)) else {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "identity comparison requires object references",
        ));
    };
    if gc.object_kind(a) != Some(a_kind) || gc.object_kind(b) != Some(b_kind) {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "invalid heap handle in identity comparison",
        ));
    }
    Ok(a == b)
}

pub fn script_equal(gc: &GcHeap, lhs: &Value, rhs: &Value) -> Result<bool, RuntimeError> {
    let left = collection_data(gc, lhs);
    let right = collection_data(gc, rhs);
    let (lhs, rhs) = (left.as_ref().unwrap_or(lhs), right.as_ref().unwrap_or(rhs));
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
        (
            Value::Interface(_) | Value::HostRoot(_) | Value::HostPathView(_) | Value::Ephemeral(_),
            _,
        )
        | (
            _,
            Value::Interface(_) | Value::HostRoot(_) | Value::HostPathView(_) | Value::Ephemeral(_),
        ) => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "value category does not support general equality",
            ));
        }
        (Value::Unit, Value::Unit) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::I32(a), Value::I32(b)) => a == b,
        (Value::I64(a), Value::I64(b)) => a == b,
        (Value::U64(a), Value::U64(b)) => a == b,
        (Value::F32(a), Value::F32(b)) => a == b,
        (Value::F64(a), Value::F64(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Tuple(a), Value::Tuple(b)) => members_equal(gc, a, b)?,
        (Value::Enum(a), Value::Enum(b)) => {
            let a = gc.enum_snapshot(*a).ok_or_else(invalid)?;
            let b = gc.enum_snapshot(*b).ok_or_else(invalid)?;
            a.tag == b.tag && members_equal(gc, &a.fields, &b.fields)?
        }
        (Value::Array(a), Value::Array(b))
        | (Value::Map(a), Value::Map(b))
        | (Value::Set(a), Value::Set(b))
        | (Value::Struct(a), Value::Struct(b)) => {
            let kind = match lhs {
                Value::Array(_) => GcObjectKind::Array,
                Value::Map(_) => GcObjectKind::Map,
                Value::Set(_) => GcObjectKind::Set,
                _ => GcObjectKind::Struct,
            };
            if gc.object_kind(*a) != Some(kind) || gc.object_kind(*b) != Some(kind) {
                return Err(invalid());
            }
            a == b
        }
        (Value::GcHandle(a), Value::GcHandle(b))
            if gc.object_kind(*a) == Some(GcObjectKind::Native)
                && gc.object_kind(*b) == Some(GcObjectKind::Native) =>
        {
            a == b
        }
        (Value::GcHandle(id), _) | (_, Value::GcHandle(id))
            if gc.object_kind(*id) == Some(GcObjectKind::Native) =>
        {
            false
        }
        (Value::GcHandle(_), _) | (_, Value::GcHandle(_)) => {
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

/// Bounded standard formatting. Mutable objects are identity previews; this
/// never calls script code while traversing the heap.
pub fn format_value(gc: &GcHeap, value: &Value, debug: bool) -> Result<String, RuntimeError> {
    #[derive(Default)]
    struct Output {
        text: String,
        failed: bool,
    }

    impl Output {
        fn push_str(&mut self, text: &str) {
            if text.len() > 1_048_576usize.saturating_sub(self.text.len()) {
                self.failed = true;
            } else if !self.failed {
                self.text.push_str(text);
            }
        }

        fn push(&mut self, ch: char) {
            self.push_str(ch.encode_utf8(&mut [0; 4]));
        }
    }

    impl Write for Output {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.push_str(text);
            if self.failed { Err(Error) } else { Ok(()) }
        }
    }

    fn render(
        gc: &GcHeap,
        value: &Value,
        debug: bool,
        depth: usize,
        out: &mut Output,
    ) -> Option<()> {
        if depth > 64 || out.failed || !gc.validate_value(value) {
            return None;
        }
        match value {
            Value::Unit => out.push_str("()"),
            Value::Bool(v) => write!(out, "{v}").ok()?,
            Value::I32(v) => write!(out, "{v}").ok()?,
            Value::I64(v) => write!(out, "{v}").ok()?,
            Value::U64(v) => write!(out, "{v}").ok()?,
            Value::F32(v) => write!(out, "{v}").ok()?,
            Value::F64(v) => write!(out, "{v}").ok()?,
            Value::Str(v) if debug => write!(out, "{v:?}").ok()?,
            Value::Str(v) => out.push_str(v),
            Value::Tuple(values) if debug => {
                out.push('(');
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    render(gc, value, true, depth + 1, out)?;
                }
                if values.len() == 1 {
                    out.push(',');
                }
                out.push(')');
            }
            Value::Enum(id) if debug => {
                let value = gc.enum_snapshot(*id)?;
                write!(
                    out,
                    "{}::{}",
                    value.tag.type_name(),
                    value.tag.variant_name()
                )
                .ok()?;
                if !value.fields.is_empty() {
                    out.push('(');
                    for (i, value) in value.fields.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        render(gc, value, true, depth + 1, out)?;
                    }
                    out.push(')');
                }
            }
            Value::Struct(id) | Value::Array(id) | Value::Map(id) | Value::Set(id) if debug => {
                let name = match value {
                    Value::Struct(_) => "Struct",
                    Value::Array(_) => "Array",
                    Value::Map(_) => "Map",
                    _ => "Set",
                };
                write!(out, "{name}@{}:{}", id.index(), id.generation()).ok()?;
            }
            Value::HostRoot(_) if debug => out.push_str("<host>"),
            Value::GcHandle(id) if debug => {
                let name = gc.native_type_name(*id)?;
                write!(out, "{name}@{}:{}", id.index(), id.generation()).ok()?;
            }
            Value::Interface(_) if debug => out.push_str("<interface>"),
            Value::Closure(_) if debug => out.push_str("<function>"),
            Value::HostPathView(_) if debug => out.push_str("<host path>"),
            Value::Ephemeral(_) if debug => out.push_str("<borrow>"),
            _ => return None,
        }
        (!out.failed).then_some(())
    }
    let mut result = Output::default();
    render(gc, value, debug, 0, &mut result).ok_or_else(|| {
        RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "unsupported format value, invalid handle, or formatting limit exceeded",
        )
    })?;
    Ok(result.text)
}

/// Builtin ordering leaves user dispatch to the checked call site.
pub fn builtin_order(gc: &GcHeap, a: &Value, b: &Value) -> Result<Option<Ordering>, RuntimeError> {
    let invalid = || {
        RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "invalid builtin ordering operands",
        )
    };
    if !gc.validate_value(a) || !gc.validate_value(b) {
        return Err(invalid());
    }
    Ok(match (a, b) {
        (Value::Unit, Value::Unit) => Some(Ordering::Equal),
        (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
        (Value::I32(a), Value::I32(b)) => a.partial_cmp(b),
        (Value::I64(a), Value::I64(b)) => a.partial_cmp(b),
        (Value::U64(a), Value::U64(b)) => a.partial_cmp(b),
        (Value::F32(a), Value::F32(b)) => a.partial_cmp(b),
        (Value::F64(a), Value::F64(b)) => a.partial_cmp(b),
        (Value::Str(a), Value::Str(b)) => a.partial_cmp(b),
        (Value::Enum(a), Value::Enum(b)) => {
            let rank = |id| match gc.enum_snapshot(id)?.tag {
                EnumTag::OrderingLess => Some(0),
                EnumTag::OrderingEqual => Some(1),
                EnumTag::OrderingGreater => Some(2),
                _ => None,
            };
            Some(
                rank(*a)
                    .ok_or_else(invalid)?
                    .cmp(&rank(*b).ok_or_else(invalid)?),
            )
        }
        _ => return Err(invalid()),
    })
}

#[cfg(test)]
mod tests {
    use kagari_abi::{scalar::BuiltinType, types::AbiType};
    use kagari_bytecode::instruction::EnumId;

    use super::*;

    #[test]
    fn declared_enum_equality_keeps_nominal_identity_across_private_layout_edits() {
        use kagari_abi::{
            layout::{EnumLayout, EnumVariantLayout},
            scalar::BuiltinType,
            types::AbiType,
        };
        use kagari_bytecode::{
            module::BytecodeModule,
            program::{BytecodeProgram, ModuleRef},
        };
        use kagari_common::identity::{
            DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
        };

        let identity = ModuleIdentity::single_file("enum-equality.kgr");
        let declaration = DefinitionPath {
            module: identity.clone(),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Enum,
                name: "Event".into(),
                occurrence: 0,
            }],
        };
        let mut other_declaration = declaration.clone();
        other_declaration.path[0].name = "OtherEvent".into();
        let variant = |name: &str, payload| EnumVariantLayout {
            declaration: DefinitionPath {
                module: identity.clone(),
                path: declaration
                    .path
                    .iter()
                    .cloned()
                    .chain([DefinitionPathSegment {
                        kind: DefinitionKind::Variant,
                        name: name.into(),
                        occurrence: 0,
                    }])
                    .collect(),
            },
            payload,
        };
        let program = |extra_variant: bool| BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                identity: identity.clone(),
                enumerations: vec![
                    EnumLayout {
                        declaration: declaration.clone(),
                        arguments: Vec::new(),
                        variants: if extra_variant {
                            vec![
                                variant("Added", Vec::new()),
                                variant("Data", vec![AbiType::Builtin(BuiltinType::I32)]),
                            ]
                        } else {
                            vec![variant("Data", vec![AbiType::Builtin(BuiltinType::I32)])]
                        },
                    },
                    EnumLayout {
                        declaration: other_declaration.clone(),
                        arguments: Vec::new(),
                        variants: vec![EnumVariantLayout {
                            declaration: DefinitionPath {
                                module: identity.clone(),
                                path: other_declaration
                                    .path
                                    .iter()
                                    .cloned()
                                    .chain([DefinitionPathSegment {
                                        kind: DefinitionKind::Variant,
                                        name: "Data".into(),
                                        occurrence: 0,
                                    }])
                                    .collect(),
                            },
                            payload: vec![AbiType::Builtin(BuiltinType::I32)],
                        }],
                    },
                ],
                ..Default::default()
            }],
        };
        let mut runtime = crate::Runtime::default();
        let old = runtime
            .load_program("enum-equality", program(false))
            .unwrap();
        let old_tag = crate::value::EnumTag::Declared(old.enum_variant(EnumId::new(0), 0).unwrap());
        let old_value = Value::Enum(runtime.alloc_enum(old_tag, vec![Value::I32(7)]).unwrap());
        let candidate = runtime
            .stage_reload_program(&old, "enum-equality", program(true))
            .unwrap();
        let new = runtime.publish_staged_reload(candidate).unwrap();
        let new_tag = crate::value::EnumTag::Declared(new.enum_variant(EnumId::new(0), 1).unwrap());
        let new_value = Value::Enum(runtime.alloc_enum(new_tag, vec![Value::I32(7)]).unwrap());

        assert!(script_equal(runtime.gc(), &old_value, &new_value).unwrap());
        let other_tag =
            crate::value::EnumTag::Declared(new.enum_variant(EnumId::new(1), 0).unwrap());
        let other_value = Value::Enum(runtime.alloc_enum(other_tag, vec![Value::I32(7)]).unwrap());
        assert!(!script_equal(runtime.gc(), &old_value, &other_value).unwrap());
    }

    #[test]
    fn enum_members_use_script_semantics_including_identity_and_nan() {
        let mut runtime = crate::Runtime::default();
        let interface = crate::layout_fixtures::interface_value(&mut runtime);
        let owner = crate::layout_fixtures::allocation_owner(&mut runtime);
        let gc = runtime.gc();
        let make = |value| {
            Value::Enum(
                gc.alloc_enum(crate::value::EnumTag::OptionSome, vec![value])
                    .unwrap(),
            )
        };
        let a = make(Value::I32(3));
        let b = make(Value::I32(3));
        assert_ne!(
            a, b,
            "Rust equality is deliberately not the script operation"
        );
        assert!(script_equal(gc, &a, &b).unwrap());
        let array = Value::Array(
            runtime
                .alloc_array(
                    &owner,
                    AbiType::Builtin(BuiltinType::I32),
                    vec![Value::I32(3)],
                )
                .unwrap(),
        );
        assert!(script_equal(gc, &make(array.clone()), &make(array)).unwrap());
        let first = make(Value::Array(
            runtime
                .alloc_array(
                    &owner,
                    AbiType::Builtin(BuiltinType::I32),
                    vec![Value::I32(3)],
                )
                .unwrap(),
        ));
        let second = make(Value::Array(
            runtime
                .alloc_array(
                    &owner,
                    AbiType::Builtin(BuiltinType::I32),
                    vec![Value::I32(3)],
                )
                .unwrap(),
        ));
        assert!(!script_equal(gc, &first, &second).unwrap());
        let nan = make(Value::F64(f64::NAN));
        assert!(!script_equal(gc, &nan, &nan).unwrap());
        assert!(script_equal(gc, &interface, &interface).is_err());
    }
}
