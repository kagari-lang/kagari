//! Public paths retain verified targets across context import and generation changes.

use crate::{
    Runtime,
    error::RuntimeErrorKind,
    module::{
        ExecutableModule, ModuleEpochRetention, Program, VerifiedProgram, state::StaticState,
    },
    native::function_handle::Function,
    reload::ReloadValidationError,
    test_support::update_programs::{bytecode, source_code, source_free_reload_results},
    value::Value,
};
use kagari_bytecode::{instruction::BytecodeInstruction, module::CallableTarget};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_types::{scalar::BuiltinType, ty::Ty};

use rstest::rstest;

mod named_contracts;

fn code(value: i32) -> VerifiedProgram {
    source_code(&format!(
        r#"
        mod hidden {{ pub fn calculate() -> i32 {{ {value} }} fn secret() {{ }} }}
        pub use crate::hidden::calculate as damage;
        pub use crate as again;
        fn main() {{ }}
    "#
    ))
}

#[test]
fn member_counts_distinguish_local_and_whole_program_leases() {
    let mut runtime = Runtime::default();
    let root_program = runtime.load_verified_program("exports", code(7)).unwrap();
    let root = runtime.select_program(&root_program).unwrap();
    let member = root
        .members()
        .find(|member| member.key() != root.key())
        .unwrap();
    let whole = runtime
        .retain_program(&member, ModuleEpochRetention::CompiledArtifact)
        .unwrap();
    let clone = whole.clone();
    let local = runtime
        .retain_module(&member, ModuleEpochRetention::RuntimeValue)
        .unwrap();
    for loaded in root.members() {
        let counts = runtime.modules().retention_counts(loaded.key());
        assert_eq!(counts.compiled_artifacts, 1);
        assert_eq!(
            counts.runtime_values,
            usize::from(loaded.key() == member.key())
        );
    }
    drop(whole);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(root.key())
            .compiled_artifacts,
        1
    );
    drop(clone);
    assert_eq!(runtime.modules().retention_counts(root.key()).total(), 0);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(member.key())
            .runtime_values,
        1
    );
    drop(local);
    assert!(
        root.members()
            .all(|member| runtime.modules().retention_counts(member.key()).total() == 0)
    );
}

#[test]
fn public_paths_remain_rooted_and_generation_pinned_after_identity_import() {
    let original = code(7);
    let target = original.exported_function("damage").unwrap();
    assert_eq!(original.exported_function("again::damage"), Some(target));
    assert!(original.exported_function("main").is_none());
    assert!(original.entry_function().is_some());
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    let loaded_program = first
        .load_verified_program("exports", original.clone())
        .unwrap();
    let loaded = first.select_program(&loaded_program).unwrap();
    let foreign_program = second
        .load_verified_program("exports", original.clone())
        .unwrap();
    let foreign = second.select_program(&foreign_program).unwrap();
    assert_ne!(loaded.definitions().id(), foreign.definitions().id());
    assert!(
        loaded
            .verified_program()
            .same_version(foreign.verified_program())
    );
    for root in [&loaded, &foreign] {
        let (owner, function) = root.exported_function("damage").unwrap();
        assert_eq!(owner.bytecode.identity.path, ["hidden"]);
        assert_eq!(owner.bytecode.functions[function.index()].name, "calculate");
        let declaration = owner.bytecode.functions[function.index()]
            .identity
            .as_ref()
            .unwrap()
            .declaration;
        assert_eq!(
            owner.definitions().resolve(declaration).unwrap().module(),
            &owner.bytecode.identity
        );
        assert!(root.exported_function("hidden::calculate").is_none());
        assert!(owner.exported_function("secret").is_none());
        assert!(owner.exported_function("calculate").is_none());
        assert_eq!(
            owner.exported_function("again::damage").unwrap().0.key(),
            owner.key()
        );
        let (entry, function) = owner.entry_function().unwrap();
        assert_eq!(entry.key(), root.key());
        assert_eq!(entry.bytecode.functions[function.index()].name, "main");
    }
    let (old_target, old_function) = loaded.exported_function("damage").unwrap();
    let _lease = first
        .retain_module(&old_target, ModuleEpochRetention::ActiveCall)
        .unwrap();
    let staged = first
        .stage_reload_verified_program(&loaded_program, None, code(9))
        .unwrap();
    first.publish_staged_reload(staged).unwrap();

    let published = first.select_program(&loaded_program).unwrap();
    let (new_target, new_function) = published.exported_function("damage").unwrap();
    assert_ne!(old_target.key(), new_target.key());
    assert_ne!(
        loaded.program_fingerprint(),
        published.program_fingerprint()
    );
    assert_eq!(
        loaded.exported_function("damage").unwrap().0.key(),
        old_target.key()
    );
    for (owner, function, expected) in [
        (&old_target, old_function, 7),
        (&new_target, new_function, 9),
    ] {
        let BytecodeInstruction::LoadConst { constant, .. } =
            owner.bytecode.functions[function.index()].instructions[0]
        else {
            panic!("constant-return fixture");
        };
        assert_eq!(
            first.read_constant(owner, constant).unwrap(),
            Value::I32(expected)
        );
    }
    first.validate_executable_module(&old_target).unwrap();
    first.validate_executable_module(&new_target).unwrap();
    assert!(second.validate_executable_module(&old_target).is_err());
}

