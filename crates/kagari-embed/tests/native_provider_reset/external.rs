//! This integration target owns its handler and uses only exported runtime APIs.
use super::{artifact, contracts::alter_contracts};
use kagari_abi::types::AbiType;
use kagari_bytecode::KbcArtifact;
use kagari_embed::{EngineConfig, ExecutionContext, KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    NativeAction, NativeContext, NativeInvocationState, NativeRegistration, RuntimeError,
    value::Value,
};

struct Fill {
    count: u64,
    index: u64,
}
impl Fill {
    fn next(&self, context: &NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        if self.index == self.count {
            return Ok(NativeAction::Complete(context.retained(0).unwrap()));
        }
        context
            .callback(
                &context.argument(1).unwrap(),
                &context.signature().params[1],
                vec![Value::U64(self.index)],
            )
            .map(NativeAction::Callback)
    }
}
impl NativeInvocationState for Fill {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        let array = context.heap().alloc_array(vec![])?;
        context.retain(0, Value::Array(array))?;
        self.next(context)
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        // Allocate before retaining the returned heap value: the common driver
        // must keep the popped callback frame's result rooted during receive().
        context.heap().alloc_array(vec![])?;
        let Value::Array(array) = context.retained(0).unwrap() else {
            unreachable!()
        };
        let AbiType::Array(item, _) = &context.signature().result else {
            unreachable!()
        };
        assert!(context.matches(&value, item));
        context.heap().array_push(array, value)?;
        self.index += 1;
        // Repeated callbacks from receive() exercise the same frame driver.
        self.next(context)
    }
}

#[test]
fn an_embedding_owned_provider_can_allocate_and_chain_checked_callbacks() {
    let mut original = artifact(
        "fn main() -> i32 { val values = ArrayList::from_fn(2usize, |i| { [21] }); values[0usize][0usize] + values[1usize][0usize] }",
    );
    let original_key = original
        .program
        .modules
        .iter()
        .flat_map(|m| &m.native_imports)
        .find(|i| {
            i.contract.signature.params.len() == 2
                && matches!(i.contract.signature.params[1], AbiType::Function { .. })
        })
        .unwrap()
        .contract
        .key;
    alter_contracts(&mut original, |contract| {
        if contract.key == original_key {
            contract.key.provider = 0x65787465726e616c;
        }
    });
    let contract = original
        .program
        .modules
        .iter()
        .flat_map(|m| &m.native_imports)
        .find(|i| i.contract.key.provider == 0x65787465726e616c)
        .unwrap()
        .contract
        .clone();
    let checked = KbcArtifact::from_program(original.program, Default::default()).unwrap();
    let checked = KbcArtifact::from_bytes(&checked.to_bytes().unwrap()).unwrap();
    let program =
        PreparedProgram::from_artifact(checked, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = KagariEngine::new(config).runtime(context.clone());
    runtime
        .runtime_mut()
        .register_native(NativeRegistration::new(contract, 1, |context| {
            assert!(context.retained(usize::MAX).is_none());
            assert!(context.retain(usize::MAX, Value::Unit).is_err());
            let Value::U64(count) = context.argument(0).unwrap() else {
                unreachable!()
            };
            Ok(Box::new(Fill { count, index: 0 }))
        }))
        .unwrap();
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
