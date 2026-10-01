//! Typed application callbacks use the same rooted driver as bundled native calls.
#![cfg(feature = "source")]
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::source::SourceFile;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_native_macros::native_module;
use kagari_runtime::value::Value;

#[native_module("game::callbacks")]
mod callbacks {
    use kagari_runtime::{
        error::RuntimeError,
        native::{NativeAction, NativeContext, NativeInvocationState},
        native_value::{
            NativeCall, NativeResult, NativeValue,
            arguments::NativeArguments,
            continuation::{NativeContinuation, NativeFn},
        },
        value::Value,
    };

    struct Invoke<A: NativeArguments, R: NativeValue> {
        call: NativeCall,
        callback: NativeFn<A, R>,
        arguments: Option<A>,
    }
    impl<A: NativeArguments, R: NativeValue> NativeInvocationState for Invoke<A, R> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            let arguments = self
                .arguments
                .take()
                .ok_or_else(|| RuntimeError::module_validation("callback invoked twice"))?;
            self.callback
                .request(context, arguments)
                .map(NativeAction::Callback)
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            let result = self.callback.result(context, value)?;
            Ok(NativeAction::Complete(
                result.write(&self.call, self.call.result_type())?,
            ))
        }
    }

    /// Invoke a callback with no arguments and return its optional text.
    #[native]
    pub fn zero(
        make: NativeFn<(), Option<String>>,
        #[context] call: &NativeCall,
    ) -> NativeContinuation<Option<String>> {
        NativeContinuation::new(Invoke {
            call: call.clone(),
            callback: make,
            arguments: Some(()),
        })
    }

    /// Invoke a checked generic binary callback and return its result.
    #[native]
    pub fn pair<T: NativeValue>(
        left: T,
        right: T,
        combine: NativeFn<(T, T), T>,
        #[context] call: &NativeCall,
    ) -> NativeContinuation<T> {
        NativeContinuation::new(Invoke {
            call: call.clone(),
            callback: combine,
            arguments: Some((left, right)),
        })
    }

    /// Invoke a checked callback with one argument and a unit result.
    #[native]
    pub fn visit(
        text: String,
        callback: NativeFn<(String,), ()>,
        #[context] call: &NativeCall,
    ) -> NativeContinuation<()> {
        NativeContinuation::new(Invoke {
            call: call.clone(),
            callback,
            arguments: Some((text,)),
        })
    }

    struct Twice<T: NativeValue> {
        call: NativeCall,
        make: NativeFn<(usize,), T>,
        first: Option<T>,
    }
    impl<T: NativeValue> NativeInvocationState for Twice<T> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            self.make
                .request(context, (0usize,))
                .map(NativeAction::Callback)
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            let result = self.make.result(context, value)?;
            if let Some(first) = self.first.take() {
                drop(result);
                Ok(NativeAction::Complete(
                    first.write(&self.call, self.call.result_type())?,
                ))
            } else {
                self.first = Some(result);
                self.make
                    .request(context, (1usize,))
                    .map(NativeAction::Callback)
            }
        }
    }

    /// Invoke the callback twice in order, retaining the first result until return.
    #[native]
    pub fn twice<T: NativeValue>(
        make: NativeFn<(usize,), T>,
        #[context] call: &NativeCall,
    ) -> NativeContinuation<T> {
        NativeContinuation::new(Twice {
            call: call.clone(),
            make,
            first: None,
        })
    }
}

fn program(engine: &KagariEngine, text: &str) -> PreparedProgram {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://typed-callbacks.kgr", text),
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
}

#[test]
fn zero_binary_and_unit_callbacks_preserve_typed_arguments_and_side_effects() {
    let api = callbacks::native_api().unwrap();
    let text = &api.declaration_sources()[0].text;
    assert!(text.contains("make: fn() -> Option<String>"));
    assert!(text.contains("combine: fn(T0, T0) -> T0"));
    assert!(text.contains("callback: fn(String) -> ()"));
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::builder()
        .config(config)
        .install(Ok(api))
        .build()
        .unwrap();
    let program = program(
        &engine,
        r#"
        use game::callbacks::{zero, pair, visit};
        fn main() -> i32 {
            val counter = [0];
            visit("text", |text| { if text == "text" { counter[0usize] = 1; } });
            val answer = pair([20], [22], |left, right| { [left[0usize] + right[0usize]] });
            if zero(|| Some("ready")) == Some("ready") && counter[0usize] == 1 {
                answer[0usize]
            } else { 0 }
        }
    "#,
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn repeated_typed_callbacks_root_heap_results_across_nested_native_calls_and_traps() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::builder()
        .config(config)
        .install(callbacks::native_api())
        .build()
        .unwrap();
    let success = program(
        &engine,
        r#"
        use game::callbacks::twice;
        fn main() -> i32 {
            val count = [0];
            val first = twice(|index| {
                count[0usize] = count[0usize] + 1;
                ArrayList::from_fn(2usize, |j| if index == 0usize { 42 } else { 0 })
            });
            if count[0usize] == 2 { first[0usize] } else { 0 }
        }
    "#,
    );
    let failure = program(
        &engine,
        r#"
        use game::callbacks::twice;
        fn main() -> i32 { twice(|index| { val a = [42]; a[99usize] }) }
    "#,
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let success = runtime.load_program(&success, Default::default()).unwrap();
    let failure = runtime.load_program(&failure, Default::default()).unwrap();
    assert!(runtime.execute(&failure, "main", &[], &context).is_err());
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert_eq!(
        runtime
            .execute(&success, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}
