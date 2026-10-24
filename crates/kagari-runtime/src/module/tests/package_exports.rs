//! Public paths retain verified targets across context import and generation changes.

use crate::{
    Runtime,
    module::{LoadedModule, ModuleEpochRetention, VerifiedProgram, state::StaticState},
    reload::{
        ReloadValidationError, validate_reload_artifact_candidate, validate_reload_candidate,
    },
    value::Value,
};
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::named::NamedContract;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use programs::{bytecode, source_code, source_free_reload_results};

// Compile the same source/artifact helpers in unit and public integration tests.
#[path = "../../../tests/support/update_programs.rs"]
mod programs;

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
    let root = runtime.load_verified_program("exports", code(7)).unwrap();
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
    let loaded = first
        .load_verified_program("exports", original.clone())
        .unwrap();
    let foreign = second
        .load_verified_program("exports", original.clone())
        .unwrap();
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
        .stage_reload_verified_program(&loaded, "exports", code(9))
        .unwrap();
    let published = first.publish_staged_reload(staged).unwrap();
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
    first.validate_loaded_module(&old_target).unwrap();
    first.validate_loaded_module(&new_target).unwrap();
    assert!(second.validate_loaded_module(&old_target).is_err());
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
    let loaded = runtime
        .load_verified_program("exports", source_code(&original))
        .unwrap();
    assert_source_free_reload(&loaded, &equivalent, true);
    let candidate = runtime
        .stage_reload_verified_program(&loaded, "exports", source_code(&equivalent))
        .unwrap();
    let published = runtime.publish_staged_reload(candidate).unwrap();
    assert_ne!(published.key(), loaded.key());
    assert_eq!(
        runtime.modules().latest("exports").unwrap().key(),
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
        .stage_reload_verified_program(&published, "exports", source_code(&preserved))
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
}

