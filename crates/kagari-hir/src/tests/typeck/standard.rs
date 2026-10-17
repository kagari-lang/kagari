use super::*;
use crate::{
    analyze_source,
    declarations::DeclarationId,
    imports::{NamespaceId, SourceItem},
    language::semantics::ProtocolSemantics,
    native::{NativeBinding, adoption},
    resolver::resolved::ResolvedName,
    tests::test_analysis,
    typeck::{FunctionImplementation, table::CallTarget},
    types::NominalType,
};
use kagari_common::identity::{
    DefinitionKind,
    map::DefinitionContext,
    metadata::DefinitionMetadata,
    table::{DefinitionId, DefinitionTable},
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::{catalog as foundation_catalog, catalog, identity as library};
use kagari_types::{
    callable::CallableImplementation,
    collection::CollectionAccess,
    declaration::{
        module::ModuleDecl,
        names::{ExportName, NameNamespace},
    },
    language::{Protocol, binding},
    surface as standard_surface,
};
use std::sync::Arc;

fn foundation_vec(definitions: &DefinitionTable, item: TypeId) -> TypeId {
    TypeId::NativeObject(NominalType {
        declaration: definitions
            .lookup(&library::type_id("Vec", DefinitionKind::AssociatedType))
            .unwrap(),
        arguments: vec![item],
        associated_types: Default::default(),
    })
}

fn foundation_interface(definitions: &DefinitionTable, name: &str) -> NominalType {
    NominalType {
        declaration: definitions.lookup(&library::trait_id(name)).unwrap(),
        arguments: vec![],
        associated_types: Default::default(),
    }
}

#[test]
fn infers_vec_method_call_types() {
    let source = SourceFile::new(
        "array-method.kgr",
        r#"
fn main() -> usize {
    val values = Vec::from([1, 2]);
    val next = values.push(3);
    next.len()
}
"#,
    );
    let analyzed = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
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
        Some(foundation_vec(
            analyzed.declarations.definitions(),
            TypeId::Builtin(BuiltinType::I32)
        ))
    );
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::USize))
    );
}

#[test]
fn infers_map_method_call_types() {
    let source = SourceFile::new(
        "string-method.kgr",
        r#"use std::collections::{HashMap};

fn main(value: HashMap<String, i32>) -> usize {
    value.len()
}
"#,
    );
    let analyzed = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
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
    assert!(standard_surface::builtin_type("String").is_some());
    assert!(standard_surface::builtin_type("usize").is_some());
    assert!(standard_surface::builtin_type("str").is_none());
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("contracts.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let authoring_facts = snapshot
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let facts = authoring_facts.records().facts();
    let declarations = snapshot.declaration_snapshot();
    for (kind, arity, variants) in [
        ("Option", 1, ["Some", "None"]),
        ("Result", 2, ["Ok", "Err"]),
    ] {
        let enumeration = facts
            .aggregates
            .enumerations()
            .find(|declaration| {
                snapshot
                    .definitions()
                    .resolve(declaration.id)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.name == kind)
            })
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
    for (name, arity) in [("HashMap", 2), ("HashSet", 1)] {
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
    assert_eq!(
        declarations.files().count(),
        catalog::shared().unwrap().len() + 1
    ); // User input plus installed library modules.
    assert!(declarations.files().any(|file| {
        catalog::shared()
            .unwrap()
            .iter()
            .any(|module| file.source().module_identity() == &module.records().identity)
    }));
    assert!(surface::supports_const_type(&TypeId::Builtin(
        BuiltinType::U64
    )));
    assert!(!surface::supports_const_type(&TypeId::Builtin(
        BuiltinType::String
    )));
    assert!(surface::supports_hash_key(
        snapshot.definitions(),
        &TypeId::Builtin(BuiltinType::String)
    ));
    assert!(!surface::supports_hash_key(
        snapshot.definitions(),
        &TypeId::Builtin(BuiltinType::F64)
    ));

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
                            == FunctionImplementation::Native(NativeBinding::Entry(binding_id(
                                snapshot.definitions(),
                                binding,
                            )))
                    })
            })
            .expect("checked native function signature")
    };
    let map_get = signature("$foundation_map_get");
    let key_bounds = map_get.bounds.get(&map_get.params[1].ty).unwrap();
    for kind in [Protocol::Eq, Protocol::Hash] {
        assert!(key_bounds.iter().any(|interface| interface.declaration
            == kind.nominal(snapshot.definitions()).unwrap().declaration));
    }
    assert_eq!(map_get.params.len(), 2);
    for binding in [
        "$foundation_map_insert",
        "$foundation_list_push",
        "$foundation_cursor_next",
    ] {
        assert_eq!(
            signature(binding).implementation,
            FunctionImplementation::Native(NativeBinding::Entry(binding_id(
                snapshot.definitions(),
                binding
            )))
        );
    }
    let set = facts
        .aggregates
        .trait_(&foundation_interface(snapshot.definitions(), "Set").declaration)
        .unwrap();
    assert_eq!(
        set.methods
            .iter()
            .map(|method| method.name.as_str())
            .collect::<Vec<_>>(),
        ["len", "is_empty", "contains"]
    );
    let len = signature("$foundation_list_len");
    assert_eq!(len.params.len(), 1);
    assert_eq!(len.params[0].name, "self");
}

