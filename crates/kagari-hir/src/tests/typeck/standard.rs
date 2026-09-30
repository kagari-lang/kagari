use super::*;
use crate::{
    aggregates::MethodDefault, builtin::traits::StandardTraitSemantics, native::NativeBinding,
};
use kagari_abi::{callable::EngineNativeBinding, standard::StandardIntrinsic};

#[test]
fn infers_array_method_call_types() {
    let source = SourceFile::new(
        "array-method.kgr",
        r#"
fn main() -> usize {
    val values = [1, 2];
    val next = values.push(3);
    next.len()
}
"#,
    );
    let analyzed = crate::analyze_source(&source, Default::default())
        .into_checked()
        .expect("checked installed declarations");
    let typed = &analyzed.typed;
    let lowered = &analyzed.lowered;
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body.unwrap());

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
    let source = SourceFile::new(
        "string-method.kgr",
        r#"
fn main(value: String) -> usize {
    value.len_bytes()
}
"#,
    );
    let analyzed = crate::analyze_source(&source, Default::default())
        .into_checked()
        .expect("checked installed declarations");
    let typed = &analyzed.typed;
    let lowered = &analyzed.lowered;
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body.unwrap());
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::USize))
    );
}

#[test]
fn exposes_installed_standard_declarations_and_checked_signatures() {
    use crate::{
        analysis::AnalysisDatabase,
        declarations::DeclarationId,
        native::NativeTypeKind,
        typeck::{ConstraintTarget, FunctionImplementation},
    };
    use kagari_abi::standard::{
        surface::{self as standard_surface, StandardEnum},
        traits::StandardTrait,
    };
    use kagari_common::source_database::{SourceDatabase, SourceLayer};

    assert!(standard_surface::builtin_type("String").is_some());
    assert!(standard_surface::builtin_type("usize").is_some());
    assert!(standard_surface::builtin_type("str").is_none());
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("contracts.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let facts = snapshot.file(root).unwrap().result().facts();
    let declarations = snapshot.declaration_snapshot();
    for (kind, arity, variants) in [
        (StandardEnum::Option, 1, ["Some", "None"]),
        (StandardEnum::Result, 2, ["Ok", "Err"]),
    ] {
        let enumeration = facts
            .aggregates
            .enumerations()
            .find(|declaration| declaration.native_type == Some(NativeTypeKind::Enum(kind)))
            .unwrap();
        assert_eq!(enumeration.generic_params.len(), arity);
        assert_eq!(
            enumeration
                .variants
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>(),
            variants
        );
    }
    for (name, arity) in [("LinkedHashMap", 2), ("LinkedHashSet", 1)] {
        let (file, declaration) = declarations
            .files()
            .find_map(|file| {
                file.declarations()
                    .iter()
                    .find(|item| item.name == name)
                    .map(|declaration| (file, declaration))
            })
            .unwrap();
        let DeclarationId::Definition(id) = &declaration.id else {
            panic!("type declaration")
        };
        assert_eq!(file.declarations().parameters_of(id).len(), arity);
    }
    for namespace in ["array", "map", "set", "string", "iter"] {
        assert!(
            declarations
                .files()
                .any(|file| file.source().module_identity().path == [namespace])
        );
    }
    assert!(
        !declarations
            .files()
            .any(|file| file.source().module_identity().path == ["fs"])
    );
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

    let signature = |binding| {
        declarations
            .files()
            .find_map(|source| {
                snapshot
                    .signature_snapshot()
                    .file(source.source().id())
                    .unwrap()
                    .signatures()
                    .facts()
                    .functions()
                    .iter()
                    .find(|function| {
                        function.implementation
                            == FunctionImplementation::Native(NativeBinding::Engine(
                                EngineNativeBinding::Intrinsic(binding),
                            ))
                    })
            })
            .expect("checked native function signature")
    };
    let map_get = signature(StandardIntrinsic::MapGet);
    let key_bounds = map_get.bounds.get(&map_get.params[1].ty).unwrap();
    for kind in [StandardTrait::Eq, StandardTrait::Hash] {
        assert!(key_bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(interface) if interface.declaration == kind.nominal().declaration)));
    }
    assert_eq!(signature(StandardIntrinsic::StringSlice).params.len(), 3);
    for binding in [
        StandardIntrinsic::OptionAndThen,
        StandardIntrinsic::ResultMapErr,
        StandardIntrinsic::MathClamp,
        StandardIntrinsic::DebugAssert,
        StandardIntrinsic::MapInsert,
    ] {
        assert_eq!(
            signature(binding).implementation,
            FunctionImplementation::Native(NativeBinding::Engine(EngineNativeBinding::Intrinsic(
                binding
            )))
        );
    }
    assert!(!facts.aggregates.inherent_methods().any(|method| matches!(
        method.owner,
        TypeId::Set(_, _)
    ) && method.function.name
        == "difference"));
    let difference = facts
        .aggregates
        .trait_(&StandardTrait::Set.nominal().declaration)
        .unwrap()
        .methods
        .iter()
        .find(|method| method.name == "difference")
        .unwrap();
    assert_eq!(
        difference.default,
        Some(MethodDefault::Native {
            binding: kagari_abi::standard::bindings::NativeDefaultMethod::SetDifference,
            overridable: false,
        })
    );
    let len_chars = signature(StandardIntrinsic::StringLenChars);
    assert_eq!(len_chars.params.len(), 1);
    assert_eq!(len_chars.params[0].name, "self");
}