#[test]
fn reload_rejects_changed_type_targets_arguments_and_marker_classes_without_effects() {
    let original = type_reload_source(
        "pub use crate::implementation::A as Object;",
        "crate::implementation::IntCell",
        "Open",
    );
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program("exports", source_code(&original))
        .unwrap();
    let (owner, function) = loaded.exported_function("value").unwrap();
    for source in [
        // Structurally identical nominal declarations are not interchangeable.
        original.replace("implementation::A as Object", "implementation::B as Object"),
        original.replace("implementation::IntCell;", "implementation::Cell<bool>;"),
        original.replace(
            "implementation::Open as Open",
            "implementation::Pair as Open",
        ),
        original.replace("implementation::Open as Open", "implementation::A as Open"),
        original.replace(
            "pub type Scalar = (i32, [i32]);",
            "pub type Scalar = (i32, [bool]);",
        ),
        original.replace("as Object;", "as Renamed;"),
    ] {
        assert_source_free_reload(&loaded, &source, false);
        assert!(matches!(
            runtime.stage_reload_verified_program(&loaded, "exports", source_code(&source)),
            Err(ReloadValidationError::PublicExportMismatch)
        ));
        let latest = runtime.modules().latest("exports").unwrap();
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

#[test]
fn reload_compares_public_path_contracts_without_expanding_alias_cycles() {
    let original = source_code(&reload_source(
        "one",
        "integer",
        "value",
        "a",
        "fn main() {}",
    ));
    let mut runtime = Runtime::default();
    let loaded = runtime.load_verified_program("exports", original).unwrap();
    for source in [
        reload_source("one", "integer", "renamed", "a", "fn main() {}"),
        reload_source("one", "boolean", "value", "a", "fn main() {}"),
        reload_source("one", "integer", "value", "b", "fn main() {}"),
        reload_source("one", "integer", "value", "a", ""),
        reload_source("one", "integer", "value", "a", "fn main() -> i32 { 0 }"),
        reload_source("one", "integer", "value", "a", "fn main() {}")
            .replace("as api", "as renamed"),
    ] {
        assert!(matches!(
            runtime.stage_reload_verified_program(&loaded, "exports", source_code(&source)),
            Err(ReloadValidationError::PublicExportMismatch)
        ));
    }
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
        .stage_reload_verified_program(&loaded, "exports", candidate)
        .unwrap();
    let published = runtime.publish_staged_reload(staged).unwrap();
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

fn assert_source_free_reload(loaded: &LoadedModule, source: &str, compatible: bool) {
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
    let loaded = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let addition = format!("{source} pub use crate::two as extra;");
    assert_source_free_reload(&loaded, &addition, true);
    let staged = runtime
        .stage_reload_verified_program(&loaded, "exports", source_code(&addition))
        .unwrap();
    let published = runtime.publish_staged_reload(staged).unwrap();
    assert!(published.exported_function("extra::run").is_some());
    assert!(published.exported_function("api::run").is_some());

    // An added alias cannot replace an old public spelling, even when the old
    // target is still exposed under its canonical name and another new alias.
    let removed = addition.replace("pub use crate::one as api;", "");
    assert_source_free_reload(&loaded, &removed, false);
    assert!(matches!(
        runtime.stage_reload_verified_program(&published, "exports", source_code(&removed),),
        Err(ReloadValidationError::PublicExportMismatch)
    ));
}

#[test]
fn reload_preserves_unused_private_nominal_and_trait_contracts_source_free() {
    let source = r#"
        struct Hidden<T> { val value: T }
        enum Secret<T> { Value(T), Empty }
        trait Internal { fn read(self) -> i32; }
        pub fn value() -> i32 { 7 }
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let root = loaded.bytecode.as_ref();
    assert_eq!(root.private_types.len(), 2);
    assert_eq!(root.trait_contracts.len(), 1);
    assert!(root.structures.is_empty());
    assert!(root.enumerations.is_empty());

    for (candidate, name) in [
        (
            source.replace("struct Hidden<T> { val value: T }", ""),
            "Hidden",
        ),
        (source.replace("val value: T", "var value: T"), "Hidden"),
        (
            source.replace("val value: T", "val value: (T, i32)"),
            "Hidden",
        ),
        (source.replace("Hidden<T>", "Hidden<T, U>"), "Hidden"),
        (
            source.replace("enum Secret<T> { Value(T), Empty }", ""),
            "Secret",
        ),
        (
            source.replace("Value(T), Empty", "Value(T, i32), Empty"),
            "Secret",
        ),
        (
            source.replace("Value(T), Empty", "Value(T), Empty, Added"),
            "Secret",
        ),
        (
            source.replace("trait Internal { fn read(self) -> i32; }", ""),
            "Internal::read",
        ),
        (
            source.replace("fn read(self) -> i32", "fn read(self) -> bool"),
            "Internal::read",
        ),
    ] {
        for result in source_free_reload_results(&loaded, &candidate)
            .into_iter()
            .chain([runtime
                .stage_reload_verified_program(&loaded, "exports", source_code(&candidate))
                .map(|_| ())])
        {
            let Err(ReloadValidationError::DeclarationContractMismatch { declaration }) = result
            else {
                panic!("expected frozen declaration rejection for {name}: {result:?}");
            };
            assert!(declaration.ends_with(name), "{declaration}");
        }
        let latest = runtime.modules().latest("exports").unwrap();
        assert_eq!(latest.key(), loaded.key());
        assert_eq!(latest.program_fingerprint(), loaded.program_fingerprint());
        assert_eq!(runtime.resources().counters().peak_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
    }

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
        .stage_reload_verified_program(&loaded, "exports", source_code(candidate))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
}

const NAMED_RELOAD_SOURCE: &str = r#"
    trait Tag {}
    struct Old { val n: i32 }
    struct Wrapper<T> { val value: T }
    impl<T: Tag> Wrapper<T> { fn unwrap(self) -> T { self.value } }
    fn unused<T>(x: T) -> T { x }
    const SECRET: i32 = 7;
    type Alias<T> = (T, i32);
    impl Old {
        fn hidden(self) -> i32 { self.n }
        pub fn read(self) -> i32 { self.n }
    }
    impl Tag for Old {}
    fn main() -> i32 { val value = Old { n: SECRET }; value.read() }
"#;

#[test]
fn named_inventory_allows_additions_and_matches_semantic_impl_owners() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program("exports", source_code(NAMED_RELOAD_SOURCE))
        .unwrap();
    let candidate = NAMED_RELOAD_SOURCE
        .replace("impl Tag for Old {}", "")
        .replace("impl Old {", "impl Tag for Old {} impl Old {")
        .replace("fn unused<T>(x: T) -> T", "fn unused<U>(x: U) -> U")
        .replace(
            "impl<T: Tag> Wrapper<T> { fn unwrap(self) -> T",
            "impl<U: Tag> Wrapper<U> { fn unwrap(self) -> U",
        );
    let candidate = format!(
        r#"{candidate}
        impl Old {{ pub fn added(self) -> i32 {{ self.n + 1 }} }}
        pub mod extra {{
            pub struct New<T> {{ val value: T }}
            impl<T> crate::Tag for New<T> {{}}
            pub fn added() -> i32 {{ 9 }}
        }}
    "#
    );
    for result in source_free_reload_results(&loaded, &candidate) {
        result.unwrap();
    }
    let staged = runtime
        .stage_reload_verified_program(&loaded, "exports", source_code(&candidate))
        .unwrap();
    let published = runtime.publish_staged_reload(staged).unwrap();
    assert!(published.exported_function("extra::added").is_some());
    assert!(loaded.exported_function("extra::added").is_none());
    assert!(published.members().count() > loaded.members().count());
}

#[test]
fn named_inventory_freezes_unused_declarations_and_restricts_new_impl_domains() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program("exports", source_code(NAMED_RELOAD_SOURCE))
        .unwrap();
    for candidate in [
        NAMED_RELOAD_SOURCE.replace("fn unused<T>(x: T) -> T { x }", ""),
        NAMED_RELOAD_SOURCE.replace(
            "fn unused<T>(x: T) -> T { x }",
            "fn unused<T>(x: T) -> (T, T) { (x, x) }",
        ),
        NAMED_RELOAD_SOURCE.replace("fn unused<T>(x: T)", "fn unused<T: Tag>(x: T)"),
        NAMED_RELOAD_SOURCE.replace("type Alias<T>", "type Alias<T: Tag>"),
        NAMED_RELOAD_SOURCE.replace("type Alias<T> = (T, i32);", ""),
        NAMED_RELOAD_SOURCE.replace("type Alias<T> = (T, i32);", "type Alias<T> = (T, bool);"),
        NAMED_RELOAD_SOURCE.replace("SECRET: i32 = 7", "SECRET: i32 = 8"),
        NAMED_RELOAD_SOURCE.replace(
            "fn hidden(self) -> i32 { self.n }",
            "fn hidden(self) -> bool { true }",
        ),
        NAMED_RELOAD_SOURCE.replace("impl Tag for Old {}", ""),
        format!("{NAMED_RELOAD_SOURCE} impl Tag for i32 {{}}"),
        format!("{NAMED_RELOAD_SOURCE} trait Other {{}} impl Other for Old {{}}"),
    ] {
        for result in source_free_reload_results(&loaded, &candidate)
            .into_iter()
            .chain([runtime
                .stage_reload_verified_program(&loaded, "exports", source_code(&candidate))
                .map(|_| ())])
        {
            assert!(
                matches!(
                    result,
                    Err(ReloadValidationError::DeclarationContractMismatch { .. })
                ),
                "{result:?}"
            );
        }
        assert_eq!(
            runtime.modules().latest("exports").unwrap().key(),
            loaded.key()
        );
        assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    }
}

#[test]
fn source_free_named_inventory_rejects_forged_binders_signatures_and_semantic_duplicates() {
    let context = DefinitionContext::new().unwrap();
    let metadata = bytecode(
        &format!(
            r#"{NAMED_RELOAD_SOURCE}
        impl Old {{ pub fn added(self) -> i32 {{ self.n }} }}
        mod extra {{ struct New {{}} impl crate::Tag for New {{}} }}
        "#
        ),
        None,
    )
    .import_into(&context, &Default::default())
    .unwrap();
    let original = metadata.into_records();
    for corruption in 0..5 {
        let mut program = original.clone();
        let module = &mut program.modules[program.root.index()];
        match corruption {
            0 => {
                let record = module
                    .named_declarations
                    .iter_mut()
                    .find(|record| matches!(record.contract, NamedContract::Alias(_)))
                    .unwrap();
                let NamedContract::Alias(alias) = &mut record.contract else {
                    unreachable!()
                };
                alias.generic_params.clear();
            }
            1 => {
                let record = module.named_declarations.iter_mut().find(|record| matches!(&record.contract, NamedContract::Function(function) if function.name == "main")).unwrap();
                let NamedContract::Function(function) = &mut record.contract else {
                    unreachable!()
                };
                function.return_type = Ty::Builtin(BuiltinType::Bool);
            }
            2 => {
                let record = module.named_declarations.iter_mut().find(|record| matches!(&record.contract, NamedContract::Implementation(header) if header.trait_type.is_some())).unwrap();
                let NamedContract::Implementation(header) = &mut record.contract else {
                    unreachable!()
                };
                header.receiver = Ty::Builtin(BuiltinType::Bool);
            }
            3 => {
                let added = module.named_declarations.iter().find(|record| matches!(&record.contract, NamedContract::Function(function) if function.name == "added")).unwrap();
                let owner = context
                    .snapshot()
                    .parent(added.declaration)
                    .unwrap()
                    .unwrap();
                let mut duplicate = module.named_declarations.iter().find(|record| matches!(&record.contract, NamedContract::Function(function) if function.name == "hidden")).unwrap().clone();
                duplicate.declaration = context
                    .intern_child(owner, DefinitionKind::Method, "hidden", 0)
                    .unwrap();
                module.named_declarations.push(duplicate);
            }
            _ => {
                // A foreign private trait implementation requires the exact
                // checked source requirement, even without any native binding.
                let dependent = program
                    .modules
                    .iter_mut()
                    .find(|module| !module.trait_references.is_empty())
                    .unwrap();
                dependent.trait_references.clear();
            }
        }
        let metadata =
            DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap();
        assert!(
            VerifiedProgram::new(metadata, &context).is_err(),
            "corruption {corruption}"
        );
    }
}

#[test]
fn ambiguous_named_locals_require_unchanged_enclosing_source_and_executable_evidence() {
    let source = r#"
        fn enclosing(flag: bool) -> i32 {
            if flag { fn local() -> i32 { 1 } local() }
            else { fn local() -> i32 { 2 } local() }
        }
        fn other() -> i32 { 3 }
        fn main() -> i32 { enclosing(true) }
    "#;
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    for result in source_free_reload_results(&loaded, source) {
        result.unwrap();
    }
    for candidate in [
        source.replace("{ 1 } local()", "{ 7 } local()"),
        source.replace("if flag", "if !flag"),
    ] {
        for result in source_free_reload_results(&loaded, &candidate) {
            assert!(
                matches!(
                    result,
                    Err(ReloadValidationError::DeclarationContractMismatch { .. })
                ),
                "{result:?}"
            );
        }
    }
    assert_eq!(
        runtime.modules().latest("exports").unwrap().key(),
        loaded.key()
    );
}

#[test]
fn source_free_reload_validators_use_the_same_public_contract() {
    let original = source_code(&reload_source(
        "one",
        "integer",
        "value",
        "a",
        "fn main() {}",
    ));
    let mut runtime = Runtime::default();
    let loaded = runtime.load_verified_program("exports", original).unwrap();
    for compatible in [true, false] {
        let alias = if compatible { "value" } else { "renamed" };
        assert_source_free_reload(
            &loaded,
            &reload_source("two", "replacement", alias, "a", "fn main() {}"),
            compatible,
        );
    }
}
