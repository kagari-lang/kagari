use super::*;

#[test]
fn infers_array_method_call_types() {
    let lowered = common::lower_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    val next = values.push(3);
    next.len()
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);

    let push_expr = match &lowered.module.stmt(block.statements[1]).kind {
        StmtKind::Binding { initializer, .. } => *initializer,
        other => panic!("unexpected stmt kind: {other:?}"),
    };
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(push_expr),
        Some(TypeId::Array(
            Box::new(TypeId::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable
        ))
    );
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::USize))
    );
}

#[test]
fn infers_string_method_call_types() {
    let lowered = common::lower_ok(
        r#"
fn main(value: String) -> usize {
    value.len_bytes()
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::USize))
    );
}

#[test]
fn exposes_stdlib_standard_builtin_surface_metadata() {
    assert!(surface::builtin_type("String").is_some());
    assert!(surface::builtin_type("usize").is_some());
    assert!(surface::builtin_type("str").is_none());

    let option = surface::standard_enum("Option").expect("Option should be standard");
    assert_eq!(option.arity, 1);
    assert_eq!(option.variants[0].name, "Some");
    assert_eq!(option.variants[1].name, "None");

    let result = surface::standard_enum("Result").expect("Result should be standard");
    assert_eq!(result.arity, 2);
    assert_eq!(result.variants[0].name, "Ok");
    assert_eq!(result.variants[1].name, "Err");
    assert_eq!(
        surface::standard_type_constructor("LinkedHashMap")
            .expect("Map should be standard")
            .arity,
        2
    );
    assert_eq!(
        surface::standard_type_constructor("LinkedHashSet")
            .expect("Set should be standard")
            .arity,
        1
    );
    assert!(surface::standard_module("std::array").is_some());
    assert!(surface::standard_module("std::map").is_some());
    assert!(surface::standard_module("std::set").is_some());
    assert!(surface::standard_module("std::string").is_some());
    assert!(surface::standard_module("std::iter").is_some());
    assert!(surface::standard_module("std::fs").is_none());

    assert!(surface::supports_const_type(&TypeId::Builtin(
        BuiltinType::U64
    )));
    assert!(!surface::supports_const_type(&TypeId::Builtin(
        BuiltinType::String
    )));
    assert!(surface::supports_hash_key(&TypeId::Builtin(
        BuiltinType::String
    )));
    assert!(!surface::supports_hash_key(&TypeId::Builtin(
        BuiltinType::F64
    )));
    let map_get = surface::standard_function(surface::StandardModule::Map, "LinkedHashMap::get")
        .expect("std::map::LinkedHashMap::get should be standard");
    assert_eq!(map_get.intrinsic, surface::StandardIntrinsic::MapGet);
    assert_eq!(
        map_get.constraints[0].constraint,
        surface::StandardTypeConstraint::HashKey
    );
    assert_eq!(
        surface::standard_function(surface::StandardModule::String, "String::slice")
            .expect("std::string::String::slice should be standard")
            .arity,
        3
    );
    assert!(
        surface::standard_function(surface::StandardModule::Option, "Option::and_then").is_some()
    );
    assert!(
        surface::standard_function(surface::StandardModule::Result, "Result::map_err").is_some()
    );
    assert!(surface::standard_function(surface::StandardModule::Math, "clamp").is_some());
    assert!(surface::standard_function(surface::StandardModule::Debug, "panic").is_some());
    assert_eq!(
        surface::standard_method(surface::StandardMethodReceiver::Map, "insert")
            .expect("Map.insert should be standard")
            .intrinsic,
        surface::StandardIntrinsic::MapInsert
    );
    assert!(surface::standard_method(surface::StandardMethodReceiver::Set, "difference").is_none());
    let difference = crate::builtin::traits::StandardTrait::Set
        .contract()
        .methods
        .iter()
        .find(|method| method.name == "difference")
        .expect("Set interface default");
    assert_eq!(
        crate::builtin::declarations::native_default_method(&difference.id),
        Some(crate::builtin::declarations::NativeDefaultMethod::SetDifference)
    );
    assert_eq!(
        surface::standard_method(surface::StandardMethodReceiver::String, "len_chars")
            .expect("String.len_chars should be standard")
            .arity,
        0
    );
}

