//! Native registration definitions drive execution and tooling without declaration binaries.
use kagari_abi::{
    callable::CallableImplementation,
    native_api::NativeModule,
    native_import::binding_id,
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{AbiType, FunctionAbi},
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, ModuleIdentity, PackageId},
};
use kagari_embed::engine::{EngineConfig, KagariEngine};
use kagari_native_macros::native_module;
use kagari_runtime::{
    error::RuntimeError,
    native::{
        NativeAction, NativeContext, NativeInvocationState,
        api::{NativeApi, NativeHandler},
        packages::standard_library,
    },
    value::Value,
};
use std::collections::BTreeSet;

struct Answer;
impl NativeInvocationState for Answer {
    fn advance(&mut self, _: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        Ok(NativeAction::Complete(Value::I32(42)))
    }
}

fn application_module() -> NativeModule {
    let mut module = NativeModule::new(ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["math".into()],
    });
    let id = module.definition(DefinitionKind::Function, "answer");
    module
        .documentation
        .insert(id, "Return the application-owned answer.".into());
    module.functions.push(FunctionAbi {
        name: "answer".into(),
        method_policy: Default::default(),
        implementation: CallableImplementation::Native(binding_id(&module.identity, "answer")),
        generic_params: vec![],
        bounds: vec![],
        params: vec![],
        return_type: AbiType::Builtin(BuiltinType::I32),
    });
    module
}
#[native_module("game::math")]
mod math {
    /// Return the application-owned answer.
    #[native]
    pub fn answer() -> i32 {
        42
    }
}
fn application_api() -> NativeApi {
    math::native_api().unwrap()
}

#[test]
fn authoring_matches_explicit_records_and_preserves_documentation() {
    let api = application_api();
    let expected = application_module();
    assert_eq!(
        api.modules()[0].native_declarations(),
        expected.native_declarations()
    );
    assert_eq!(api.modules()[0].documentation, expected.documentation);
}

#[native_module("game::aliases", runtime = kagari_runtime)]
mod aliases {
    use kagari_runtime::native_value::{NativeResult, NativeValue};
    type Count = usize;
    /// Preserve aliases and checked optional values.
    #[native]
    pub fn increment(value: Count) -> NativeResult<Option<Count>> {
        Ok(value.checked_add(1))
    }
    #[native]
    pub fn identity<T: NativeValue>(value: T) -> T {
        value
    }
    #[native]
    pub fn positive(value: i32) -> bool {
        value > 0
    }
    #[native]
    pub fn text(value: String) -> String {
        value
    }
}

#[test]
fn authoring_resolves_rust_aliases_and_generic_value_contracts() {
    let api = aliases::native_api().unwrap();
    let text = &api.declaration_sources()[0].text;
    assert!(text.contains("fn increment(value: usize) -> Option<usize>;"));
    assert!(text.contains("fn identity<T0>(value: T0) -> T0;"));
    assert!(text.contains("fn positive(value: i32) -> bool;"));
    let engine = KagariEngine::builder().install(Ok(api)).build().unwrap();
    assert_eq!(
        engine
            .native_declaration_sources()
            .iter()
            .map(|source| source.uri.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "kagari://native/game/aliases.kgr",
            "kagari://native/kagari-std/array.kgr",
            "kagari://native/kagari-std/cmp.kgr",
            "kagari://native/kagari-std/debug.kgr",
            "kagari://native/kagari-std/fmt.kgr",
            "kagari://native/kagari-std/hash.kgr",
            "kagari://native/kagari-std/math.kgr",
            "kagari://native/kagari-std/numeric.kgr",
            "kagari://native/kagari-std/ops.kgr",
            "kagari://native/kagari-std/option.kgr",
            "kagari://native/kagari-std/result.kgr",
            "kagari://native/kagari-std/string.kgr",
        ])
    );
    assert!(
        KagariEngine::builder()
            .install(math::native_api())
            .install(math::native_api())
            .build()
            .is_err()
    );
    assert!(
        KagariEngine::builder()
            .install(Err(RuntimeError::module_validation("rejected package")))
            .build()
            .is_err()
    );
}

#[cfg(feature = "source")]
#[native_module("game::retained")]
mod retained {
    use kagari_runtime::{
        error::RuntimeError,
        native_value::{NativeResult, array::NativeArray},
    };
    use std::cell::RefCell;
    thread_local! {
        static SAVED: RefCell<Option<NativeArray<i32>>> = const { RefCell::new(None) };
    }
    #[native]
    pub fn save(value: NativeArray<i32>) -> usize {
        let len = value.len();
        SAVED.with(|saved| *saved.borrow_mut() = Some(value));
        len
    }
    #[native]
    pub fn take() -> NativeResult<NativeArray<i32>> {
        SAVED
            .with(|saved| saved.borrow_mut().take())
            .ok_or_else(|| RuntimeError::module_validation("missing retained array"))
    }
}

