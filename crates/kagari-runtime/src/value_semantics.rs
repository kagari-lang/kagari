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
            a.tag == b.tag && members_equal(gc, &a.fields, &b.fields)?
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
    fn declared_enum_equality_keeps_nominal_identity_across_private_layout_edits() {
        use kagari_common::identity::{
            DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
        };
        use kagari_ir::{
            bytecode::{BytecodeModule, BytecodeProgram, ModuleRef},
            module::{
                EnumLayout, EnumVariantLayout,
                abi::{AbiType, BuiltinType},
            },
        };

        let identity = ModuleIdentity::single_file("enum-equality.kgr");
        let declaration = DefinitionId {
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
            declaration: DefinitionId {
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
                            declaration: DefinitionId {
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
        let old_tag = crate::value::EnumTag::Declared(
            old.enum_variant(kagari_ir::bytecode::EnumId::new(0), 0)
                .unwrap(),
        );
        let old_value = Value::Enum(runtime.alloc_enum(old_tag, vec![Value::I32(7)]).unwrap());
        let candidate = runtime
            .stage_reload_program(&old, "enum-equality", program(true))
            .unwrap();
        let new = runtime.publish_staged_reload(candidate).unwrap();
        let new_tag = crate::value::EnumTag::Declared(
            new.enum_variant(kagari_ir::bytecode::EnumId::new(0), 1)
                .unwrap(),
        );
        let new_value = Value::Enum(runtime.alloc_enum(new_tag, vec![Value::I32(7)]).unwrap());

        assert!(script_equal(runtime.gc(), &old_value, &new_value).unwrap());
        let other_tag = crate::value::EnumTag::Declared(
            new.enum_variant(kagari_ir::bytecode::EnumId::new(1), 0)
                .unwrap(),
        );
        let other_value = Value::Enum(runtime.alloc_enum(other_tag, vec![Value::I32(7)]).unwrap());
        assert!(!script_equal(runtime.gc(), &old_value, &other_value).unwrap());
    }

    #[test]
    fn enum_members_use_script_semantics_including_identity_and_nan() {
        let mut runtime = crate::Runtime::default();
        let interface = crate::layout_fixtures::interface_value(&mut runtime);
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
        let array = Value::Array(gc.alloc_array(vec![Value::I32(3)]).unwrap());
        assert!(script_equal(gc, &make(array.clone()), &make(array)).unwrap());
        let first = make(Value::Array(gc.alloc_array(vec![Value::I32(3)]).unwrap()));
        let second = make(Value::Array(gc.alloc_array(vec![Value::I32(3)]).unwrap()));
        assert!(!script_equal(gc, &first, &second).unwrap());
        let nan = make(Value::F64(f64::NAN));
        assert!(!script_equal(gc, &nan, &nan).unwrap());
        assert!(script_equal(gc, &interface, &interface).is_err());
    }
}
