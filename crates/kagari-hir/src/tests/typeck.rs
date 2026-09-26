use kagari_common::{DiagnosticKind, SourceFile, TypePosition};
use kagari_syntax::parse_module;

use crate::{
    builtin::surface::{self, IterableProtocol, StandardIntrinsic},
    hir::{ExportItem, ExprKind, PatternKind, StmtKind},
    resolver::resolve_names,
    tests::common,
    tests::common::check_module,
    types::{BuiltinType, TypeId},
};

#[test]
fn const_arithmetic_failures_preserve_other_semantic_facts() {
    for (expression, reason) in [
        ("2147483647 + 1", "integer overflow"),
        ("(-2147483647 - 1) - 1", "integer overflow"),
        ("50000 * 50000", "integer overflow"),
        ("-(-2147483647 - 1)", "integer overflow"),
        ("(-2147483647 - 1) / -1", "integer overflow"),
        ("1 / 0", "integer division by zero"),
        ("(2147483647 + 1) - 1", "integer overflow"),
    ] {
        let source = SourceFile::new(
            "const.kgr",
            format!(
                "const BAD: i32 = {expression}; const GOOD: i32 = 6 * 7; fn good() -> i32 {{ GOOD }}"
            ),
        );
        let result = crate::analyze_source(&source, Default::default());
        let diagnostic = result.diagnostics().iter().find(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::InvalidConstInitializer { const_name, reason: actual }
                if const_name == "BAD" && actual == reason
        )).unwrap_or_else(|| panic!("{expression}: {:?}", result.diagnostics()));
        let span = diagnostic.span.unwrap();
        assert!(span.start >= "const BAD: i32 = ".len());
        assert!(span.end <= "const BAD: i32 = ".len() + expression.len());
        let facts = result.facts();
        assert_eq!(facts.typed.const_values.len(), 1);
        assert_eq!(
            facts.typed.const_values.values().next(),
            Some(&crate::typeck::ScalarValue::I32(42))
        );
        assert_eq!(
            facts.typed.functions[0].return_type,
            TypeId::Builtin(BuiltinType::I32)
        );
        assert!(result.into_codegen().is_err());
    }
}

#[test]
fn invalid_literals_and_patterns_retain_neighbor_types() {
    for source in [
        "fn bad() -> i32 { 2147483648 }",
        "fn bad() -> i32 { -2147483649 }",
        "fn bad() -> i32 { 99999999999999999999999999999999999999999 }",
        "fn bad() -> f32 { 99999999999999999999999999999999999999999.0 }",
        "fn bad() -> i32 { match 1 { 2147483648 => 1, _ => 2 } }",
        "const BAD: i32 = 2147483648;",
    ] {
        let source = SourceFile::new("literal.kgr", format!("{source} fn good() -> i32 {{ 42 }}"));
        let result = crate::analyze_source(&source, Default::default());
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::InvalidLiteral { .. })),
            "{:?}",
            result.diagnostics()
        );
        assert_eq!(
            result
                .facts()
                .typed
                .functions
                .iter()
                .find(|function| function.name == "good")
                .unwrap()
                .return_type,
            TypeId::Builtin(BuiltinType::I32)
        );
        assert!(result.into_codegen().is_err());
    }
    let source = SourceFile::new(
        "literal.kgr",
        "fn main() -> i32 { match true { 1 => 1, _ => 2 } }",
    );
    let result = crate::analyze_source(&source, Default::default());
    assert!(
        result.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::PatternTypeMismatch { .. }
        ))
    );
    assert!(result.into_codegen().is_err());
}

#[test]
fn const_type_mismatch_is_rejected_before_codegen() {
    let source = SourceFile::new(
        "const.kgr",
        "const BAD: i32 = true; fn main() -> i32 { BAD }",
    );
    let result = crate::analyze_source(&source, Default::default());
    assert!(result.diagnostics().iter().any(|diagnostic| matches!(
        &diagnostic.kind, DiagnosticKind::InvalidConstInitializer { reason, .. }
            if reason == "expected `i32`, found `bool`"
    )));
    assert!(result.into_codegen().is_err());
}

#[test]
fn reports_unknown_parameter_type() {
    let lowered = common::lower_ok("fn foo(value: number) {}");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject type");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::UnknownType {
            type_name: "number".to_string(),
            function_name: "foo".to_string(),
            position: TypePosition::Parameter,
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: unknown parameter type `number` in function `foo` at 13..20"
    );
}