const RELATIVE_BINDINGS: &str = r#"
    static var STATE: i32 = initialize();
    fn initialize() -> i32 { 7 }
    pub fn value() -> i32 { 7 }
    pub struct Object { pub val root: bool }
    mod implementation {
        pub struct Object { pub var value: i32 }
        pub struct Cell<T> { pub var value: T }
        pub struct Unused<T> { pub val value: T }
        pub type IntCell = Cell<i32>;
        pub type Open<T> = Cell<T>;
        pub type Scalar = i32;
        pub enum Event { Empty }
        pub trait Reader { fn read(self) -> i32; }
        pub fn value() -> i32 { 13 }
        pub fn other() -> i32 { 19 }
        fn secret() -> i32 { 99 }
    }
    pub mod api {
        pub use crate::implementation::{Object, Cell, Unused, IntCell, Open, Scalar, Event, Reader, value, other};
        pub use crate::implementation::Object as Reexport;
        pub type Alias = crate::implementation::Object;
        pub use crate::implementation::value as selected;
        pub use crate::api as again;
    }
    pub mod facade { pub use crate::api as nested; }
    fn main() { STATE = 99; }
"#;

fn relative_binding_code(source: &str) -> VerifiedProgram {
    let (definitions, mut code) = bytecode(source, None).into_parts();
    let root = code.modules[code.root.index()].identity.clone();
    let mut slotless = root.clone();
    slotless.path.push("slotless".into());
    let api = code.exports.namespaces[&root].modules["api"].clone();
    // Source lowering currently emits slots even for re-export-only modules.
    // Add a legal graph-only namespace while retaining all executable evidence;
    // normal verification must admit it before either binder sees the fixture.
    code.exports
        .namespaces
        .insert(slotless.clone(), code.exports.namespaces[&api].clone());
    code.exports
        .namespaces
        .get_mut(&root)
        .unwrap()
        .modules
        .insert("slotless".into(), slotless);
    let metadata = DefinitionMetadata::checked(definitions, code, &Default::default()).unwrap();
    VerifiedProgram::new(metadata, &DefinitionContext::new().unwrap()).unwrap()
}

fn relative_binding_fixture() -> (Runtime, Program) {
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("relative", relative_binding_code(RELATIVE_BINDINGS))
        .unwrap();
    (runtime, program)
}

