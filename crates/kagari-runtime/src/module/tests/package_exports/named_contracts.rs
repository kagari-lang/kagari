//! Frozen declaration inventory and source-free forged-contract admission.

use crate::{
    Runtime,
    module::VerifiedProgram,
    reload::ReloadValidationError,
    test_support::update_programs::{bytecode, source_code, source_free_reload_results},
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::named::NamedContract;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use rstest::rstest;

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
    let loaded_program = runtime
        .load_verified_program("exports", source_code(NAMED_RELOAD_SOURCE))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
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
        .stage_reload_verified_program(&loaded_program, None, source_code(&candidate))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let published = runtime.select_program(&loaded_program).unwrap();
    assert!(published.exported_function("extra::added").is_some());
    assert!(loaded.exported_function("extra::added").is_none());
    assert!(published.members().count() > loaded.members().count());
}

#[rstest]
#[case::removed_function(NAMED_RELOAD_SOURCE.replace("fn unused<T>(x: T) -> T { x }", ""))]
#[case::function_result(NAMED_RELOAD_SOURCE.replace(
            "fn unused<T>(x: T) -> T { x }",
            "fn unused<T>(x: T) -> (T, T) { (x, x) }",
        ))]
#[case::function_bound(NAMED_RELOAD_SOURCE.replace("fn unused<T>(x: T)", "fn unused<T: Tag>(x: T)"))]
#[case::alias_bound(NAMED_RELOAD_SOURCE.replace("type Alias<T>", "type Alias<T: Tag>"))]
#[case::removed_alias(NAMED_RELOAD_SOURCE.replace("type Alias<T> = (T, i32);", ""))]
#[case::alias_target(NAMED_RELOAD_SOURCE.replace("type Alias<T> = (T, i32);", "type Alias<T> = (T, bool);"))]
#[case::constant_value(NAMED_RELOAD_SOURCE.replace("SECRET: i32 = 7", "SECRET: i32 = 8"))]
#[case::method_result(NAMED_RELOAD_SOURCE.replace(
            "fn hidden(self) -> i32 { self.n }",
            "fn hidden(self) -> bool { true }",
        ))]
#[case::removed_impl(NAMED_RELOAD_SOURCE.replace("impl Tag for Old {}", ""))]
#[case::old_builtin_impl(format!("{NAMED_RELOAD_SOURCE} impl Tag for i32 {{}}"))]
#[case::old_nominal_impl(format!("{NAMED_RELOAD_SOURCE} trait Other {{}} impl Other for Old {{}}"))]
fn named_inventory_freezes_unused_declarations_and_restricts_new_impl_domains(
    #[case] candidate: String,
) {
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(NAMED_RELOAD_SOURCE))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    for result in source_free_reload_results(&loaded, &candidate)
        .into_iter()
        .chain([runtime
            .stage_reload_verified_program(&loaded_program, None, source_code(&candidate))
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
        runtime.select_program(&loaded_program).unwrap().key(),
        loaded.key()
    );
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InventoryCorruption {
    AliasBinders,
    FunctionSignature,
    ImplReceiver,
    DuplicateMethod,
    TraitProvenance,
}

#[rstest]
#[case::alias_binders(InventoryCorruption::AliasBinders)]
#[case::function_signature(InventoryCorruption::FunctionSignature)]
#[case::impl_receiver(InventoryCorruption::ImplReceiver)]
#[case::duplicate_method(InventoryCorruption::DuplicateMethod)]
#[case::trait_provenance(InventoryCorruption::TraitProvenance)]
fn source_free_named_inventory_rejects_forged_binders_signatures_and_semantic_duplicates(
    #[case] corruption: InventoryCorruption,
) {
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
    let mut program = original.clone();
    let module = &mut program.modules[program.root.index()];
    match corruption {
        InventoryCorruption::AliasBinders => {
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
        InventoryCorruption::FunctionSignature => {
            let record = module.named_declarations.iter_mut().find(|record| matches!(&record.contract, NamedContract::Function(function) if function.name == "main")).unwrap();
            let NamedContract::Function(function) = &mut record.contract else {
                unreachable!()
            };
            function.return_type = Ty::Builtin(BuiltinType::Bool);
        }
        InventoryCorruption::ImplReceiver => {
            let record = module.named_declarations.iter_mut().find(|record| matches!(&record.contract, NamedContract::Implementation(header) if header.trait_type.is_some())).unwrap();
            let NamedContract::Implementation(header) = &mut record.contract else {
                unreachable!()
            };
            header.receiver = Ty::Builtin(BuiltinType::Bool);
        }
        InventoryCorruption::DuplicateMethod => {
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
        InventoryCorruption::TraitProvenance => {
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
        "corruption {corruption:?}"
    );
}

#[rstest]
#[case::local_body(|source: &str| source.replace("{ 1 } local()", "{ 7 } local()"))]
#[case::enclosing_condition(|source: &str| source.replace("if flag", "if !flag"))]
fn ambiguous_named_locals_require_unchanged_enclosing_source_and_executable_evidence(
    #[case] candidate: fn(&str) -> String,
) {
    let source = r#"
        fn enclosing(flag: bool) -> i32 {
            if flag { fn local() -> i32 { 1 } local() }
            else { fn local() -> i32 { 2 } local() }
        }
        fn other() -> i32 { 3 }
        fn main() -> i32 { enclosing(true) }
    "#;
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    for result in source_free_reload_results(&loaded, source) {
        result.unwrap();
    }
    let candidate = candidate(source);
    for result in source_free_reload_results(&loaded, &candidate) {
        assert!(
            matches!(
                result,
                Err(ReloadValidationError::DeclarationContractMismatch { .. })
            ),
            "{result:?}"
        );
    }
    assert_eq!(
        runtime.select_program(&loaded_program).unwrap().key(),
        loaded.key()
    );
}