#[test]
fn reports_unknown_return_type() {
    let lowered = common::lower_ok("fn foo() -> number {}");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject type");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::UnknownType {
            type_name: "number".to_string(),
            function_name: "foo".to_string(),
            position: TypePosition::Return,
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: unknown return type `number` in function `foo` at 11..18"
    );
}

#[test]
fn reports_invalid_const_initializer_expression() {
    let lowered = common::lower_ok("const VALUE: i32 = type_of(1);");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject const initializer");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidConstInitializer {
            const_name: "VALUE".to_string(),
            reason: "unsupported const initializer expression".to_string(),
        }
    );
}

#[test]
fn reports_reflection_write_on_const_value() {
    let lowered = common::lower_ok(
        r#"
struct Point { var x: i32 }
struct Holder { var inner: Point }
const ROOT: Holder = Holder { inner: Point { x: 1 } };

fn main() -> Point { set_field(ROOT.inner, "x", 2) }
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject reflection write on const");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ConstWriteNotAllowed {
                const_name: "ROOT".to_string(),
            }
    }));
}

#[test]
fn reports_const_dependency_cycle() {
    let lowered = common::lower_ok(
        r#"
const A: i32 = B;
const B: i32 = A;
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject cycle");

    assert!(diagnostics.iter().any(|diagnostic| diagnostic.kind
        == DiagnosticKind::ConstCycle {
            const_name: "A".to_string(),
        }));
}

#[test]
fn rejects_heap_backed_const_types() {
    let lowered = common::lower_ok(
        r#"
struct Point { var x: i32, var y: i32 }
const PAIR: (i32, i32) = (1, 2);
const VALUES: [i32] = [3, 4];
const POINT: Point = Point { x: 5, y: 6 };
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject heap-backed const types");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "PAIR".to_string(),
                reason: "const type `(i32, i32)` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "VALUES".to_string(),
                reason: "const type `[i32]` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "POINT".to_string(),
                reason: "const type `Point` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
}

#[test]
fn plain_function_calls_keep_fresh_return_flow() {
    let lowered = common::lower_ok(
        r#"
struct Point { var x: i32 }

fn id(point: Point) -> Point { point }

fn main(point: Point) -> Point {
    id(point)
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should accept plain call");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Struct(crate::types::NominalType {
            associated_types: Default::default(),
            declaration: common::definition(
                &lowered,
                kagari_common::identity::DefinitionKind::Struct,
                "Point"
            ),
            arguments: Vec::new()
        }))
    );
}