#[test]
fn retained_namespace_bindings_follow_current_declarations_not_retargeted_aliases() {
    let (runtime, program) = relative_binding_fixture();
    let root = program.root();
    let api = program.module("slotless").unwrap();
    let selected = runtime.bind_function::<(), i32>(&api, "selected").unwrap();
    let child = runtime
        .bind_function::<(), i32>(&api, "again::value")
        .unwrap();
    let root_value = runtime.bind_function::<(), i32>(&root, "value").unwrap();
    let declaration = |function: &Function<(), i32>| {
        function
            .call_route()
            .unwrap()
            .declaration()
            .unwrap()
            .unwrap()
    };
    assert_eq!(declaration(&selected), declaration(&child));
    assert_ne!(declaration(&root_value), declaration(&child));
    assert_eq!(
        declaration(
            &runtime
                .bind_function::<(), i32>(&root, "facade::nested::value")
                .unwrap()
        ),
        declaration(&child)
    );
    // Exporting an item never grants a Module for its private implementation owner.
    for path in ["implementation", "facade::nested::implementation"] {
        let error = root.module(path).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
        assert!(error.message().contains("public path not found"));
    }
    let replacement = RELATIVE_BINDINGS.replace("{ 13 }", "{ 17 }").replace(
        "implementation::value as selected",
        "implementation::other as selected",
    );
    let staged = runtime
        .stage_reload_verified_program(&program, None, relative_binding_code(&replacement))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    let fresh = runtime.bind_function::<(), i32>(&api, "selected").unwrap();
    assert_ne!(declaration(&selected), declaration(&fresh));
    assert_eq!(declaration(&child), declaration(&selected));
    for (function, expected) in [(&selected, 17), (&fresh, 19), (&root_value, 7)] {
        let target = runtime
            .select_call_route(function.call_route().unwrap())
            .unwrap();
        let CallableTarget::Script(function) = target.target() else {
            panic!("script fixture");
        };
        let BytecodeInstruction::LoadConst { constant, .. } =
            target.owner().bytecode.functions[function.index()].instructions[0]
        else {
            panic!("constant-return fixture");
        };
        assert_eq!(
            runtime.read_constant(target.owner(), constant).unwrap(),
            Value::I32(expected)
        );
    }
}

