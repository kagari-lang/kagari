//! Task storage, fluent methods and callbacks share the canonical core::task owner.
mod composition;
pub(super) mod documentation;
use crate::{
    catalog::documentation::{example, markdown},
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
    "Host-driven asynchronous jobs with retained terminal results.\n\nThe host creates and owns TaskScope and drives the runtime scheduler. Script\nreceives only the admission capability. The example requires an open scope\nprovided by that host; it observes that admission does not run the factory.\nDropping a script Task handle does not cancel its job.",
    documentation::SCOPE_EXAMPLE,
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
            documentation::TASK_EXAMPLE,
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
            documentation::SCOPE_EXAMPLE,
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
            "An admission failure: no Task is returned and the factory does not run.\n\nScopeClosed means the host closed the scope; CapacityExceeded means admission\ncapacity is exhausted; DispatchUnavailable means the host notification route\nis unavailable. Execution failures after admission belong to the Task outcome.\nThe example receives a scope that its host has already closed.",
            documentation::CLOSED_EXAMPLE,
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
        methods.define(
            MethodDecl::instance("spawn")
                .documentation(markdown(MODULE_DOCS))
                .configure(move |function| {
                    let output = function.type_parameter("T")?.ty();
                    function
                        .parameter("arg1", Type::function([], future.apply([output.clone()])?)?)?;
                    function
                        .returns(result.apply([spawned.apply([output])?, error.apply([])?])?)?;
                    Ok(())
                })
                .implementation_with_send(|view| {
                    let symbols = EnumSymbols::new(
                        view.catalog(),
                        &[
                            enums::RESULT,
                            (
                                "SpawnError",
                                &["ScopeClosed", "CapacityExceeded", "DispatchUnavailable"],
                            ),
                        ],
                    )?;
                    Ok(NativeBinding::new(
                        vec![Codec::Value; 2],
                        Codec::Value,
                        move |cx| spawn(&symbols, cx),
                    ))
                }),
        )
    })?;
    module.inherent_generic(&task, |_, methods| {
        methods.define(MethodDecl::instance("cancel").returns(Type::unit())
            .documentation(example("Requests cooperative cancellation. Already completed Tasks keep their result.\nCancelling a waiter detaches its wait without cancelling the dependency.\nThe host supplies an open scope and drives both the cancellation and probe.\nThe cancelled task body in this example never performs its effect.", documentation::CANCEL_EXAMPLE))
            .implementation_send(NativeBinding::new(vec![Codec::Value], Codec::Value, cancel)))?;
        Ok(())
    })
}

fn spawn(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let scope = cx.argument(0)?;
    let factory = cx.argument(1)?;
    let result = cx.result_type_argument()?;
    match cx.spawn_task(&scope, &factory)? {
        Ok(task) => {
            let value = task
                .value(cx.heap())
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
            let _error = cx
                .heap()
                .root_value(value)
                .ok_or_else(|| RuntimeError::module_validation("SpawnError root"))?;
            enums::allocate(symbols, cx, &result, "Result", "Err", vec![value])
        }
    }
}

fn cancel(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.cancel_task(&cx.argument(0)?)?;
    Ok(Value::Unit)
}
