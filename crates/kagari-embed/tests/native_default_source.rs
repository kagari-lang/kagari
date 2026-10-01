//! Registered default records enter HIR directly and lower to real native targets.
#![cfg(feature = "source")]

use kagari_abi::{
    callable::{CallableImplementation, MethodPolicy, NativeDefaultApplication},
    native_api::NativeModule,
    native_import::{binding_id, callables::NativeCallableRequirement},
    scalar::BuiltinType,
    types::{
        AbiType, AssociatedTypeAbi, ConstraintAbi, FunctionAbi, GenericBoundAbi,
        GenericParameterAbi, NominalAbiType, ParameterAbi, TraitAbi,
    },
};
use kagari_bytecode::{artifact::KbcArtifact, module::CallableTarget};
use kagari_common::{
    identity::{DefinitionKind, ModuleIdentity, PackageId, associated_type_id},
    source::SourceFile,
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        NativeAction, NativeContext, NativeInvocationState,
        api::{NativeApi, NativeHandler},
    },
    value::Value,
};
use std::collections::BTreeMap;

struct Forward;
impl NativeInvocationState for Forward {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        context
            .selected_callback(0, vec![context.argument(0).unwrap()])
            .map(NativeAction::Callback)
    }
    fn receive(
        &mut self,
        _: &mut NativeContext<'_>,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        Ok(NativeAction::Complete(value))
    }
}

struct BoolRead(bool);
impl NativeInvocationState for BoolRead {
    fn advance(&mut self, _: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        Ok(NativeAction::Complete(Value::I32(if self.0 {
            42
        } else {
            0
        })))
    }
}

fn module() -> NativeModule {
    let mut module = NativeModule::new(ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["defaults".into()],
    });
    let owner = module.definition(DefinitionKind::Trait, "Source");
    let output = associated_type_id(&owner, "Output");
    let projection = AbiType::Projection {
        receiver: Box::new(AbiType::SelfType(owner.clone())),
        interface: Box::new(NominalAbiType {
            declaration: owner.clone(),
            arguments: vec![],
            associated_types: BTreeMap::new(),
        }),
        member: output.clone(),
        arguments: vec![],
    };
    let method = FunctionAbi {
        name: "read".into(),
        implementation: CallableImplementation::Required,
        method_policy: MethodPolicy::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![ParameterAbi {
            name: "self".into(),
            ty: AbiType::SelfType(owner.clone()),
            mutable: false,
        }],
        return_type: projection.clone(),
    };
    let mut default = method.clone();
    default.name = "echo".into();
    default.method_policy.override_allowed = false;
    default.implementation = CallableImplementation::NativeDefault(NativeDefaultApplication {
        declaration: module.definition(DefinitionKind::Function, "echo_template"),
        arguments: vec![AbiType::SelfType(owner.clone()), projection],
    });
    module.traits.push(TraitAbi {
        name: "Source".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![AssociatedTypeAbi {
            declaration: output.clone(),
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: vec![],
        }],
        associated_consts: vec![],
        methods: vec![method, default],
    });
    for (name, selected) in [("echo_template", "read"), ("invoke_default", "echo")] {
        let declaration = module.definition(DefinitionKind::Function, name);
        let receiver = GenericParameterAbi {
            owner: declaration.clone(),
            position: 0,
        };
        let result = GenericParameterAbi {
            owner: declaration.clone(),
            position: 1,
        };
        let interface = NominalAbiType {
            declaration: owner.clone(),
            arguments: vec![],
            associated_types: BTreeMap::from([(output.clone(), result.as_type())]),
        };
        module.functions.push(FunctionAbi {
            name: name.into(),
            implementation: CallableImplementation::Native(binding_id(&module.identity, name)),
            method_policy: MethodPolicy::default(),
            generic_params: vec![receiver.clone(), result.clone()],
            bounds: vec![GenericBoundAbi {
                ty: receiver.as_type(),
                constraints: vec![ConstraintAbi::Trait(interface.clone())],
            }],
            params: vec![ParameterAbi {
                name: "value".into(),
                ty: receiver.as_type(),
                mutable: false,
            }],
            return_type: result.as_type(),
        });
        module.callable_requirements.insert(
            declaration,
            vec![NativeCallableRequirement {
                receiver: receiver.as_type(),
                interface,
                member: NativeModule::method_id(&owner, selected),
                arguments: vec![],
            }],
        );
    }
    let contract = module.traits[0].clone();
    module
        .implement_trait(
            &contract,
            NominalAbiType {
                declaration: owner,
                arguments: vec![],
                associated_types: BTreeMap::from([(output, AbiType::Builtin(BuiltinType::I32))]),
            },
            AbiType::Builtin(BuiltinType::Bool),
            vec![],
            &[("read", binding_id(&module.identity, "read_bool"))],
        )
        .unwrap();
    module
}