#[test]
fn relative_object_aliases_share_exact_layout_and_reject_unsupported_leaves_without_effects() {
    let (runtime, program) = relative_binding_fixture();
    let root = program.root();
    let api = program.module("slotless").unwrap();
    let executable = runtime.select_program(&program).unwrap();
    assert!(
        !executable
            .members()
            .any(|member| member.bytecode.identity.path == ["slotless"])
    );
    let object = runtime.bind_type(&api, "Object", &[]).unwrap();
    runtime.bind_field::<i32>(&object, "value").unwrap();
    let root_object = runtime.bind_type(&root, "Object", &[]).unwrap();
    runtime.bind_field::<bool>(&root_object, "root").unwrap();
    assert_ne!(
        object.type_argument().ty(),
        root_object.type_argument().ty()
    );
    let nested = program.module("facade").unwrap().module("nested").unwrap();
    let nested_object = runtime.bind_type(&nested, "Object", &[]).unwrap();
    assert_eq!(
        nested_object.type_argument().ty(),
        object.type_argument().ty()
    );
    // No source expression constructs Cell<i32>; the closed export roots its layout.
    let closed = runtime.bind_type(&api, "IntCell", &[]).unwrap();
    runtime.bind_field::<i32>(&closed, "value").unwrap();
    let arguments = runtime
        .type_arguments(&executable, None, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap();
    let direct = runtime.bind_type(&api, "Cell", &arguments).unwrap();
    assert_eq!(closed.type_argument().ty(), direct.type_argument().ty());
    assert_eq!(closed.owner().key(), direct.owner().key());
    let Ty::Struct(nominal) = closed.type_argument().ty() else {
        panic!("closed object alias");
    };
    let declaration = closed
        .owner()
        .definition(nominal.declaration)
        .unwrap()
        .to_path();
    let advanced = runtime
        .bind_type_declaration(&api, &declaration, &arguments)
        .unwrap();
    assert_eq!(closed.type_argument().ty(), advanced.type_argument().ty());
    assert_eq!(closed.owner().key(), advanced.owner().key());
    let error = runtime.bind_type(&api, "Unused", &arguments).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(
        error
            .message()
            .contains("missing executable struct application")
    );
    for member in executable.members() {
        assert!(
            runtime
                .module_instance_snapshot(&member)
                .unwrap()
                .module_slots
                .iter()
                .all(|state| matches!(state, StaticState::Uninitialized(_)))
        );
    }
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

fn type_reload_source(object: &str, cell: &str, open: &str) -> String {
    format!(
        r#"
        mod implementation {{
            pub struct A {{ }}
            pub struct B {{ }}
            pub struct Cell<T> {{ val value: T }}
            pub type Alias = A;
            pub type IntCell = Cell<i32>;
            pub type Via = IntCell;
            pub type Open<T> = A;
            pub type OtherOpen<T> = B;
            pub type Pair<T, U> = A;
        }}
        pub mod api {{
            {object}
            pub type Cell = {cell};
            pub use crate::implementation::{open} as Open;
            pub type Scalar = (i32, [i32]);
            pub use crate::api as again;
        }}
        static var STATE: i32 = initialize();
        fn initialize() -> i32 {{ 7 }}
        pub fn value() -> i32 {{ 7 }}
        fn main() {{ STATE = 99; }}
        "#
    )
}

#[test]
fn reload_normalizes_type_aliases_and_reexports_before_publication() {
    let original = type_reload_source(
        "pub use crate::implementation::A as Object;",
        "crate::implementation::IntCell",
        "Open",
    );
    let equivalent = type_reload_source(
        "pub type Object = crate::implementation::Alias;",
        "crate::implementation::Via",
        "OtherOpen",
    );
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(&original))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    assert_source_free_reload(&loaded, &equivalent, true);
    let candidate = runtime
        .stage_reload_verified_program(&loaded_program, None, source_code(&equivalent))
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();

    let published = runtime.select_program(&loaded_program).unwrap();
    assert_ne!(published.key(), loaded.key());
    assert_eq!(
        runtime.select_program(&loaded_program).unwrap().key(),
        published.key()
    );
    assert!(matches!(
        runtime
            .module_instance_snapshot(&published)
            .unwrap()
            .module_slots
            .as_slice(),
        [StaticState::Uninitialized(_)]
    ));
    // Public path equivalence does not authorize deleting the newly published
    // authored alias. Keep its declaration while changing to an equivalent target.
    for result in source_free_reload_results(&published, &original) {
        assert!(matches!(
            result,
            Err(ReloadValidationError::DeclarationContractMismatch { .. })
        ));
    }
    let preserved =
        equivalent.replace("crate::implementation::Alias;", "crate::implementation::A;");
    let candidate = runtime
        .stage_reload_verified_program(&loaded_program, None, source_code(&preserved))
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
}

#[rstest]
#[case::nominal_target(|original: &str| original.replace("implementation::A as Object", "implementation::B as Object"))]
#[case::applied_argument(|original: &str| original.replace("implementation::IntCell;", "implementation::Cell<bool>;"))]
#[case::open_arity(|original: &str| original.replace(
            "implementation::Open as Open",
            "implementation::Pair as Open",
        ))]
#[case::open_to_concrete(|original: &str| original.replace("implementation::Open as Open", "implementation::A as Open"))]
#[case::nested_scalar(|original: &str| original.replace(
            "pub type Scalar = (i32, [i32]);",
            "pub type Scalar = (i32, [bool]);",
        ))]
#[case::removed_path(|original: &str| original.replace("as Object;", "as Renamed;"))]
fn reload_rejects_changed_type_targets_arguments_and_marker_classes_without_effects(
    #[case] candidate: fn(&str) -> String,
) {
    let original = type_reload_source(
        "pub use crate::implementation::A as Object;",
        "crate::implementation::IntCell",
        "Open",
    );
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(&original))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let (owner, function) = loaded.exported_function("value").unwrap();
    let source = candidate(&original);
    assert_source_free_reload(&loaded, &source, false);
    assert!(matches!(
        runtime.stage_reload_verified_program(&loaded_program, None, source_code(&source)),
        Err(ReloadValidationError::PublicExportMismatch)
    ));
    let latest = runtime.select_program(&loaded_program).unwrap();
    assert_eq!(latest.key(), loaded.key());
    assert_eq!(latest.program_fingerprint(), loaded.program_fingerprint());
    let (still_owner, still_function) = latest.exported_function("value").unwrap();
    assert_eq!(still_owner.key(), owner.key());
    assert_eq!(still_function, function);
    assert!(matches!(
        runtime
            .module_instance_snapshot(&latest)
            .unwrap()
            .module_slots
            .as_slice(),
        [StaticState::Uninitialized(_)]
    ));
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