#[test]
fn resolves_language_builtin_type_annotations() {
    let source = SourceFile::new(
        "native-annotations.kgr",
        r#"use std::collections::{HashMap, HashSet};
use app::aliases::Pick;

fn choose(value: Pick<i32>) -> Pick<i32> { value }
fn fallible(value: Result<i32, String>) -> Result<i32, String> { value }
fn lookup(value: HashMap<String, i32>) -> HashMap<String, i32> { value }
fn unique(value: HashSet<String>) -> HashSet<String> { value }
fn sized(value: usize) -> usize { value }
fn constructed() -> Pick<i32> { Pick(7) }
"#,
    );
    let mut aliases = ModuleDecl::new(kagari_stdlib::namespaces::module("app", "aliases"));
    let context = DefinitionContext::new().unwrap();
    let mut modules = adoption::adopt(
        &foundation_catalog::shared().unwrap(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let option = context.lookup(&binding::option_declaration()).unwrap();
    let some = context
        .snapshot()
        .lookup_child(option, DefinitionKind::Variant, "Some", 0)
        .unwrap();
    aliases
        .exports
        .insert(ExportName::new(NameNamespace::Type, "Pick"), option);
    aliases
        .exports
        .insert(ExportName::new(NameNamespace::Value, "Pick"), some);
    aliases.collect_definitions(&context).unwrap();
    modules.push(Arc::new(
        DefinitionMetadata::checked(context.snapshot(), aliases, &Default::default()).unwrap(),
    ));
    let analyzed = crate::analyze_source(&source, modules)
        .expect("installed declaration analysis")
        .into_checked()
        .expect("checked installed declarations");
    let typed = &analyzed.typed;

    assert_eq!(
        typed.functions[0].return_type,
        TypeId::Enum(crate::types::NominalType {
            declaration: analyzed
                .declarations
                .definitions()
                .lookup(&binding::option_declaration())
                .unwrap(),
            arguments: vec![TypeId::Builtin(BuiltinType::I32)],
            associated_types: Default::default()
        })
    );
    assert_eq!(
        typed.functions[1].return_type,
        TypeId::Enum(crate::types::NominalType {
            declaration: analyzed
                .declarations
                .definitions()
                .lookup(&binding::result_declaration())
                .unwrap(),
            arguments: vec![
                TypeId::Builtin(BuiltinType::I32),
                TypeId::Builtin(BuiltinType::String),
            ],
            associated_types: Default::default()
        })
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
fn resolves_native_constructor_imports_facade_exports_and_function_calls() {
    let source = SourceFile::new(
        "constructor-imports.kgr",
        r#"
        pub use alloc::vec as foundation;
        use alloc::vec::Vec::new as make_list;
        fn alias() -> Vec<i32> { make_list() }
        fn qualified() -> Vec<i32> { foundation::Vec::new() }
    "#,
    );
    let analyzed = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("checked constructor imports");
    for (name, function) in [("foundation", false), ("make_list", true)] {
        let binding = analyzed
            .names
            .items
            .lookup(
                name,
                if function {
                    NameNamespace::Value
                } else {
                    NameNamespace::Type
                },
            )
            .unwrap()
            .target()
            .unwrap();
        let (unit, is_function) = match &binding {
            ResolvedName::Source(target) => {
                assert!(matches!(target.item, SourceItem::Function(_)));
                (&target.unit, true)
            }
            ResolvedName::Namespace(NamespaceId::Module(unit)) => (unit, false),
            other => panic!("source namespace or constructor: {other:?}"),
        };
        assert_eq!(unit.module, kagari_stdlib::namespaces::type_owner("Vec"));
        assert!(analyzed.names.catalog.valid(unit));
        assert_eq!(is_function, function);
    }
    let lowered = &analyzed.lowered;
    for function in &lowered.module.functions {
        let tail = lowered
            .module
            .block(function.body.unwrap())
            .tail_expr
            .unwrap();
        let call = analyzed.typed.type_table.call_resolution(tail).unwrap();
        let CallTarget::SourceFunction(target) = &call.target else {
            panic!("imported constructor");
        };
        let imported = analyzed.imported_functions.target(target).unwrap();
        assert_eq!(
            *analyzed
                .declarations
                .definitions()
                .resolve(imported.declaration)
                .unwrap()
                .module(),
            kagari_stdlib::namespaces::type_owner("Vec")
        );
        assert_eq!(imported.signature.name, "new");
        assert_eq!(call.type_arguments, [TypeId::Builtin(BuiltinType::I32)]);
        assert_eq!(
            analyzed.typed.type_table.expr_type(tail),
            Some(foundation_vec(
                analyzed.declarations.definitions(),
                TypeId::Builtin(BuiltinType::I32)
            ))
        );
    }
}

#[test]
fn type_checks_standard_methods_and_records_checked_bindings() {
    let source = SourceFile::new(
        "standard-methods.kgr",
        r#"use std::collections::{HashMap};

fn get(values: HashMap<String, i32>) -> Option<i32> {
    values.get("key")
}

fn len(value: Vec<i32>) -> usize {
    value.len()
}

fn popped(values: Vec<i32>) -> Option<i32> {
    values.pop()
}
"#,
    );
    let analyzed = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("checked installed method declarations");
    let lowered = &analyzed.lowered;
    let typed = &analyzed.typed;
    let binding = |expression| {
        let call = typed.type_table.call_resolution(expression).unwrap();
        let CallTarget::TraitMethod { method, interface } = &call.target else {
            panic!("checked trait method target: {:?}", call.target);
        };
        (
            interface.declaration,
            analyzed
                .declarations
                .definitions()
                .resolve(*method)
                .unwrap()
                .last()
                .unwrap()
                .name
                .to_owned(),
        )
    };

    let keys_tail = lowered
        .module
        .block(lowered.module.functions[0].body.unwrap())
        .tail_expr
        .expect("keys tail expr");
    assert_eq!(
        binding(keys_tail),
        (
            analyzed
                .declarations
                .definitions()
                .lookup(&library::trait_id("Map"))
                .unwrap(),
            "get".into()
        )
    );
    assert_eq!(
        typed.type_table.expr_type(keys_tail),
        Some(TypeId::Enum(crate::types::NominalType {
            declaration: analyzed
                .declarations
                .definitions()
                .lookup(&binding::option_declaration())
                .unwrap(),
            arguments: vec![TypeId::Builtin(BuiltinType::I32)],
            associated_types: Default::default()
        }))
    );

    let chars_tail = lowered
        .module
        .block(lowered.module.functions[1].body.unwrap())
        .tail_expr
        .expect("chars tail expr");
    assert_eq!(
        binding(chars_tail),
        (
            analyzed
                .declarations
                .definitions()
                .lookup(&library::trait_id("List"))
                .unwrap(),
            "len".into()
        )
    );

    let popped_tail = lowered
        .module
        .block(lowered.module.functions[2].body.unwrap())
        .tail_expr
        .expect("popped tail expr");
    assert_eq!(
        typed.type_table.expr_type(popped_tail),
        Some(TypeId::Enum(crate::types::NominalType {
            declaration: analyzed
                .declarations
                .definitions()
                .lookup(&binding::option_declaration())
                .unwrap(),
            arguments: vec![TypeId::Builtin(BuiltinType::I32)],
            associated_types: Default::default()
        }))
    );
}

#[test]
fn enforces_standard_hash_key_constraints_for_collections_and_generic_calls() {
    let lowered = common::lower_ok(
        r#"use std::collections::{HashMap, HashSet};
use std::hash::{Hash};

fn contains<K: Eq + Hash, V>(values: HashMap<K, V>, key: K) -> bool {
    values.contains_key(key)
}

fn unique<T: Eq + Hash>(values: HashSet<T>) -> usize {
    values.len()
}
"#,
    );
    crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("hash-key constrained generics should type check");

    let lowered = common::lower_ok(
        "use std::collections::{HashMap};\nfn bad(values: HashMap<f64, i32>) -> usize { values.len() }",
    );
    let diagnostics = crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect_err("f64 map keys should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::StandardConstraintNotSatisfied { type_name, constraint, .. }
                if type_name == "f64" && constraint == "Eq + Hash"
        )
    }));

    let lowered = common::lower_ok(
        "use std::collections::{HashMap};\nfn bad<K, V>(values: HashMap<K, V>) -> usize { values.len() }",
    );
    let diagnostics = crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect_err("unconstrained generic map key should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::StandardConstraintNotSatisfied { type_name, constraint, .. }
                if type_name == "K" && constraint == "Eq + Hash"
        )
    }));

    // Header checking must defer nominal proofs, then reject missing evidence
    // even when no function body constructs or accesses the declaration.
    for declaration in [
        "struct Store { val keys: HashSet<Key> }",
        "enum Store { Keys(HashSet<Key>) }",
    ] {
        let lowered = common::lower_ok(&format!(
            "use std::collections::HashSet; struct Key {{}} {declaration}"
        ));
        let diagnostics =
            crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
                .expect("installed declaration analysis")
                .into_checked()
                .expect_err("nominal keys still require explicit protocols");
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            &diagnostic.kind,
            DiagnosticKind::StandardConstraintNotSatisfied { type_name, constraint, .. }
                if type_name == "Key" && constraint == "Eq + Hash"
        )));
    }
}