fn api(module: NativeModule) -> Result<NativeApi, RuntimeError> {
    let handlers = ["echo_template", "invoke_default"].map(|name| {
        NativeHandler::new(binding_id(&module.identity, name), 0, |_| {
            Ok(Box::new(Forward))
        })
    });
    let mut handlers = Vec::from(handlers);
    handlers.push(NativeHandler::new(
        binding_id(&module.identity, "read_bool"),
        0,
        |context| {
            let Some(Value::Bool(value)) = context.argument(0) else {
                return Err(RuntimeError::module_validation("bool receiver"));
            };
            Ok(Box::new(BoolRead(value)))
        },
    ));
    NativeApi::new(vec![module], handlers)
}

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(api(module()))
        .build()
        .unwrap()
}

const SOURCE: &str = r#"
use game::defaults::{Source, invoke_default};
struct Item { val value: i32 }
impl Source for Item {
    type Output = i32;
    fn read(self) -> i32 { self.value }
}
fn generic<T: Source<Output = i32>>(value: T) -> i32 { value.echo() }
fn dynamic(value: Source<Output = i32>) -> i32 { value.echo() }
fn direct_main() -> i32 { Item { value: 42 }.echo() }
fn generic_main() -> i32 { generic(Item { value: 42 }) }
fn dynamic_main() -> i32 { dynamic(Item { value: 42 }) }
fn selected_main() -> i32 { invoke_default(Item { value: 42 }) }
fn native_main() -> i32 { true.echo() }
fn native_dynamic_main() -> i32 { dynamic(true) }
fn native_selected_main() -> i32 { invoke_default(true) }
"#;

#[test]
fn registered_defaults_compile_and_execute_direct_generic_dynamic_and_selected_calls() {
    let engine = engine();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://native-default-source.kgr", SOURCE),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let default_slots = artifact
        .program
        .modules
        .iter()
        .flat_map(|module| &module.interface_tables)
        .flat_map(|table| &table.methods)
        .filter(|slot| slot.method.path.last().unwrap().name == "echo")
        .collect::<Vec<_>>();
    assert!(!default_slots.is_empty());
    assert!(
        default_slots
            .iter()
            .all(|slot| matches!(slot.target, CallableTarget::Native(_)))
    );
    let bytes = artifact.to_bytes().unwrap();
    let prepared = PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    for entry in [
        "direct_main",
        "generic_main",
        "dynamic_main",
        "selected_main",
        "native_main",
        "native_dynamic_main",
        "native_selected_main",
    ] {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
    }
}