fn reload_source(next: &str, target: &str, alias: &str, nominal: &str, entry: &str) -> String {
    format!(
        r#"
        pub mod one {{ pub fn run() -> i32 {{ 1 }} pub use crate::{next} as next; }}
        pub mod two {{ pub fn run() -> i32 {{ 2 }} pub use crate::one as next; }}
        mod implementation {{
            pub struct A {{ }}
            pub struct B {{ }}
            pub fn a() -> A {{ A {{ }} }}
            pub fn b() -> B {{ B {{ }} }}
            pub fn integer() -> i32 {{ 3 }}
            pub fn replacement() -> i32 {{ 4 }}
            pub fn boolean() -> bool {{ true }}
        }}
        pub use crate::one as api;
        pub use crate::implementation::{target} as {alias};
        pub use crate::implementation::{nominal} as make;
        {entry}
    "#
    )
}

#[rstest]
#[case::function_rename(reload_source("one", "integer", "renamed", "a", "fn main() {}"))]
#[case::function_signature(reload_source("one", "boolean", "value", "a", "fn main() {}"))]
#[case::nominal_result(reload_source("one", "integer", "value", "b", "fn main() {}"))]
#[case::removed_entry(reload_source("one", "integer", "value", "a", ""))]
#[case::entry_signature(reload_source("one", "integer", "value", "a", "fn main() -> i32 { 0 }"))]
#[case::module_rename(reload_source("one", "integer", "value", "a", "fn main() {}")
            .replace("as api", "as renamed"))]
fn reload_compares_public_path_contracts_without_expanding_alias_cycles(#[case] source: String) {
    let original = source_code(&reload_source(
        "one",
        "integer",
        "value",
        "a",
        "fn main() {}",
    ));
    let mut runtime = Runtime::default();
    let loaded_program = runtime.load_verified_program("exports", original).unwrap();
    assert!(matches!(
        runtime.stage_reload_verified_program(&loaded_program, None, source_code(&source)),
        Err(ReloadValidationError::PublicExportMismatch)
    ));
}

#[test]
fn reload_accepts_equivalent_alias_cycles_and_same_signature_retargeting() {
    let original = source_code(&reload_source(
        "one",
        "integer",
        "value",
        "a",
        "fn main() {}",
    ));
    let mut runtime = Runtime::default();
    let loaded_program = runtime.load_verified_program("exports", original).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();

    // One-state and two-state namespace cycles expose the same path language.
    // Retargeting a public leaf to a same-signature declaration is compatible.
    let candidate = source_code(&reload_source(
        "two",
        "replacement",
        "value",
        "a",
        "fn main() {}",
    ));
    let old = loaded.exported_function("api::next::run").unwrap().0;
    let _lease = runtime
        .retain_module(&old, ModuleEpochRetention::ActiveCall)
        .unwrap();
    let staged = runtime
        .stage_reload_verified_program(&loaded_program, None, candidate)
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let published = runtime.select_program(&loaded_program).unwrap();
    assert_eq!(old.bytecode.identity.path, ["one"]);
    assert_eq!(
        published
            .exported_function("api::next::run")
            .unwrap()
            .0
            .bytecode
            .identity
            .path,
        ["two"]
    );
    assert_eq!(
        loaded.exported_function("api::next::run").unwrap().0.key(),
        old.key()
    );
    let (member, function) = published.exported_function("value").unwrap();
    assert_eq!(
        member.bytecode.functions[function.index()].name,
        "replacement"
    );
}

