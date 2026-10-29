//! Task storage, fluent methods and callbacks share the canonical core::task owner.
mod composition;
use crate::{
    foundation::{
        enumeration,
        enumerations::value::{self as enums, EnumSymbols},
        type_docs,
    },
    identity,
};
use kagari_common::identity::DefinitionKind;
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::draft::ModuleDraft,
        binding::{Codec, NativeBinding, NativeResult},
        context::CallContext,
        declarations::MethodDecl,
        storage::NativeStorage,
        types::Type,
    },
    task::SpawnError,
    value::Value,
};
use kagari_types::declaration::{TypeDefKind, VariantDef, native::NativeStorageLayout};

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Scope-owned asynchronous jobs with retained terminal results and host-controlled scheduling.",
    "fn retain(value: Task<i32>) -> core::task::Task<i32> { value }",
);

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    composition::declare(module)?;
    let task = module.define_type(
        "Task",
        TypeDefKind::NativeStorage(NativeStorageLayout::Task),
        1,
        |_, _| Ok(()),
    )?;
    module.bind_storage(&task, NativeStorage::task())?;
    type_docs(
        module,
        &task,
        "Task",
        (
            "A scope-owned execution handle. Dropping a handle does not cancel the task. Completed values are retained by live handles and pending host reports.",
            MODULE_DOCS.1,
        ),
        &[],
    )?;
    let scope = module.define_type(
        "TaskScope",
        TypeDefKind::NativeStorage(NativeStorageLayout::TaskScope),
        0,
        |_, _| Ok(()),
    )?;
    module.bind_storage(&scope, NativeStorage::task_scope())?;
    type_docs(
        module,
        &scope,
        "TaskScope",
        (
            "A host-created capability for admitting independent tasks into one host-owned scope. Only the host owner controls driving and scope closure.",
            "fn retain(value: TaskScope) -> core::task::TaskScope { value }",
        ),
        &[],
    )?;
    let variants = ["ScopeClosed", "CapacityExceeded", "DispatchUnavailable"];
    let error = module.define_type("SpawnError", TypeDefKind::Enum, 0, |record, _| {
        record.variants = variants
            .iter()
            .map(|name| VariantDef {
                name: (*name).into(),
                payload: vec![],
                reports_failure: false,
            })
            .collect();
        Ok(())
    })?;
    type_docs(
        module,
        &error,
        "SpawnError",
        (
            "A failed Task admission: ScopeClosed, CapacityExceeded or DispatchUnavailable. No task is admitted on Err. Failures after successful admission belong to the Task outcome.",
            "fn closed() -> SpawnError { SpawnError::ScopeClosed }",
        ),
        &variants,
    )?;
    let future = module.reference_type(
        identity::type_id("Future", DefinitionKind::AssociatedType),
        TypeDefKind::NativeStorage(NativeStorageLayout::Future),
        1,
    )?;
    let result = enumeration(module, "Result", 2)?;
    let spawned = task.clone();
    module.inherent(scope.apply([])?, |methods| {
        methods.define(MethodDecl::instance("spawn")
            .documentation("Admit a zero-argument factory returning Future<T>. The factory runs once on the first host drive, after admission. Closures and checked Fn implementations use ordinary callable coercion. Admission failures return SpawnError; execution failures terminate the Task.\n\n# Examples\n\n```kgr\nasync fn answer() -> i32 { 42 }\nfn submit(scope: TaskScope) -> Result<Task<i32>, SpawnError> { scope.spawn(|| answer()) }\n```")
            .configure(move |function| {
                let output = function.type_parameter("T")?.ty();
                function.parameter("arg1", Type::function([], future.apply([output.clone()])?)?)?;
                function.returns(result.apply([spawned.apply([output])?, error.apply([])?])?)?;
                Ok(())
            })
            .implementation_with_send(|view| {
                let symbols = EnumSymbols::new(view.catalog(), &[
                    enums::RESULT,
                    ("SpawnError", &["ScopeClosed", "CapacityExceeded", "DispatchUnavailable"]),
                ])?;
                Ok(unsafe { NativeBinding::new_transport(vec![Codec::Value; 2], Codec::Value,
                    move |cx| spawn(&symbols, cx)) })
            }))
    })?;
    module.inherent_generic(&task, |_, methods| {
        methods.define(MethodDecl::instance("cancel").returns(Type::unit())
            .documentation("Request cooperative cancellation of this Task. Completed Tasks are unchanged. Cancelling a waiting Task detaches its wait without cancelling the dependency.\n\n# Examples\n\n```kgr\nfn stop<T>(task: Task<T>) { task.cancel(); }\n```")
            .implementation_send(unsafe { NativeBinding::new_transport(vec![Codec::Value], Codec::Value, cancel) }))?;
        Ok(())
    })
}

fn spawn(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    // SAFETY: The call frame retains task inputs until task admission/cancellation finishes.
    let scope = unsafe { cx.argument_transport(0) }?;
    // SAFETY: The call frame retains task inputs until task admission/cancellation finishes.
    let factory = unsafe { cx.argument_transport(1) }?;
    let result = cx.result_type_argument()?;
    // SAFETY: The call frame retains scope/factory/task inputs through admission; successful spawn
    // returns an owning root.
    match unsafe { cx.spawn_task_transport(&scope, &factory) }? {
        Ok(task) => {
            // SAFETY: task remains retained until Result::Ok publishes its edge.
            let value = unsafe { task.value_transport(cx.heap()) }
                .ok_or_else(|| RuntimeError::module_validation("spawned Task root"))?;
            enums::allocate(symbols, cx, &result, "Result", "Ok", vec![value])
        }
        Err(error) => {
            let ty = cx.type_parameter(&result, 1)?;
            let member = match error {
                SpawnError::ScopeClosed => "ScopeClosed",
                SpawnError::CapacityExceeded => "CapacityExceeded",
                SpawnError::DispatchUnavailable => "DispatchUnavailable",
            };
            let value = enums::allocate(symbols, cx, &ty, "SpawnError", member, vec![])?;
            let _error = unsafe {
                // SAFETY: Publish the newly allocated error value before constructing its result carrier.
                cx.heap().root_value_transport(value)
            }
            .ok_or_else(|| RuntimeError::module_validation("SpawnError root"))?;
            enums::allocate(symbols, cx, &result, "Result", "Err", vec![value])
        }
    }
}

fn cancel(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    // SAFETY: The call frame retains task inputs until task admission/cancellation finishes.
    unsafe { cx.cancel_task_transport(&cx.argument_transport(0)?) }?;
    Ok(Value::Unit)
}
