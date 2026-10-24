use super::*;
use crate::test_support::native::program::ProgramFixture;
use kagari_contract::{
    layout::{StructFieldLayout, StructLayout},
    types::TraitContract,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param, TraitDef},
    ty::NominalTy,
};

fn boxed(context: &DefinitionContext, module: &BytecodeModule, item: Ty) -> StructLayout {
    let root = context.intern_root(&module.identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Struct, "Box", 0)
        .unwrap();
    let nominal = Ty::Struct(NominalTy {
        declaration,
        arguments: vec![item.clone()],
        associated_types: Default::default(),
    });
    StructLayout {
        declaration,
        arguments: vec![item.clone()],
        fields: vec![
            StructFieldLayout {
                declaration: context
                    .intern_child(declaration, DefinitionKind::Field, "value", 0)
                    .unwrap(),
                name: "value".into(),
                ty: item,
                mutable: true,
                visibility: Visibility::Private,
            },
            StructFieldLayout {
                declaration: context
                    .intern_child(declaration, DefinitionKind::Field, "next", 0)
                    .unwrap(),
                name: "next".into(),
                ty: Ty::Array(Box::new(nominal)),
                mutable: true,
                visibility: Visibility::Private,
            },
        ],
    }
}

fn fixture(context: &DefinitionContext) -> BytecodeProgram {
    let mut module = BytecodeModule::default();
    let layout = boxed(context, &module, scalar());
    let ty = Ty::Struct(NominalTy {
        declaration: layout.declaration,
        arguments: layout.arguments.clone(),
        associated_types: Default::default(),
    });
    module.structures.push(layout);
    add_static(
        context,
        &mut module,
        "integers",
        ty,
        true,
        Visibility::Private,
    );
    ProgramFixture {
        root: ModuleRef::new(0),
        modules: vec![module],
        exports: Default::default(),
    }
    .build(context)
}

fn metadata(
    context: &DefinitionContext,
    code: BytecodeProgram,
) -> DefinitionMetadata<BytecodeProgram> {
    ProgramFixture::from(code)
        .metadata(context.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn adding_an_unrelated_specialization_and_reordering_layouts_preserves_cells() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let mut code = fixture(&context);
    let old = runtime
        .load_program("boxes", metadata(&context, code.clone()))
        .unwrap();
    let boolean = boxed(&context, &code.modules[0], Ty::Builtin(BuiltinType::Bool));
    let ty = Ty::Struct(NominalTy {
        declaration: boolean.declaration,
        arguments: boolean.arguments.clone(),
        associated_types: Default::default(),
    });
    code.modules[0].structures.insert(0, boolean);
    add_static(
        &context,
        &mut code.modules[0],
        "booleans",
        ty,
        false,
        Visibility::Private,
    );
    let candidate = runtime
        .stage_reload_program(&old, "boxes", metadata(&context, code.clone()))
        .expect("adding Box<bool> must not change the Box<i32> storage contract");
    let current = runtime.publish_staged_reload(candidate).unwrap();
    assert_eq!(
        cell(&runtime.modules, &old, 0),
        cell(&runtime.modules, &current, 0)
    );
    assert_ne!(
        cell(&runtime.modules, &current, 0),
        cell(&runtime.modules, &current, 1)
    );
    code.modules[0].structures.reverse();
    let candidate = runtime
        .stage_reload_program(&current, "boxes", metadata(&context, code))
        .expect("physical layout order is not the applied schema");
    let next = runtime.publish_staged_reload(candidate).unwrap();
    for slot in 0..2 {
        assert_eq!(
            cell(&runtime.modules, &current, slot),
            cell(&runtime.modules, &next, slot)
        );
    }
}

#[test]
fn private_interface_signatures_follow_applied_payloads_not_layout_inventory() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let mut code = fixture(&context);
    let module = &mut code.modules[0];
    let root = context.intern_root(&module.identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Trait, "Factory", 0)
        .unwrap();
    context
        .intern_child(declaration, DefinitionKind::Method, "make", 0)
        .unwrap();
    let output = module.module_slots[0].contract.ty.clone();
    module.trait_contracts.push(TraitContract {
        declaration,
        abi: TraitDef {
            name: "Factory".into(),
            generic_params: vec![],
            bounds: vec![],
            supertraits: vec![],
            associated_consts: vec![],
            associated_types: vec![],
            conversion_adapter: None,
            methods: vec![FnDecl {
                name: "make".into(),
                implementation: CallableImplementation::Required,
                host_policy: None,
                method_policy: Default::default(),
                generic_params: vec![],
                bounds: vec![],
                params: vec![Param {
                    name: "self".into(),
                    ty: Ty::SelfType(declaration),
                    mutable: false,
                }],
                return_type: output,
            }],
        },
    });
    let ty = Ty::Trait(NominalTy {
        declaration,
        arguments: vec![],
        associated_types: Default::default(),
    });
    module.module_slots[0].contract.ty = ty.clone();
    module.functions[0].metadata.semantic.result = Some(ty);
    let old = runtime
        .load_program("factory", metadata(&context, code.clone()))
        .unwrap();
    let boolean = boxed(&context, &code.modules[0], Ty::Builtin(BuiltinType::Bool));
    code.modules[0].structures.insert(0, boolean);
    let candidate = runtime
        .stage_reload_program(&old, "factory", metadata(&context, code.clone()))
        .unwrap();
    let current = runtime.publish_staged_reload(candidate).unwrap();
    for contract in [false, true] {
        let mut changed = code.clone();
        if contract {
            changed.modules[0].trait_contracts[0].abi.methods[0].return_type =
                Ty::Builtin(BuiltinType::Bool);
        } else {
            changed.modules[0].structures[1].fields[0].mutable = false;
        }
        VerifiedProgram::new(metadata(&context, changed.clone()), &context).unwrap();
        assert!(
            runtime
                .stage_reload_program(&current, "factory", metadata(&context, changed))
                .is_err()
        );
    }
    assert_eq!(
        runtime.modules.latest("factory").unwrap().key(),
        current.key()
    );
}

#[test]
fn selected_specialization_and_recursive_payload_changes_are_rejected() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let code = fixture(&context);
    let old = runtime
        .load_program("boxes", metadata(&context, code.clone()))
        .unwrap();
    for recursive in [false, true] {
        let mut changed = code.clone();
        let boolean = boxed(
            &context,
            &changed.modules[0],
            Ty::Builtin(BuiltinType::Bool),
        );
        changed.modules[0].structures.push(boolean.clone());
        let layout = &mut changed.modules[0].structures[0];
        if recursive {
            layout.fields[1].ty = Ty::Array(Box::new(Ty::Struct(NominalTy {
                declaration: boolean.declaration,
                arguments: boolean.arguments,
                associated_types: Default::default(),
            })));
        } else {
            layout.fields[0].ty = Ty::Builtin(BuiltinType::Bool);
        }
        VerifiedProgram::new(metadata(&context, changed.clone()), &context).unwrap();
        assert!(
            runtime
                .stage_reload_program(&old, "boxes", metadata(&context, changed))
                .is_err()
        );
        assert_eq!(runtime.modules.latest("boxes").unwrap().key(), old.key());
    }
}
