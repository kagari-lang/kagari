use super::{compile, insert};
use crate::{BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId};
use kagari_runtime::{
    error::RuntimeError,
    module::entry::ProgramEntry,
    native::{
        assembly::{
            ModuleDefinition, ModuleSetBuilder,
            draft::{ModuleDraft, TraitImplementation},
            references::DraftReference,
        },
        binding::NativeResult,
        declarations::MethodDecl,
    },
};
use kagari_types::ty::{GenericBound, Ty};

fn local_trait(module: &mut ModuleDraft<'_>, name: &str) -> NativeResult<DraftReference> {
    let id = module.definition(DefinitionKind::Trait, name)?;
    let path = module.definitions().resolve(id).unwrap().to_path();
    module.reference_trait(path, 1)
}

fn forwarding(module: &mut ModuleDraft<'_>, callee: &str) -> NativeResult<()> {
    let forward = local_trait(module, "Forward")?;
    let mut methods = Vec::new();
    for name in ["accept", "substitute"] {
        methods.push(
            module.trait_method(
                forward.id(),
                MethodDecl::static_method(name)
                    .parameter("value", forward.parameter(0)?)
                    .returns(forward.receiver()?),
            )?,
        );
    }
    module.define_trait("Forward", 1, |record, _| {
        record.methods = methods;
        Ok(())
    })?;
    let callee = module.reference_method(&forward, callee, 0)?;

    let reverse = local_trait(module, "Reverse")?;
    let method = module.trait_method(
        reverse.id(),
        MethodDecl::instance("reverse").returns(reverse.parameter(0)?),
    )?;
    module.define_trait("Reverse", 1, |record, _| {
        record.methods.push(method);
        Ok(())
    })?;
    let reverse_method = module.reference_method(&reverse, "reverse", 0)?;
    module.trait_implementation_with(2, |params| {
        let source = &params[0];
        let destination = params[1].clone();
        let constraint = forward.apply([source.clone()])?;
        let Ty::Trait(interface) = constraint.abi() else {
            unreachable!("Forward is a trait")
        };
        let implementation = TraitImplementation {
            receiver: source.clone(),
            interface: reverse.apply([destination.clone()])?,
            bounds: vec![GenericBound {
                ty: destination.abi().clone(),
                constraints: vec![interface.clone()],
            }],
            bindings: vec![],
        };
        Ok(
            implementation.with_script_method(reverse_method, move |scope| {
                scope.trait_call(&callee, destination, constraint, [], [scope.parameter(0)?])
            }),
        )
    })
}

fn engine(callee: &'static str) -> KagariEngine {
    let mut set = ModuleSetBuilder::new(&Default::default()).unwrap();
    set.add(ModuleDefinition::new(
        ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["forwarding".into()],
        },
        move |module| forwarding(module, callee),
    ))
    .unwrap();
    let modules = set.finish().unwrap().into_modules().unwrap();
    KagariEngine::with_native_modules(Default::default(), modules).unwrap()
}

fn product(engine: &KagariEngine) -> Program {
    let root = insert(
        engine,
        "caller",
        include_str!("../../../../tests/fixtures/language/blanket_traits/registered-caller.kgr"),
    );
    let artifact = compile(engine, root);
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    Program::from_artifact(
        BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn execute(engine: &KagariEngine, program: &Program) -> i32 {
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(program, Default::default()).unwrap();
    runtime
        .execute_typed(&loaded, ProgramEntry::Main, (), &context)
        .unwrap()
}

#[test]
fn registered_blanket_forwarding_authenticates_the_selected_member() {
    let installed = engine("accept");
    assert_eq!(execute(&installed, &product(&installed)), 42);

    // This producer changes only the member named by the Script expression. Its
    // product is internally valid and even executes with that producer installed.
    let other = engine("substitute");
    let replacement = product(&other);
    assert_eq!(execute(&other, &replacement), 17);
    let mut runtime = installed.runtime(Default::default());
    let error = runtime
        .load_program(&replacement, Default::default())
        .unwrap_err();
    assert_eq!(
        error.reason(),
        RuntimeError::module_validation(
            "Script executable differs from its installed canonical body"
        )
        .reason()
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
}