#[test]
fn resolves_stdlib_standard_builtin_type_annotations() {
    let source = SourceFile::new(
        "native-annotations.kgr",
        r#"
fn choose(value: Option<i32>) -> Option<i32> { value }
fn fallible(value: Result<i32, String>) -> Result<i32, String> { value }
fn lookup(value: LinkedHashMap<String, i32>) -> LinkedHashMap<String, i32> { value }
fn unique(value: LinkedHashSet<String>) -> LinkedHashSet<String> { value }
fn sized(value: usize) -> usize { value }
"#,
    );
    let analyzed = crate::analyze_source(&source, Default::default())
        .into_checked()
        .expect("checked installed declarations");
    let typed = &analyzed.typed;

    assert_eq!(
        typed.functions[0].return_type,
        TypeId::StandardEnum {
            kind: kagari_abi::standard::surface::StandardEnum::Option,
            args: vec![TypeId::Builtin(BuiltinType::I32)],
        }
    );
    assert_eq!(
        typed.functions[1].return_type,
        TypeId::StandardEnum {
            kind: kagari_abi::standard::surface::StandardEnum::Result,
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
    let source = kagari_common::SourceFile::new(
        "standard-imports.kgr",
        r#"
pub use std::math;
use std::map::LinkedHashMap::len as map_len;

fn size(values: LinkedHashMap<String, i32>) -> usize {
    map_len(values)
}

fn clamp(value: i32) -> i32 {
    math::clamp(value, value, value)
}
"#,
    );
    let analyzed = crate::analyze_source(&source, Default::default())
        .into_checked()
        .expect("installed declarations should resolve and type check");
    let lowered = &analyzed.lowered;
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
    for (name, function) in [("math", false), ("map_len", true)] {
        let binding = analyzed.names.items.lookup(name).unwrap().target().unwrap();
        let crate::imports::ImportTarget::Source(target) =
            analyzed.names.imports.binding(binding).unwrap()
        else {
            panic!("standard imports must retain their source declaration");
        };
        assert_eq!(target.module.package.0, "kagari-std");
        assert_eq!(
            matches!(target.item, Some(ExportItem::Function(_))),
            function
        );
        if !function {
            assert!(target.item.is_none());
        }
    }
    for (function, namespace, name, expected) in [
        (
            &lowered.module.functions[0],
            "map",
            "len",
            BuiltinType::USize,
        ),
        (
            &lowered.module.functions[1],
            "math",
            "clamp",
            BuiltinType::I32,
        ),
    ] {
        let tail = lowered
            .module
            .block(function.body.unwrap())
            .tail_expr
            .unwrap();
        let call = analyzed.typed.type_table.call_resolution(tail).unwrap();
        let crate::typeck::CallTarget::SourceFunction(target) = &call.target else {
            panic!("call must use the imported checked function");
        };
        let imported = analyzed.imported_functions.target(target).unwrap();
        assert_eq!(imported.declaration.module.package.0, "kagari-std");
        assert_eq!(imported.declaration.module.path, [namespace]);
        assert_eq!(imported.signature.name, name);
        assert_eq!(
            analyzed.typed.type_table.expr_type(tail),
            Some(TypeId::Builtin(expected))
        );
    }
}

#[test]
fn type_checks_standard_methods_and_records_checked_bindings() {
    let source = SourceFile::new(
        "standard-methods.kgr",
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
    let analyzed = crate::analyze_source(&source, Default::default())
        .into_checked()
        .expect("checked installed method declarations");
    let lowered = &analyzed.lowered;
    let typed = &analyzed.typed;
    let binding = |expression| {
        let call = typed.type_table.call_resolution(expression).unwrap();
        let crate::typeck::CallTarget::SourceFunction(target) = &call.target else {
            panic!("ordinary imported method target");
        };
        analyzed
            .imported_functions
            .target(target)
            .unwrap()
            .signature
            .implementation
    };

    let keys_tail = lowered
        .module
        .block(lowered.module.functions[0].body.unwrap())
        .tail_expr
        .expect("keys tail expr");
    assert_eq!(
        binding(keys_tail),
        crate::typeck::FunctionImplementation::Native(NativeBinding::Engine(
            EngineNativeBinding::Intrinsic(StandardIntrinsic::MapKeys)
        ))
    );
    let mut list = kagari_abi::standard::traits::StandardTrait::List.nominal();
    list.arguments.push(TypeId::Builtin(BuiltinType::String));
    assert_eq!(
        typed.type_table.expr_type(keys_tail),
        Some(TypeId::Trait(list))
    );

    let chars_tail = lowered
        .module
        .block(lowered.module.functions[1].body.unwrap())
        .tail_expr
        .expect("chars tail expr");
    assert_eq!(
        binding(chars_tail),
        crate::typeck::FunctionImplementation::Native(NativeBinding::Engine(
            EngineNativeBinding::Intrinsic(StandardIntrinsic::StringLenChars)
        ))
    );

    let popped_tail = lowered
        .module
        .block(lowered.module.functions[2].body.unwrap())
        .tail_expr
        .expect("popped tail expr");
    assert_eq!(
        typed.type_table.expr_type(popped_tail),
        Some(TypeId::StandardEnum {
            kind: kagari_abi::standard::surface::StandardEnum::Option,
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
    crate::analyze_source(&lowered.source, Default::default())
        .into_checked()
        .expect("hash-key constrained generics should type check");

    let lowered = common::lower_ok(
        "fn bad(values: LinkedHashMap<f64, i32>) -> usize { std::map::LinkedHashMap::len(values) }",
    );
    let diagnostics = crate::analyze_source(&lowered.source, Default::default())
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
    let diagnostics = crate::analyze_source(&lowered.source, Default::default())
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
    let diagnostics = crate::analyze_source(&lowered.source, Default::default())
        .into_checked()
        .expect_err("standard call arity should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::CallArityMismatch {
                function_name: "clamp".to_owned(),
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
    let diagnostics = crate::analyze_source(&lowered.source, Default::default())
        .into_checked()
        .expect_err("standard method key type should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "contains_key".to_owned(),
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