#[test]
fn registered_final_defaults_reject_source_overrides_and_invalid_native_templates() {
    let engine = engine();
    let changed = SOURCE.replace(
        "fn read(self) -> i32 { self.value }",
        "fn read(self) -> i32 { self.value } fn echo(self) -> i32 { 0 }",
    );
    let error = engine
        .compile_to_artifact(
            SourceFile::new("memory://native-default-override.kgr", changed),
            Default::default(),
            Default::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected final override diagnostic: {error:?}");
    };
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code
        == "KG_TYPE_TRAIT_METHOD_MISMATCH"
        && diagnostic.message.contains("forbids overriding")));
    let mut definition = module();
    definition.functions[0].params[0].mutable = true;
    assert!(api(definition).is_err());
    let mut definition = module();
    let CallableImplementation::NativeDefault(application) =
        &mut definition.traits[0].methods[1].implementation
    else {
        unreachable!()
    };
    application.declaration.path[0].name = "missing_template".into();
    assert!(api(definition).is_err());
}

const HEAP_SOURCE: &str = r#"
use game::defaults::{Source, invoke_default};
struct Payload { val value: i32 }
struct Boxed<T> { val value: T }
impl<T> Source for Boxed<T> {
    type Output = T;
    fn read(self) -> T { val scratch = Payload { value: 0 }; self.value }
}
fn dynamic(value: Source<Output = Payload>) -> Payload { value.echo() }
fn direct_main() -> i32 {
    val result = Boxed { value: Payload { value: 42 } }.echo();
    val scratch = Payload { value: 0 };
    result.value
}
fn dynamic_main() -> i32 {
    val result = dynamic(Boxed { value: Payload { value: 42 } });
    val scratch = Payload { value: 0 };
    result.value
}
fn selected_main() -> i32 {
    val result: Payload = invoke_default(Boxed { value: Payload { value: 42 } });
    val scratch = Payload { value: 0 };
    result.value
}
"#;

#[test]
fn generic_default_interfaces_and_heap_callbacks_clean_up_every_budget_cut() {
    let engine = engine();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://native-default-heap.kgr", HEAP_SOURCE),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let prepared = PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    for entry in ["direct_main", "dynamic_main", "selected_main"] {
        let mut finished = false;
        for limit in 0..100 {
            let mut limited = context.clone();
            limited.resources.max_instruction_steps = Some(limit);
            match runtime.execute(&loaded, entry, &[], &limited) {
                Ok(report) => {
                    assert_eq!(report.return_value, Value::I32(42));
                    finished = true;
                }
                Err(error) => assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"),
            }
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
            assert!(!runtime.runtime().is_quarantined());
            runtime.runtime().collect_garbage().unwrap();
            assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
            if finished {
                break;
            }
        }
        assert!(finished, "{entry}");
    }
}

#[test]
fn overridable_defaults_keep_explicit_script_selection_for_direct_dynamic_and_native_calls() {
    let mut definition = module();
    definition.traits[0].methods[1]
        .method_policy
        .override_allowed = true;
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(api(definition))
        .build()
        .unwrap();
    let source = SOURCE.replace(
        "fn read(self) -> i32 { self.value }",
        "fn read(self) -> i32 { self.value } fn echo(self) -> i32 { 99 }",
    );
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://native-default-script-override.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    for entry in ["direct_main", "dynamic_main", "selected_main"] {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::I32(99)
        );
    }
}

#[test]
fn default_template_callbacks_keep_their_script_generation_after_reload() {
    let engine = engine();
    let prepare = |source: &str| {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://native-default-reload.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        PreparedProgram::from_artifact(
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap()
    };
    let original = prepare(SOURCE);
    let candidate = prepare(&SOURCE.replace(
        "fn read(self) -> i32 { self.value }",
        "fn read(self) -> i32 { self.value + 1 }",
    ));
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let old = runtime.load_program(&original, Default::default()).unwrap();
    let current = runtime
        .reload_program(&old, &candidate, Default::default())
        .unwrap();
    for (program, expected) in [(&old, 42), (&current, 43)] {
        for entry in ["direct_main", "dynamic_main", "selected_main"] {
            assert_eq!(
                runtime
                    .execute(program, entry, &[], &context)
                    .unwrap()
                    .return_value,
                Value::I32(expected)
            );
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
        }
    }
}
