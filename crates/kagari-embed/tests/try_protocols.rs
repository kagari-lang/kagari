#![cfg(feature = "source")]
use kagari_embed::{
    BytecodeArtifact,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://try-protocols.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &Default::default())
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn control_flow_preserves_break_values_and_changes_the_success_type() {
    execute(
        r#"
        use core::ops::ControlFlow;
        struct Failure { var value: i32 }
        fn stop(error: Failure) -> ControlFlow<Failure, i32> { ControlFlow::Break(error) }
        fn forward(error: Failure) -> ControlFlow<Failure, String> { stop(error)?; ControlFlow::Continue("unused") }
        fn pass(value: ControlFlow<Failure, i32>) -> ControlFlow<Failure, i32> { ControlFlow::Continue(value? + 22) }
        fn main() -> i32 {
            val error = Failure { value: 0 };
            match forward(error) { ControlFlow::Break(same) => { same.value = 20; }, ControlFlow::Continue(_) => { error.value = 0; } };
            match pass(ControlFlow::Continue(0)) { ControlFlow::Continue(value) => value + error.value, ControlFlow::Break(_) => 0 }
        }
    "#,
    );
}

#[test]
fn source_enum_carriers_implement_the_same_protocol() {
    execute(
        r#"
        use core::ops::{Try, FromResidual, ControlFlow};
        use core::convert::Infallible;
        enum Carrier<T, E> { Value(T), Failure(E) }
        impl<T, E> FromResidual<Carrier<Infallible, E>> for Carrier<T, E> {
            fn from_residual(value: Carrier<Infallible, E>) -> Self {
                match value { Carrier::Failure(error) => Carrier::Failure(error), Carrier::Value(never) => match never {} }
            }
        }
        impl<T, E> Try for Carrier<T, E> {
            type Output = T;
            type Residual = Carrier<Infallible, E>;
            fn from_output(value: T) -> Self { Carrier::Value(value) }
            fn branch(self) -> ControlFlow<Self::Residual, T> {
                match self { Carrier::Value(value) => ControlFlow::Continue(value), Carrier::Failure(error) => ControlFlow::Break(Carrier::Failure(error)) }
            }
        }
        fn pass(value: Carrier<i32, String>) -> Carrier<i32, String> { Carrier::Value(value? + 22) }
        fn stop(value: Carrier<i32, String>) -> Carrier<String, String> { value?; Carrier::Value("unused") }
        fn main() -> i32 {
            val callback = || { val value: Carrier<i32, String> = Carrier::Failure("stopped"); value?; Carrier::Value("unused") };
            val callback_result = match callback() { Carrier::Failure(error) => error == "stopped", Carrier::Value(_) => false };
            val first = match pass(Carrier::Value(20)) { Carrier::Value(value) => value, Carrier::Failure(_) => 0 };
            match stop(Carrier::Failure("stopped")) { Carrier::Failure(error) => if error == "stopped" && callback_result { first } else { 0 }, Carrier::Value(_) => 0 }
        }
    "#,
    );
}

#[test]
fn source_struct_carriers_use_selected_methods_once() {
    execute(
        r#"
        use core::ops::{Try, FromResidual, ControlFlow};
        use core::convert::Infallible;
        struct State { var calls: i32 }
        struct Carrier<T> { val value: Option<T>, val state: State }
        struct Residual { val state: State }
        impl<T> FromResidual<Residual> for Carrier<T> {
            fn from_residual(residual: Residual) -> Self { residual.state.calls += 1; Carrier { value: None, state: residual.state } }
        }
        impl<T> Try for Carrier<T> {
            type Output = T;
            type Residual = Residual;
            fn from_output(value: T) -> Self { Carrier { value: Some(value), state: State { calls: 0 } } }
            fn branch(self) -> ControlFlow<Residual, T> {
                self.state.calls += 1;
                match self.value { Some(value) => ControlFlow::Continue(value), None => ControlFlow::Break(Residual { state: self.state }) }
            }
        }
        fn operand(state: State) -> Carrier<i32> { state.calls += 10; Carrier { value: None, state } }
        fn forward(state: State) -> Carrier<String> { operand(state)?; state.calls += 100; Carrier { value: Some("unused"), state } }
        fn main() -> i32 {
            val state = State { calls: 30 };
            val result = forward(state);
            match result.value { None => state.calls, Some(_) => 0 }
        }
    "#,
    );
}

#[test]
fn bounded_try_and_from_residual_functions_keep_associated_outputs() {
    execute(
        r#"
        use core::ops::{Try, FromResidual, ControlFlow};
        fn forward<A: Try, R: FromResidual<A::Residual>>(value: A, wrap: fn(A::Output) -> R) -> R { wrap(value?) }
        fn main() -> i32 {
            val result: Option<i32> = forward(Some(20), |value| Some(value + 22));
            match result { Some(value) => value, None => 0 }
        }
    "#,
    );
}

#[path = "support/try_carrier.rs"]
mod native_provider;

#[test]
fn native_carriers_link_into_a_fresh_artifact_only_runtime() {
    fn engine() -> KagariEngine {
        let mut builder = KagariEngine::builder().unwrap();
        let mut config = EngineConfig::default();
        config.default_runtime.gc.collection_threshold = Some(1);
        builder.config(config);
        builder.install(native_provider::module().unwrap()).unwrap();
        builder.build().unwrap()
    }
    let compiler = engine();
    for source in [
        "use external::try_carrier::Carrier; fn forward(value: Carrier<i32>) -> Carrier<String> { value?; Carrier::Data(\"unused\") } fn main()->i32 { match forward(Carrier::Stop) { Carrier::Stop => 42, Carrier::Data(_) => 0 } }",
        "use external::try_carrier::Carrier; struct Payload { val value: Vec<i32> } fn forward<T>(value: Carrier<T>) -> Carrier<T> { Carrier::Data(value?) } fn main()->i32 { val value = Payload { value: [42] }; match forward(Carrier::Data(value)) { Carrier::Data(item) => item.value[0usize], Carrier::Stop => 0 } }",
    ] {
        let artifact = compiler
            .compile_to_artifact(
                SourceFile::new("memory://native-try.kgr", source),
                Default::default(),
            )
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let mut runtime = engine().runtime(Default::default());
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &Default::default())
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn documented_custom_carrier_and_control_flow_execute() {
    execute(include_str!("../../../examples/syntax/try-protocols.kgr"));
}

#[test]
fn invalid_protocols_and_residuals_fail_during_analysis() {
    use kagari_embed::error::EmbeddingError;
    let prefix = "use core::ops::{Try,FromResidual,ControlFlow}; use core::convert::Infallible; ";
    for (source, code) in [
        (
            "#[lang = \"try\"] pub trait Counterfeit { type Output; type Residual; }",
            "KG_LANGUAGE_ROLE",
        ),
        (
            "struct Carrier {} impl Try for Carrier { type Output = i32; type Residual = Option<Infallible>; fn from_output(value:i32)->Self {Carrier{}} fn branch(self)->ControlFlow<Option<Infallible>,i32> {ControlFlow::Continue(42)} }",
            "KG_TYPE_INVALID_TRAIT_IMPL",
        ),
        (
            "struct Carrier {} impl FromResidual<Option<Infallible>> for Carrier { fn from_residual(value:Option<Infallible>)->Self {Carrier{}} } impl Try for Carrier { type Output = i32; type Residual = Option<Infallible>; fn from_output(value:i32)->Self {Carrier{}} fn branch(self)->ControlFlow<Option<Infallible>,bool> {ControlFlow::Continue(true)} }",
            "KG_TYPE_TRAIT_METHOD_MISMATCH",
        ),
        (
            "fn forward(value:ControlFlow<String,i32>)->Option<i32> {Some(value?)}",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "fn forward<A: Try, R>(value:A)->R { value?; loop {} }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "struct Carrier {} impl FromResidual<Option<Infallible>> for Carrier { fn from_residual(value:Option<Infallible>)->Self {Carrier{}} } impl FromResidual<Option<Infallible>> for Carrier { fn from_residual(value:Option<Infallible>)->Self {Carrier{}} }",
            "KG_TYPE_INVALID_TRAIT_IMPL",
        ),
    ] {
        let error = KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("memory://invalid-try.kgr", format!("{prefix}{source}")),
                Default::default(),
            )
            .unwrap_err();
        let EmbeddingError::Diagnostics { diagnostics } = error else {
            panic!("{error:?}")
        };
        assert!(
            diagnostics.iter().any(|d| d.code == code),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn executable_from_adapters_reject_forged_source_types_and_targets() {
    use kagari_types::{language::Protocol, scalar::BuiltinType, ty::Ty};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "memory://try-witness.kgr",
                "fn main()->Result<i32,i64> { val value: Result<i32,i32> = Err(42); Ok(value?) }",
            ),
            Default::default(),
        )
        .unwrap();
    for mutation in 0..3 {
        let mut program = artifact.program.clone();
        let function = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .find(|function| {
                function
                    .metadata
                    .semantic
                    .protocol_adapter
                    .as_ref()
                    .is_some_and(|required| {
                        Protocol::from_id(&required.interface.declaration) == Some(Protocol::From)
                    })
            })
            .expect("a checked numeric From adapter");
        let required = function
            .metadata
            .semantic
            .protocol_adapter
            .as_mut()
            .unwrap();
        match mutation {
            0 => required.interface.arguments[0] = Ty::Builtin(BuiltinType::String),
            1 => required.receiver = Ty::Builtin(BuiltinType::Bool),
            _ => required.member.path.last_mut().unwrap().name = "branch".into(),
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "forged adapter {mutation}"
        );
    }
}