#[native_module("game::collision")]
mod collision {
    #[native(binding = "same")]
    pub fn first() -> i32 {
        1
    }
    #[native(binding = "same")]
    pub fn second() -> i32 {
        2
    }
}
#[test]
fn different_rust_functions_cannot_silently_share_a_binding() {
    assert!(collision::native_api().is_err());
}

#[test]
fn registration_rejects_missing_duplicate_unknown_and_invalid_contracts() {
    let module = application_module();
    assert!(NativeApi::new(vec![module.clone()], vec![], Default::default()).is_err());
    let handler = || {
        NativeHandler::new(binding_id(&module.identity, "answer"), 0, |_| {
            Ok(Box::new(Answer))
        })
    };
    assert!(
        NativeApi::new(
            vec![module.clone()],
            vec![handler(), handler()],
            Default::default()
        )
        .is_err()
    );
    let unknown = NativeHandler::new(binding_id(&module.identity, "other"), 0, |_| {
        Ok(Box::new(Answer))
    });
    assert!(NativeApi::new(vec![module.clone()], vec![unknown], Default::default()).is_err());
    assert!(
        KagariEngine::with_native_apis(
            Default::default(),
            vec![application_api(), application_api()]
        )
        .is_err()
    );
    let mut module = module;
    module.functions[0]
        .params
        .push(kagari_abi::types::ParameterAbi {
            name: "bad".into(),
            ty: AbiType::Parameter {
                owner: binding_id(&module.identity, "unbound"),
                position: 0,
            },
            mutable: false,
        });
    assert!(module.validate().is_err());
    let mut iterator = application_module();
    iterator.functions[0].return_type = AbiType::Iter(Box::new(AbiType::Builtin(BuiltinType::I32)));
    assert!(iterator.validate().is_ok());
    let mut unsupported = application_module();
    unsupported.functions[0].return_type = AbiType::Map {
        key: Box::new(AbiType::Builtin(BuiltinType::I32)),
        value: Box::new(AbiType::Builtin(BuiltinType::I32)),
        access: CollectionAccess::Mutable,
    };
    assert!(unsupported.validate().is_err());
}

#[test]
fn trait_implementations_cannot_change_signatures_or_omit_supertraits() {
    let api = standard_library();
    let module = api
        .modules()
        .iter()
        .find(|module| module.identity.path == ["array"])
        .unwrap()
        .as_ref();
    let mut changed = module.clone();
    changed.implementations[2].methods[0].params[1].ty = AbiType::Builtin(BuiltinType::String);
    assert!(changed.validate().is_err());
    let mut changed = module.clone();
    changed.implementations[1].trait_type = None;
    assert!(changed.validate().is_err());
    let mut changed = module.clone();
    let implementation = changed.implementations.pop().unwrap();
    assert!(
        changed
            .implement_trait(
                &module.traits[1],
                implementation.trait_type.unwrap(),
                implementation.for_type,
                implementation.generic_params,
                &[]
            )
            .is_err()
    );
}