#[test]
fn resolves_stdlib_standard_builtin_type_annotations() {
    let lowered = common::lower_ok(
        r#"
fn choose(value: Option<i32>) -> Option<i32> { value }
fn fallible(value: Result<i32, String>) -> Result<i32, String> { value }
fn lookup(value: LinkedHashMap<String, i32>) -> LinkedHashMap<String, i32> { value }
fn unique(value: LinkedHashSet<String>) -> LinkedHashSet<String> { value }
fn sized(value: usize) -> usize { value }
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");

    assert_eq!(
        typed.functions[0].return_type,
        TypeId::StandardEnum {
            kind: surface::StandardEnum::Option,
            args: vec![TypeId::Builtin(BuiltinType::I32)],
        }
    );
    assert_eq!(
        typed.functions[1].return_type,
        TypeId::StandardEnum {
            kind: surface::StandardEnum::Result,
            args: vec![
                TypeId::Builtin(BuiltinType::I32),
                TypeId::Builtin(BuiltinType::String),
            ],
        }
    );
    assert_eq!(
        typed.functions[2].return_type,
        TypeId::Map {
            key: Box::new(TypeId::Builtin(BuiltinType::String)),
            value: Box::new(TypeId::Builtin(BuiltinType::I32)),
            access: CollectionAccess::Mutable
        }
    );
    assert_eq!(
        typed.functions[3].return_type,
        TypeId::Set(
            Box::new(TypeId::Builtin(BuiltinType::String)),
            CollectionAccess::Mutable
        )
    );
    assert_eq!(
        typed.functions[4].return_type,
        TypeId::Builtin(BuiltinType::USize)
    );
}

#[test]
fn resolves_standard_module_imports_facade_exports_and_function_calls() {
    let lowered = common::lower_ok(
        r#"
pub use std::math as math;
use std::map::LinkedHashMap::len as map_len;

fn size(values: LinkedHashMap<String, i32>) -> usize {
    map_len(values)
}

fn clamp(value: i32) -> i32 {
    math::clamp(value, value, value)
}
"#,
    );
    assert!(
        lowered
            .module
            .imports
            .iter()
            .any(|import| { import.alias == "math" && import.path == "std::math" })
    );
    assert!(
        lowered.module.exports.iter().any(|export| {
            export.name == "math" && matches!(export.item, ExportItem::Import(_))
        })
    );

    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    assert!(names.items.lookup("math").is_some_and(|r| matches!(
        r.target(),
        Some(crate::resolver::ResolvedName::StandardModule(_))
    )));
    assert!(names.items.lookup("map_len").is_some_and(|r| matches!(
        r.target(),
        Some(crate::resolver::ResolvedName::StandardFunction(_))
    )));
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");

    let size = &lowered.module.functions[0];
    let size_tail = lowered
        .module
        .block(size.body)
        .tail_expr
        .expect("size tail expr");
    assert_eq!(
        typed
            .type_table
            .call_resolution(size_tail)
            .map(|call| call.target),
        Some(crate::typeck::CallTarget::StandardIntrinsic(
            StandardIntrinsic::MapLen
        ))
    );
    assert_eq!(
        typed.type_table.expr_type(size_tail),
        Some(TypeId::Builtin(BuiltinType::USize))
    );

    let clamp = &lowered.module.functions[1];
    let clamp_tail = lowered
        .module
        .block(clamp.body)
        .tail_expr
        .expect("clamp tail expr");
    assert_eq!(
        typed
            .type_table
            .call_resolution(clamp_tail)
            .map(|call| call.target),
        Some(crate::typeck::CallTarget::StandardIntrinsic(
            StandardIntrinsic::MathClamp
        ))
    );
    assert_eq!(
        typed.type_table.expr_type(clamp_tail),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn type_checks_standard_methods_and_records_intrinsics() {
    let lowered = common::lower_ok(
        r#"
fn keys(values: LinkedHashMap<String, i32>) -> List<String> {
    values.keys()
}

fn chars(value: String) -> usize {
    value.len_chars()
}

fn popped(values: ArrayList<i32>) -> Option<i32> {
    values.pop()
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");

    let keys_tail = lowered
        .module
        .block(lowered.module.functions[0].body)
        .tail_expr
        .expect("keys tail expr");
    assert_eq!(
        typed
            .type_table
            .call_resolution(keys_tail)
            .map(|call| call.target),
        Some(crate::typeck::CallTarget::StandardIntrinsic(
            StandardIntrinsic::MapKeys
        ))
    );
    let mut list = crate::builtin::traits::StandardTrait::List.nominal();
    list.arguments.push(TypeId::Builtin(BuiltinType::String));
    assert_eq!(
        typed.type_table.expr_type(keys_tail),
        Some(TypeId::Trait(list))
    );

    let chars_tail = lowered
        .module
        .block(lowered.module.functions[1].body)
        .tail_expr
        .expect("chars tail expr");
    assert_eq!(
        typed
            .type_table
            .call_resolution(chars_tail)
            .map(|call| call.target),
        Some(crate::typeck::CallTarget::StandardIntrinsic(
            StandardIntrinsic::StringLenChars
        ))
    );

    let popped_tail = lowered
        .module
        .block(lowered.module.functions[2].body)
        .tail_expr
        .expect("popped tail expr");
    assert_eq!(
        typed.type_table.expr_type(popped_tail),
        Some(TypeId::StandardEnum {
            kind: surface::StandardEnum::Option,
            args: vec![TypeId::Builtin(BuiltinType::I32)],
        })
    );
}

#[test]
fn enforces_standard_hash_key_constraints_for_collections_and_generic_calls() {
    let lowered = common::lower_ok(
        r#"
fn contains<K: Eq + Hash, V>(values: LinkedHashMap<K, V>, key: K) -> bool {
    std::map::LinkedHashMap::contains_key(values, key)
}

fn unique<T: Eq + Hash>(values: LinkedHashSet<T>) -> usize {
    std::set::LinkedHashSet::len(values)
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    check_module(&lowered, &names, None)
        .into_checked()
        .expect("hash-key constrained generics should type check");

    let lowered = common::lower_ok(
        "fn bad(values: LinkedHashMap<f64, i32>) -> usize { std::map::LinkedHashMap::len(values) }",
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("f64 map keys should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::StandardConstraintNotSatisfied { constraint, .. }
                if constraint == "Eq + Hash"
        )
    }));

    let lowered = common::lower_ok(
        "fn bad<K, V>(values: LinkedHashMap<K, V>) -> usize { std::map::LinkedHashMap::len(values) }",
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("unconstrained generic map key should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::StandardConstraintNotSatisfied { type_name, constraint, .. }
                if type_name == "K" && constraint == "Eq + Hash"
        )
    }));
}

#[test]
fn rejects_standard_library_invalid_arity_and_argument_types() {
    let lowered = common::lower_ok("fn bad() -> i32 { std::math::clamp(1, 2) }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("standard call arity should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::CallArityMismatch {
                function_name: "std::math::clamp".to_owned(),
                expected: 3,
                found: 2,
            }
    }));

    let lowered = common::lower_ok(
        r#"
fn bad(values: LinkedHashMap<String, i32>) -> bool {
    values.contains_key(1)
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("standard method key type should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "std::map::LinkedHashMap::contains_key".to_owned(),
                parameter_name: "key".to_owned(),
                expected: "String".to_owned(),
                found: "i32".to_owned(),
            }
    }));
}

#[test]
fn checks_standard_numeric_surface() {
    let lowered = common::lower_ok(
        r#"
fn signed(value: i16) -> i16 { -value }
fn unsigned(lhs: u64, rhs: u64) -> u64 { lhs + rhs }
fn float(lhs: f64, rhs: f64) -> bool { lhs < rhs }
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    check_module(&lowered, &names, None)
        .into_checked()
        .expect("standard numeric types should check");

    let lowered = common::lower_ok("fn bad(value: u32) -> u32 { -value }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("unsigned negation should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::UnaryOperandTypeMismatch {
                operator: "-",
                expected: "numeric".to_string(),
                found: "u32".to_string(),
            }
    }));
}

#[test]
fn checks_print_builtin_signature() {
    let lowered = common::lower_ok(r#"fn main() { print("hello"); }"#);
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    check_module(&lowered, &names, None)
        .into_checked()
        .expect("print should accept str");

    let lowered = common::lower_ok("fn main() { print(1); }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject print argument");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "print".to_string(),
                parameter_name: "message".to_string(),
                expected: "String".to_string(),
                found: "i32".to_string(),
            }
    }));
}