#[test]
fn reports_function_call_argument_type_mismatch() {
    let lowered = common::lower_ok(
        r#"
fn add_one(value: i32) -> i32 { value + 1 }

fn main() -> i32 {
    add_one(true)
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject argument type");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "add_one".to_string(),
                parameter_name: "value".to_string(),
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_function_call_arity_mismatch() {
    let lowered = common::lower_ok(
        r#"
fn answer() -> i32 { 42 }

fn main() -> i32 {
    answer(1)
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject arity mismatch");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::CallArityMismatch {
                function_name: "answer".to_string(),
                expected: 0,
                found: 1,
            }
    }));
}

#[test]
fn records_expression_types_for_resolved_body_expressions() {
    let lowered =
        common::lower_ok("fn main(value: i32) -> i32 { val next: i32 = value + 1; next }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered.module.block(function.body);
    let let_stmt = lowered.module.stmt(block.statements[0]);
    let init_expr = match &let_stmt.kind {
        StmtKind::Binding { initializer, .. } => *initializer,
        other => panic!("unexpected stmt kind: {other:?}"),
    };
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(init_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

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
        Some(TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32))))
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
        surface::standard_type_constructor("Map")
            .expect("Map should be standard")
            .arity,
        2
    );
    assert_eq!(
        surface::standard_type_constructor("Set")
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
    assert!(matches!(
        surface::iterable_protocol(&TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32)))),
        Some(IterableProtocol::Array {
            item: TypeId::Builtin(BuiltinType::I32)
        })
    ));
    assert!(matches!(
        surface::iterable_protocol(&TypeId::Builtin(BuiltinType::String)),
        Some(IterableProtocol::String {
            item: BuiltinType::String
        })
    ));
    assert!(matches!(
        surface::iterable_protocol(&TypeId::Map {
            key: Box::new(TypeId::Builtin(BuiltinType::String)),
            value: Box::new(TypeId::Builtin(BuiltinType::I32)),
        }),
        Some(IterableProtocol::Map {
            key: TypeId::Builtin(BuiltinType::String),
            value: TypeId::Builtin(BuiltinType::I32),
        })
    ));
    assert!(matches!(
        surface::iterable_protocol(&TypeId::Set(Box::new(TypeId::Builtin(BuiltinType::String)))),
        Some(IterableProtocol::Set {
            item: TypeId::Builtin(BuiltinType::String),
        })
    ));

    let map_get = surface::standard_function(surface::StandardModule::Map, "get")
        .expect("std::map::get should be standard");
    assert_eq!(map_get.intrinsic, surface::StandardIntrinsic::MapGet);
    assert_eq!(
        map_get.constraints[0].constraint,
        surface::StandardTypeConstraint::HashKey
    );
    assert_eq!(
        surface::standard_function(surface::StandardModule::String, "slice")
            .expect("std::string::slice should be standard")
            .arity,
        3
    );
    assert!(surface::standard_function(surface::StandardModule::Option, "and_then").is_some());
    assert!(surface::standard_function(surface::StandardModule::Result, "map_err").is_some());
    assert!(surface::standard_function(surface::StandardModule::Math, "clamp").is_some());
    assert!(surface::standard_function(surface::StandardModule::Debug, "panic").is_some());
    assert_eq!(
        surface::standard_method(surface::StandardMethodReceiver::Map, "insert")
            .expect("Map.insert should be standard")
            .intrinsic,
        surface::StandardIntrinsic::MapInsert
    );
    assert_eq!(
        surface::standard_method(surface::StandardMethodReceiver::Set, "difference")
            .expect("Set.difference should be standard")
            .arity,
        1
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
fn lookup(value: Map<String, i32>) -> Map<String, i32> { value }
fn unique(value: Set<String>) -> Set<String> { value }
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
        }
    );
    assert_eq!(
        typed.functions[3].return_type,
        TypeId::Set(Box::new(TypeId::Builtin(BuiltinType::String)))
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
use std::map::len as map_len;

fn size(values: Map<String, i32>) -> usize {
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
fn keys(values: Map<String, i32>) -> [String] {
    values.keys()
}

fn chars(value: String) -> usize {
    value.len_chars()
}

fn popped(values: [i32]) -> Option<i32> {
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
    assert_eq!(
        typed.type_table.expr_type(keys_tail),
        Some(TypeId::Array(Box::new(TypeId::Builtin(
            BuiltinType::String
        ))))
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
fn contains<K: HashKey, V>(values: Map<K, V>, key: K) -> bool {
    std::map::contains_key(values, key)
}

fn unique<T: HashKey>(values: Set<T>) -> usize {
    std::set::len(values)
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    check_module(&lowered, &names, None)
        .into_checked()
        .expect("hash-key constrained generics should type check");

    let lowered =
        common::lower_ok("fn bad(values: Map<f64, i32>) -> usize { std::map::len(values) }");
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
                if constraint == "HashKey"
        )
    }));

    let lowered =
        common::lower_ok("fn bad<K, V>(values: Map<K, V>) -> usize { std::map::len(values) }");
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
                if type_name == "K" && constraint == "HashKey"
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
fn bad(values: Map<String, i32>) -> bool {
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
                function_name: "std::map::contains_key".to_owned(),
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

#[test]
fn reports_return_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { true }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject return");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::ReturnTypeMismatch {
            function_name: "foo".to_string(),
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_break_and_continue_outside_loop() {
    let lowered = common::lower_ok("fn foo() { break; continue; }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject control flow");

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::BreakOutsideLoop);
    assert_eq!(diagnostics[1].kind, DiagnosticKind::ContinueOutsideLoop);
}

#[test]
fn reports_invalid_assignment_target() {
    let lowered = common::lower_ok("fn foo() -> i32 { foo = 1; 0 }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject assignment target");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "function item is not assignable".to_string(),
        }
    );
}

#[test]
fn reports_assignment_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { var x: i32 = 1; x = true; x }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject assignment");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::AssignmentTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_condition_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { if 1 { 1 } else { 2 } }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject condition type");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ConditionTypeMismatch {
                context: "if",
                found: "i32".to_string(),
            }
    }));
}