#[test]
fn generated_sources_come_from_the_installed_packages() {
    let mut scalar_enum = application_module();
    scalar_enum.functions[0].return_type = AbiType::StandardEnum {
        kind: StandardEnum::Ordering,
        args: vec![],
    };
    scalar_enum.validate().unwrap();
    assert!(
        scalar_enum
            .declaration_source()
            .unwrap()
            .text
            .contains("-> Ordering;")
    );
    let engine =
        KagariEngine::with_native_apis(Default::default(), vec![application_api()]).unwrap();
    let sources = engine.native_declaration_sources();
    let array = sources
        .iter()
        .find(|source| source.uri.ends_with("/array.kgr"))
        .unwrap();
    assert!(array.text.contains("pub trait MutableList<T0>: List<T0>"));
    assert!(
        array
            .text
            .contains("impl<T0> MutableList<T0> for ArrayList<T0>")
    );
    assert_eq!(array.text, include_str!("../../../stdlib/array.kgr"));
    let application = sources
        .iter()
        .find(|source| source.uri == "kagari://native/game/math.kgr")
        .unwrap();
    assert!(application.text.contains("pub fn answer() -> i32;"));
    assert!(
        application
            .text
            .contains("Return the application-owned answer.")
    );
    let config = EngineConfig {
        install_standard_library: false,
        ..Default::default()
    };
    let engine = KagariEngine::with_native_apis(config, vec![application_api()]).unwrap();
    assert_eq!(engine.native_declaration_sources().len(), 1);
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_bytecode::artifact::KbcArtifact;
    use kagari_common::{
        collection::CollectionAccess, source::SourceFile, source_database::SourceLayer,
    };

    use kagari_embed::{context::ExecutionContext, program::PreparedProgram};
    use std::{cell::RefCell, rc::Rc};

    fn execute(engine: &KagariEngine, text: &str) -> Value {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://native-registration.kgr", text),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let program = PreparedProgram::from_artifact(
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let result = runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value;
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        result
    }

    #[test]
    fn authoring_keeps_runtime_output_contract_validation() {
        // Advanced factories still reject forged runtime results after installation.
        let mut module = application_module();
        module.functions[0].name = "wrong".into();
        module.functions[0].return_type = AbiType::Builtin(BuiltinType::USize);
        let id = binding_id(&module.identity, "wrong");
        module.functions[0].implementation = CallableImplementation::Native(id.clone());
        let api = NativeApi::new(
            vec![module],
            vec![NativeHandler::new(id, 0, |_| Ok(Box::new(Answer)))],
            Default::default(),
        )
        .unwrap();
        let engine = KagariEngine::with_native_apis(Default::default(), vec![api]).unwrap();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://wrong-native-output.kgr",
                    "use game::math::wrong; fn main() -> usize { wrong() }",
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert!(runtime.execute(&loaded, "main", &[], &context).is_err());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
    }

    #[test]
    fn typed_rust_alias_generic_and_optional_values_execute_under_gc() {
        let mut config = EngineConfig::default();
        config.default_runtime.gc.collection_threshold = Some(1);
        let engine = KagariEngine::builder()
            .config(config)
            .install(aliases::native_api())
            .build()
            .unwrap();
        assert_eq!(
            execute(
                &engine,
                r#"
            use game::aliases::{increment, identity, positive, text};
            fn main() -> i32 {
                val values = identity([20, 22]);
                val view: List<i32> = values;
                if positive(values[0usize]) && increment(1usize) == Some(2usize)
                    && view.get(1usize) == Some(22) && view.get(4usize) == None
                    && text("typed") == "typed" {
                    identity(values[0usize]) + values[1usize]
                } else { 0 }
            }
        "#
            ),
            Value::I32(42)
        );
        assert!(
            engine
                .compile_source(
                    SourceFile::new(
                        "memory://typed-input.kgr",
                        "use game::aliases::positive; fn main() -> bool { positive(1usize) }"
                    ),
                    Default::default()
                )
                .is_err()
        );
    }

    #[test]
    fn typed_array_roots_survive_a_call_and_reject_another_heap() {
        let engine = KagariEngine::builder()
            .install(retained::native_api())
            .build()
            .unwrap();
        let prepare = |text| {
            let artifact = engine
                .compile_to_artifact(
                    SourceFile::new("memory://retained-native.kgr", text),
                    Default::default(),
                    Default::default(),
                )
                .unwrap();
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap()
        };
        let save = prepare("use game::retained::save; fn main() -> usize { save([42]) }");
        let take = prepare("use game::retained::take; fn main() -> i32 { take()[0usize] }");
        let context = ExecutionContext::default();
        let mut first = engine.runtime(context.clone());
        let save_loaded = first.load_program(&save, Default::default()).unwrap();
        let take_loaded = first.load_program(&take, Default::default()).unwrap();
        assert_eq!(
            first
                .execute(&save_loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::U64(1)
        );
        first.runtime().collect_garbage().unwrap();
        assert_eq!(
            first
                .execute(&take_loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        assert_eq!(first.runtime().gc().active_roots(), 0);

        first.execute(&save_loaded, "main", &[], &context).unwrap();
        let mut second = engine.runtime(context.clone());
        let other_loaded = second.load_program(&take, Default::default()).unwrap();
        assert!(
            second
                .execute(&other_loaded, "main", &[], &context)
                .is_err()
        );
        assert_eq!(first.runtime().gc().active_roots(), 0);
        assert_eq!(second.runtime().gc().active_roots(), 0);
        assert_eq!(
            second.runtime().resources().counters().current_call_depth,
            0
        );
    }

    #[test]
    fn mutable_list_native_impl_works_through_generic_and_dynamic_calls() {
        let mut config = EngineConfig::default();
        config.default_runtime.gc.collection_threshold = Some(1);
        let engine = KagariEngine::new(config);
        assert_eq!(
            execute(
                &engine,
                r#"
            fn update<L: MutableList<i32>>(values: L) { values.set(0usize, 20); }
            fn main() -> i32 {
                val values = [0, 0];
                update(values);
                val mutable: MutableList<i32> = values;
                mutable.set(1usize, 22);
                val readonly: List<i32> = values;
                if readonly.len() == 2usize { values[0usize] + values[1usize] } else { 0 }
            }
        "#
            ),
            Value::I32(42)
        );
        assert!(
            engine
                .compile_source(
                    SourceFile::new(
                        "memory://readonly-native.kgr",
                        "fn main() { val values: List<i32> = [0]; values.set(0usize, 42); }"
                    ),
                    Default::default()
                )
                .is_err()
        );
    }

    #[test]
    fn application_native_executes_without_the_default_package() {
        let config = EngineConfig {
            install_standard_library: false,
            ..Default::default()
        };
        let engine = KagariEngine::with_native_apis(config, vec![application_api()]).unwrap();
        assert_eq!(
            execute(
                &engine,
                "use game::math::answer; fn main() -> i32 { answer() }"
            ),
            Value::I32(42)
        );
        assert!(
            engine
                .compile_source(
                    SourceFile::new(
                        "memory://no-default-api.kgr",
                        "fn main() -> usize { [1].len() }"
                    ),
                    Default::default()
                )
                .is_err()
        );
        let default = KagariEngine::default();
        let artifact = default
            .compile_to_artifact(
                SourceFile::new(
                    "memory://requires-array.kgr",
                    "fn main() -> usize { [1].len() }",
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        assert!(
            engine
                .runtime(Default::default())
                .load_program(&prepared, Default::default())
                .is_err()
        );
    }

    #[test]
    fn generated_native_methods_support_navigation_docs_and_completion() {
        let engine =
            KagariEngine::with_native_apis(Default::default(), vec![application_api()]).unwrap();
        let text = "use game::math::answer; fn main() { val values = [0]; values.set(0usize, answer()); values. }";
        let file = engine
            .set_source(
                "memory://native-tooling.kgr",
                text.into(),
                SourceLayer::Base,
            )
            .unwrap();
        let snapshot = engine
            .analyze(
                engine.source_snapshot(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
        let declaration = snapshot
            .definition_at(file, text.find("set(").unwrap())
            .unwrap();
        let source = snapshot.source(declaration.location.file).unwrap();
        assert!(source.name().starts_with("kagari://native/"));
        assert_eq!(
            &source.text()[declaration.location.range.start..declaration.location.range.end],
            "set"
        );
        assert!(
            snapshot
                .documentation_at(file, text.find("set(").unwrap())
                .unwrap()
                .documentation
                .contains("Replace a valid slot")
        );
        let completions = snapshot
            .file(file)
            .unwrap()
            .method_completions(text.rfind("values.").unwrap() + "values.".len());
        assert!(completions.iter().any(|method| method.name == "set"));
        let offset = text.rfind("answer()").unwrap();
        let documentation = snapshot.documentation_at(file, offset).unwrap();
        assert_eq!(
            documentation.documentation,
            "Return the application-owned answer."
        );
        assert!(documentation.written_signature.contains("answer() -> i32"));
    }

    #[test]
    fn failed_native_set_preserves_slots_and_releases_invocation_roots() {
        let saved = Rc::new(RefCell::new(None));
        let captured = saved.clone();
        let mut module = application_module();
        module.documentation.clear();
        module.functions[0].name = "seed".into();
        let binding = binding_id(&module.identity, "seed");
        module.functions[0].implementation = CallableImplementation::Native(binding.clone());
        module.functions[0].return_type = AbiType::Array(
            Box::new(AbiType::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable,
        );
        struct Seed;
        impl NativeInvocationState for Seed {
            fn advance(
                &mut self,
                context: &mut NativeContext<'_>,
            ) -> Result<NativeAction, RuntimeError> {
                Ok(NativeAction::Complete(context.retained(0).unwrap()))
            }
        }
        let handler = NativeHandler::new(binding, 1, move |context| {
            let id = context.heap().alloc_array(vec![Value::I32(42)])?;
            context.retain(0, Value::Array(id))?;
            *captured.borrow_mut() = Some(id);
            Ok(Box::new(Seed))
        });
        let api = NativeApi::new(vec![module], vec![handler], Default::default()).unwrap();
        let mut config = EngineConfig::default();
        config.default_runtime.gc.collection_threshold = Some(1);
        let engine = KagariEngine::with_native_apis(config, vec![api]).unwrap();
        assert!(
            saved.borrow().is_none(),
            "registration must not execute handlers"
        );
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://set-failure.kgr",
                    r#"
            use game::math::seed;
            fn main() { val values: MutableList<i32> = seed(); values.set(9usize, 0); }
        "#,
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        assert!(runtime.execute(&loaded, "main", &[], &context).is_err());
        let id = saved.borrow().unwrap();
        assert_eq!(
            runtime.runtime().gc().array_get(id, 0),
            Some(Value::I32(42))
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}