#[test]
fn rejects_standard_library_invalid_arity_and_argument_types() {
    let lowered = common::lower_ok("fn bad() { Vec::push([1]); }");
    let diagnostics = crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect_err("standard call arity should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::CallArityMismatch {
                function_name: "push".to_owned(),
                expected: 2,
                found: 1,
            }
    }));

    let lowered = common::lower_ok(
        r#"use std::collections::{HashMap};

fn bad(values: HashMap<String, i32>) -> bool {
    values.contains_key(1)
}
"#,
    );
    let diagnostics = crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect_err("standard method key type should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "contains_key".to_owned(),
                parameter_name: "arg1".to_owned(),
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
    analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("standard numeric types should check");

    let lowered = common::lower_ok("fn bad(value: u32) -> u32 { -value }");
    let diagnostics = analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
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
    let context = DefinitionContext::new().unwrap();
    let lowered = common::lower_ok(r#"fn main() { print("hello"); }"#);
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    check_module(&context, &lowered, &names, None)
        .into_checked()
        .expect("print should accept str");

    let lowered = common::lower_ok("fn main() { print(1); }");
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&context, &lowered, &names, None)
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

fn binding_id(definitions: &DefinitionTable, name: &str) -> DefinitionId {
    for module in catalog::shared().unwrap() {
        for declaration in module
            .records()
            .native_declarations(module.definitions())
            .unwrap()
        {
            let CallableImplementation::Native(id) = declaration.function.implementation else {
                continue;
            };
            let view = module.definitions().resolve(id).unwrap();
            if view.last().is_some_and(|part| part.name == name) {
                return definitions
                    .lookup(&view.to_path())
                    .expect("adopted native binding");
            }
        }
    }
    panic!("installed native binding {name}")
}