fn assert_source_free_reload(loaded: &ExecutableModule, source: &str, compatible: bool) {
    let [raw, artifact] = source_free_reload_results(loaded, source);
    if compatible {
        raw.unwrap();
        artifact.unwrap();
    } else {
        assert!(matches!(
            raw,
            Err(ReloadValidationError::PublicExportMismatch)
        ));
        assert!(matches!(
            artifact,
            Err(ReloadValidationError::PublicExportMismatch)
        ));
    }
}

#[test]
fn reload_accepts_added_public_paths_but_preserves_old_paths() {
    let source = r#"
        pub mod one { pub fn run() -> i32 { 1 } pub struct Object {} }
        pub mod two { pub fn run() -> i32 { 2 } pub use crate::one::Object; }
        pub use crate::one as api;
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let addition = format!("{source} pub use crate::two as extra;");
    assert_source_free_reload(&loaded, &addition, true);
    let staged = runtime
        .stage_reload_verified_program(&loaded_program, None, source_code(&addition))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let published = runtime.select_program(&loaded_program).unwrap();
    assert!(published.exported_function("extra::run").is_some());
    assert!(published.exported_function("api::run").is_some());

    // An added alias cannot replace an old public spelling, even when the old
    // target is still exposed under its canonical name and another new alias.
    let removed = addition.replace("pub use crate::one as api;", "");
    assert_source_free_reload(&loaded, &removed, false);
    assert!(matches!(
        runtime.stage_reload_verified_program(&loaded_program, None, source_code(&removed),),
        Err(ReloadValidationError::PublicExportMismatch)
    ));
}