#[test]
fn reports_binary_operand_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { 1 + true }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject operands");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::BinaryOperandTypeMismatch {
                operator: "+",
                expected: "matching numeric".to_string(),
                lhs: "i32".to_string(),
                rhs: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_array_element_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> [i32] { [1, true] }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject array elements");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArrayElementTypeMismatch {
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_invalid_struct_initializers() {
    let lowered = common::lower_ok(
        r#"
struct Point { var x: i32, var y: bool }

fn foo() -> Point {
    Point { x: true, z: 1, x: 2 }
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject struct init");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::AssignmentTypeMismatch {
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "unknown field `z`".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "duplicate field `x`".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "missing field `y`".to_string(),
            }
    }));
}

#[test]
fn allows_assignment_to_var_local_but_not_val_local_or_param() {
    let var_local = common::lower_ok("fn foo() -> i32 { var x: i32 = 1; x = 2; x }");
    let names = resolve_names(&var_local)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&var_local, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = &var_local.module.functions[0];
    let block = var_local.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    let val_local = common::lower_ok("fn foo() -> i32 { val x: i32 = 1; x = 2; x }");
    let names = resolve_names(&val_local)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&val_local, &names, None)
        .into_checked()
        .expect_err("val local should reject write");
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "`val` binding cannot be reassigned".to_string(),
        }
    );

    let param_assignment = common::lower_ok("fn foo(value: i32) -> i32 { value = 1; value }");
    let names = resolve_names(&param_assignment)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&param_assignment, &names, None)
        .into_checked()
        .expect_err("parameter should reject write");
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "function parameters are `val` bindings and cannot be reassigned".to_string(),
        }
    );
}

#[test]
fn allows_assignment_to_var_field_and_index_places() {
    let field_assignment = common::lower_ok(
        r#"
struct Point { var x: i32 }
struct Holder { val inner: Point }

fn main() -> i32 {
    val holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 3;
    holder.inner.x
}
"#,
    );
    let names = resolve_names(&field_assignment)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&field_assignment, &names, None)
        .into_checked()
        .expect("field assignment should type check");
    let function = &field_assignment.module.functions[0];
    let block = field_assignment.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    let param_field_assignment = common::lower_ok(
        r#"
struct Point { var x: i32 }

fn main(point: Point) -> i32 {
    point.x = 3;
    point.x
}
"#,
    );
    let names = resolve_names(&param_field_assignment)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&param_field_assignment, &names, None)
        .into_checked()
        .expect("var field assignment through parameter should type check");
    let function = &param_field_assignment.module.functions[0];
    let block = param_field_assignment.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    let index_assignment = common::lower_ok(
        r#"
fn main() -> i32 {
    val values = [1, 2];
    values[1] = 9;
    values[1]
}
"#,
    );
    let names = resolve_names(&index_assignment)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&index_assignment, &names, None)
        .into_checked()
        .expect("index assignment should type check");
    let function = &index_assignment.module.functions[0];
    let block = index_assignment.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn rejects_assignment_to_val_field() {
    let field_assignment = common::lower_ok(
        r#"
struct Point { val x: i32 }

fn main() -> i32 {
    val point = Point { x: 1 };
    point.x = 3;
    point.x
}
"#,
    );
    let names = resolve_names(&field_assignment)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&field_assignment, &names, None)
        .into_checked()
        .expect_err("val field should reject write");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "`val` field `x` cannot be assigned".to_string(),
        }
    );
}

