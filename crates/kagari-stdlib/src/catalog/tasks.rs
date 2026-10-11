//! Scope and Task storage roles are sealed runtime-owned capabilities.
use crate::catalog::{
    contracts::{enum_type, method, unit},
    identities,
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext, table::DefinitionId};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        TypeDef, TypeDefKind, VariantDef,
        module::{DeclarationError, ImplDecl, ModuleDecl},
        native::NativeStorageLayout,
    },
    ty::{GenericParam, NominalTy, Ty},
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for (name, layout) in [
        ("Task", NativeStorageLayout::Task),
        ("TaskScope", NativeStorageLayout::TaskScope),
    ] {
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::NativeStorage(layout),
            generic_params: if layout == NativeStorageLayout::Task {
                vec![GenericParam {
                    owner: identities::definition(ids, DefinitionKind::AssociatedType, name)?,
                    position: 0,
                }]
            } else {
                vec![]
            },
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        });
    }
    module.types.push(TypeDef {
        name: "SpawnError".into(),
        kind: TypeDefKind::Enum,
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: ["ScopeClosed", "CapacityExceeded", "DispatchUnavailable"]
            .into_iter()
            .map(|name| VariantDef {
                name: name.into(),
                payload: vec![],
                reports_failure: false,
            })
            .collect(),
    });
    spawn(ids, module)?;
    cancel(ids, module)?;

    Ok(())
}

fn storage(
    ids: &DefinitionContext,
    _module: &ModuleDecl,
    name: &str,
    arguments: Vec<Ty<DefinitionId>>,
) -> Result<Ty<DefinitionId>, DeclarationError> {
    Ok(Ty::NativeObject(NominalTy {
        declaration: identities::definition(ids, DefinitionKind::AssociatedType, name)?,
        arguments,
        associated_types: Default::default(),
    }))
}

fn spawn(ids: &DefinitionContext, module: &mut ModuleDecl) -> Result<(), DeclarationError> {
    let owner = identities::implementation(ids, module.implementations.len())?;
    let member = identities::method(ids, &owner, "spawn")?;
    let parameter = GenericParam {
        owner: member,
        position: 0,
    };
    let output = parameter.as_type();
    let receiver = storage(ids, module, "TaskScope", vec![])?;
    // The ordinary callable coercion accepts closures and arbitrary checked Fn
    // implementations, including a generic F: Fn() -> Future<T> argument.
    let factory = Ty::Function {
        params: vec![],
        result: Box::new(storage(ids, module, "Future", vec![output.clone()])?),
    };
    let result = enum_type(
        ids,
        "Result",
        vec![
            storage(ids, module, "Task", vec![output])?,
            enum_type(ids, "SpawnError", vec![])?,
        ],
    )?;
    let mut function = method("spawn", vec![receiver.clone(), factory], result);
    function.generic_params = vec![parameter];
    function.implementation = CallableImplementation::Native(identities::definition(
        ids,
        DefinitionKind::Function,
        "$foundation_task_spawn",
    )?);
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: receiver,
        methods: vec![function],
    });
    module.documentation.insert(member,
        "Admit a zero-argument factory returning Future<T>. The factory runs once on the first host drive, after admission. Closures and checked Fn implementations use ordinary callable coercion. Admission failures return SpawnError; execution failures terminate the Task.\n\n# Examples\n\n```kgr\nasync fn answer() -> i32 { 42 }\nfn submit(scope: TaskScope) -> Result<Task<i32>, SpawnError> { scope.spawn(|| answer()) }\n```".into());

    Ok(())
}

fn cancel(ids: &DefinitionContext, module: &mut ModuleDecl) -> Result<(), DeclarationError> {
    let owner = identities::implementation(ids, module.implementations.len())?;
    let member = identities::method(ids, &owner, "cancel")?;
    let parameter = GenericParam { owner, position: 0 };
    let receiver = storage(ids, module, "Task", vec![parameter.as_type()])?;
    let mut function = method("cancel", vec![receiver.clone()], unit());
    function.generic_params = vec![parameter.clone()];
    function.implementation = CallableImplementation::Native(identities::definition(
        ids,
        DefinitionKind::Function,
        "$foundation_task_cancel",
    )?);
    module.implementations.push(ImplDecl {
        generic_params: vec![parameter],
        bounds: vec![],
        trait_type: None,
        for_type: receiver,
        methods: vec![function],
    });
    module.documentation.insert(member,
        "Request cooperative cancellation of this Task. Completed Tasks are unchanged. Cancelling a waiting Task detaches its wait without cancelling the dependency.\n\n# Examples\n\n```kgr\nfn stop<T>(task: Task<T>) { task.cancel(); }\n```".into());

    Ok(())
}