#[rstest]
#[case::removed_struct(|source: &str| source.replace("struct Hidden<T> { val value: T }", ""), "Hidden")]
#[case::field_mutability(|source: &str| source.replace("val value: T", "var value: T"), "Hidden")]
#[case::field_type(|source: &str| source.replace("val value: T", "val value: (T, i32)"), "Hidden")]
#[case::generic_arity(|source: &str| source.replace("Hidden<T>", "Hidden<T, U>"), "Hidden")]
#[case::removed_enum(|source: &str| source.replace("enum Secret<T> { Value(T), Empty }", ""), "Secret")]
#[case::variant_payload(|source: &str| source.replace("Value(T), Empty", "Value(T, i32), Empty"), "Secret")]
#[case::added_variant(|source: &str| source.replace("Value(T), Empty", "Value(T), Empty, Added"), "Secret")]
#[case::removed_trait(|source: &str| source.replace("trait Internal { fn read(self) -> i32; }", ""), "Internal::read")]
#[case::trait_signature(|source: &str| source.replace("fn read(self) -> i32", "fn read(self) -> bool"), "Internal::read")]
fn reload_preserves_unused_private_nominal_and_trait_contracts_source_free(
    #[case] candidate: fn(&str) -> String,
    #[case] name: &str,
) {
    let source = r#"
        struct Hidden<T> { val value: T }
        enum Secret<T> { Value(T), Empty }
        trait Internal { fn read(self) -> i32; }
        pub fn value() -> i32 { 7 }
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let root = loaded.bytecode.as_ref();
    assert_eq!(root.private_types.len(), 2);
    assert_eq!(root.trait_contracts.len(), 1);
    assert!(root.structures.is_empty());
    assert!(root.enumerations.is_empty());
    let candidate = candidate(source);
    for result in source_free_reload_results(&loaded, &candidate)
        .into_iter()
        .chain([runtime
            .stage_reload_verified_program(&loaded_program, None, source_code(&candidate))
            .map(|_| ())])
    {
        let Err(ReloadValidationError::DeclarationContractMismatch { declaration }) = result else {
            panic!("expected frozen declaration rejection for {name}: {result:?}");
        };
        assert!(declaration.ends_with(name), "{declaration}");
    }
    let latest = runtime.select_program(&loaded_program).unwrap();
    assert_eq!(latest.key(), loaded.key());
    assert_eq!(latest.program_fingerprint(), loaded.program_fingerprint());
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn reload_accepts_reordered_private_contracts_and_binder_renaming() {
    let source = r#"
        struct Hidden<T> { val value: T }
        enum Secret<T> { Value(T), Empty }
        trait Internal { fn read(self) -> i32; }
        pub fn value() -> i32 { 7 }
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let root = loaded.bytecode.as_ref();
    assert_eq!(root.private_types.len(), 2);
    assert_eq!(root.trait_contracts.len(), 1);
    assert!(root.structures.is_empty());
    assert!(root.enumerations.is_empty());

    // Source order, binder spelling and fresh private declarations do not alter
    // the old contracts, even though each compilation has an independent ID scope.
    let candidate = r#"
        trait Internal { fn read(self) -> i32; }
        enum Secret<U> { Value(U), Empty }
        struct Added {}
        struct Hidden<U> { val value: U }
        pub fn value() -> i32 { 7 }
        fn main() {}
    "#;
    assert_source_free_reload(&loaded, candidate, true);
    let staged = runtime
        .stage_reload_verified_program(&loaded_program, None, source_code(candidate))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
}

#[rstest]
#[case::compatible_alias(true)]
#[case::removed_alias(false)]
fn source_free_reload_validators_use_the_same_public_contract(#[case] compatible: bool) {
    let original = source_code(&reload_source(
        "one",
        "integer",
        "value",
        "a",
        "fn main() {}",
    ));
    let mut runtime = Runtime::default();
    let loaded_program = runtime.load_verified_program("exports", original).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let alias = if compatible { "value" } else { "renamed" };
    assert_source_free_reload(
        &loaded,
        &reload_source("two", "replacement", alias, "a", "fn main() {}"),
        compatible,
    );
}

#[rstest]
#[case::invalid_path("Object::", false, "invalid public path")]
#[case::private("secret", false, "public path not found")]
#[case::missing("missing", false, "public path not found")]
#[case::function("value", false, "public path has wrong target kind")]
#[case::module("again", false, "public path has wrong target kind")]
#[case::scalar("Scalar", false, "type is not a script object")]
#[case::enum_leaf("Event", false, "type is not a script object")]
#[case::trait_leaf("Reader", false, "type is not a script object")]
#[case::open_alias("Open", false, "open generic alias is not supported for host binding")]
#[case::applied_open_alias("Open", true, "open generic alias is not supported for host binding")]
#[case::missing_arguments("Cell", false, "type argument count mismatch")]
#[case::closed_alias_arguments("IntCell", true, "type argument count mismatch")]
#[case::nongeneric_arguments("Object", true, "type argument count mismatch")]
fn relative_object_binding_rejections_preserve_state_and_heap(
    #[case] path: &str,
    #[case] with_arguments: bool,
    #[case] detail: &str,
) {
    let (runtime, program) = relative_binding_fixture();
    let api = program.module("slotless").unwrap();
    let executable = runtime.select_program(&program).unwrap();
    let arguments = runtime
        .type_arguments(&executable, None, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap();
    let arguments = if with_arguments {
        arguments.as_slice()
    } else {
        &[]
    };
    let error = runtime.bind_type(&api, path, arguments).unwrap_err();
    assert_eq!(
        error.kind(),
        RuntimeErrorKind::TypedPathValidation,
        "{path}: {error}"
    );
    assert!(error.message().contains(detail), "{path}: {error}");
    for member in executable.members() {
        assert!(
            runtime
                .module_instance_snapshot(&member)
                .unwrap()
                .module_slots
                .iter()
                .all(|state| matches!(state, StaticState::Uninitialized(_)))
        );
    }
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[rstest]
#[case::reexport("Reexport")]
#[case::closed_alias("Alias")]
#[case::cyclic_namespace("again::Object")]
fn relative_object_alias_paths_retain_the_same_nominal_layout(#[case] path: &str) {
    let (runtime, program) = relative_binding_fixture();
    let api = program.module("slotless").unwrap();
    let object = runtime.bind_type(&api, "Object", &[]).unwrap();
    let alias = runtime.bind_type(&api, path, &[]).unwrap();
    assert_eq!(alias.type_argument().ty(), object.type_argument().ty());
    assert_eq!(alias.owner().key(), object.owner().key());
    runtime.bind_field::<i32>(&alias, "value").unwrap();
}