#[test]
fn reports_if_branch_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { if true { 1 } else { false } }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject if");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::IfBranchTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_match_arm_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { match 1 { 1 => 1, _ => false } }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject match");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::MatchArmTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn records_named_match_pattern_binding_type() {
    let lowered = common::lower_ok("fn foo(value: i32) -> i32 { match value { bound => bound } }");
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    let pattern_local = match &lowered.module.expr(tail_expr).kind {
        ExprKind::Match { arms, .. } => match lowered.module.pattern(arms[0].pattern).kind {
            PatternKind::Name { local, .. } => local,
            ref other => panic!("unexpected pattern kind: {other:?}"),
        },
        ref other => panic!("unexpected expr kind: {other:?}"),
    };

    assert_eq!(
        typed.type_table.local_type(pattern_local),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn records_const_reference_types() {
    let lowered = common::lower_ok(
        r#"
const VERSION: i32 = 1;

fn main() -> i32 { VERSION }
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        typed.consts.get(&lowered.module.consts[0].id),
        Some(&TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn rejects_assignment_to_const() {
    let const_storage = common::lower_ok(
        r#"
const VERSION: i32 = 1;
fn main() -> i32 { VERSION = 2; 0 }
"#,
    );
    let names = resolve_names(&const_storage)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&const_storage, &names, None)
        .into_checked()
        .expect_err("type checker should reject writes");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "`const` item cannot be reassigned".to_string(),
        }
    );
}

#[test]
fn rejects_non_spec_source_forms_before_hir_analysis() {
    let cases = [
        "fn main() { let value = 1; }",
        "pub static mut COUNTER: i32 = 0;",
        "fn apply(effect: dyn Effect) {}",
    ];

    for source in cases {
        let source = SourceFile::new("test.kg", source);
        let diagnostics = parse_module(&source).expect_err("source form should be rejected");
        assert!(
            !diagnostics.is_empty(),
            "expected parser diagnostics for non-spec source form"
        );
    }
}

#[test]
fn validates_trait_impl_and_interface_method_calls() {
    let lowered = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    fn show(self) -> String {
        self.name
    }
}

fn show_interface(value: Display) -> String {
    value.show()
}

fn show_static<T>(value: T) -> String
where T: Display
{
    value.show()
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");

    let show_interface = typed
        .functions
        .iter()
        .find(|function| function.name == "show_interface")
        .expect("expected show_interface");
    assert_eq!(
        show_interface.params[0].ty,
        TypeId::Trait(crate::types::NominalType {
            associated_types: Default::default(),
            declaration: common::definition(
                &lowered,
                kagari_common::identity::DefinitionKind::Trait,
                "Display"
            ),
            arguments: Vec::new()
        })
    );
    assert_eq!(
        show_interface.return_type,
        TypeId::Builtin(BuiltinType::String)
    );
}

#[test]
fn reports_unknown_trait_bounds() {
    let lowered = common::lower_ok(
        r#"
fn show<T>(value: T) -> T
where T: Missing
{
    value
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject unknown bound");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::UnknownTrait {
                trait_name: "Missing".to_string(),
            }
    }));
}

#[test]
fn rejects_interface_use_of_generic_trait_methods() {
    let lowered = common::lower_ok(
        r#"
trait Mapper {
    fn map<T>(self, value: T) -> T;
}

fn use_mapper(value: Mapper) {
    value;
}
"#,
    );
    let names = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject interface type");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidInterfaceType {
                trait_name: "Mapper".to_string(),
                reason: "method `map` is not interface-compatible".to_string(),
            }
    }));
}

#[test]
fn rejects_interface_method_with_another_self_parameter() {
    let lowered = common::lower_ok(
        "trait Pair { fn same(self, other: Self) -> bool; } fn use_pair(value: Pair) {}",
    );
    let names = resolve_names(&lowered).into_checked().unwrap();
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("a second Self argument cannot be called through an interface value");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidInterfaceType { reason, .. }
            if reason == "method `same` is not interface-compatible"
    )));
}

#[test]
fn rejects_invalid_trait_impls() {
    let missing_method = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {}
"#,
    );
    let names = resolve_names(&missing_method)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&missing_method, &names, None)
        .into_checked()
        .expect_err("type checker should reject impl");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::TraitMethodMismatch {
                trait_name: "Display".to_string(),
                method_name: "show".to_string(),
                reason: "missing impl method".to_string(),
            }
    }));

    let wrong_return = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    fn show(self) -> i32 {
        1
    }
}
"#,
    );
    let names = resolve_names(&wrong_return)
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&wrong_return, &names, None)
        .into_checked()
        .expect_err("type checker should reject impl");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::TraitMethodMismatch {
                trait_name: "Display".to_string(),
                method_name: "show".to_string(),
                reason: "return type expected `String`, found `i32`".to_string(),
            }
    }));
}

#[test]
fn trait_method_generic_binders_match_by_position() {
    let valid = common::lower_ok(
        "trait Convert { fn take<T>(self, value: T) -> T; } struct Holder {} impl Convert for Holder { fn take<U>(self, value: U) -> U { value } }",
    );
    let names = resolve_names(&valid).into_checked().unwrap();
    check_module(&valid, &names, None)
        .into_checked()
        .expect("equivalent method binders should match");

    let invalid = common::lower_ok(
        "trait Convert { fn take<T>(self, value: T) -> T; } struct Holder {} impl Convert for Holder { fn take<U, V>(self, value: U) -> U { value } }",
    );
    let names = resolve_names(&invalid).into_checked().unwrap();
    let diagnostics = check_module(&invalid, &names, None)
        .into_checked()
        .expect_err("different method binder arity should fail");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::TraitMethodMismatch { reason, .. }
            if reason == "generic parameter count differs"
    )));
}

#[test]
fn private_trait_method_bounds_match_after_trait_and_method_substitution() {
    let valid = common::lower_ok(
        "trait Marker<T> {} trait Consumer<T> { fn take<U: Marker<T>>(self, value: U) -> U; } struct Holder {} impl Consumer<i32> for Holder { fn take<V: Marker<i32>>(self, value: V) -> V { value } }",
    );
    let names = resolve_names(&valid).into_checked().unwrap();
    check_module(&valid, &names, None)
        .into_checked()
        .expect("equivalent applied method bounds should match");

    let invalid = common::lower_ok(
        "trait Marker<T> {} trait Consumer<T> { fn take<U: Marker<T>>(self, value: U) -> U; } struct Holder {} impl Consumer<i32> for Holder { fn take<V: Marker<bool>>(self, value: V) -> V { value } }",
    );
    let names = resolve_names(&invalid).into_checked().unwrap();
    let diagnostics = check_module(&invalid, &names, None)
        .into_checked()
        .expect_err("different private method bounds should fail");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::TraitMethodMismatch { reason, .. } if reason == "generic bound differs"
    )));
}

#[test]
fn applied_trait_interface_type_allows_inherited_binders_only() {
    let valid = common::lower_ok(
        "trait Echo<T> { fn get(self) -> T; } fn use_interface(value: Echo<i32>) {}",
    );
    let names = resolve_names(&valid).into_checked().unwrap();
    check_module(&valid, &names, None)
        .into_checked()
        .expect("an applied trait interface has concrete inherited arguments");

    let invalid = common::lower_ok(
        "trait Echo<T> { fn get<U>(self, value: U) -> T; } fn use_interface(value: Echo<i32>) {}",
    );
    let names = resolve_names(&invalid).into_checked().unwrap();
    let diagnostics = check_module(&invalid, &names, None)
        .into_checked()
        .expect_err("a method-local generic binder is not interface compatible");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidInterfaceType { reason, .. }
            if reason == "method `get` is not interface-compatible"
    )));
}

#[test]
fn wide_const_dependencies_keep_values_and_error_owners_by_declaration_slot() {
    let mut text = String::new();
    for index in 0..2_000 {
        text.push_str(&format!("const C{index}: i32 = BASE + {index}; "));
    }
    text.push_str("const BASE: i32 = 42; const BAD: i32 = BASE / 0; fn good() -> i32 { C1999 }");
    let result = crate::analyze_source(
        &SourceFile::new("wide-const.kgr", text.clone()),
        Default::default(),
    );
    let facts = result.facts();
    assert_eq!(facts.typed.const_values.len(), 2_001);
    for (index, item) in facts.lowered.module.consts.iter().take(2_000).enumerate() {
        assert_eq!(
            facts.typed.const_values.get(&item.id),
            Some(&crate::typeck::ScalarValue::I32(42 + index as i32))
        );
    }
    assert_eq!(result.diagnostics().len(), 1);
    let diagnostic = &result.diagnostics()[0];
    assert!(
        matches!(&diagnostic.kind, DiagnosticKind::InvalidConstInitializer { const_name, reason } if const_name == "BAD" && reason == "integer division by zero")
    );
    let span = diagnostic.span.unwrap();
    assert_eq!(&text[span.start..span.end], "BASE / 0");
    assert!(result.into_codegen().is_err());
}

#[test]
fn forward_const_annotations_preserve_cycle_and_initializer_errors() {
    for (text, expected) in [
        ("const A: i32 = B; const B: i32 = A;", "KG_TYPE_CONST_CYCLE"),
        (
            "const A: i32 = B; const B: i32 = true;",
            "KG_TYPE_INVALID_CONST_INITIALIZER",
        ),
    ] {
        let result = crate::analyze_source(
            &SourceFile::new("const-errors.kgr", text),
            Default::default(),
        );
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == expected),
            "{:?}",
            result.diagnostics()
        );
        assert!(
            !result
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::InvalidValueTarget { .. }))
        );
        assert!(result.into_codegen().is_err());
    }
}
