use super::{bound, owner, path};
use crate::native::{
    assembly::{
        ModuleDefinition, ModuleSetBuilder,
        draft::{ModuleDraft, TraitImplementation},
        references::{DraftMember, DraftReference},
    },
    binding::NativeResult,
    catalog::DeclarationCatalog,
    declarations::MethodDecl,
};
use kagari_common::identity::DefinitionKind;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{AssociatedTypeDef, module::ModuleDecl, script::ScriptExpr},
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};

fn contract(
    module: &mut ModuleDraft<'_>,
    name: &str,
    instance: bool,
) -> NativeResult<(DraftReference, DraftMember, DraftMember)> {
    let interface = module.reference_trait(path("contracts", DefinitionKind::Trait, name), 1)?;
    let output = module.reference_associated_type(&interface, "Output", 0)?;
    let member = module.reference_method(&interface, "convert", 1)?;
    let input = interface.parameter(0)?;
    let result = output.projection(interface.receiver()?, interface.apply([input.clone()])?, [])?;
    let signature = if instance {
        MethodDecl::instance("convert")
    } else {
        MethodDecl::static_method("convert").parameter("input", input)
    };
    let mut signature = module.trait_method(
        interface.id(),
        signature
            .parameter("tag", member.parameter(0)?)
            .returns(result),
    )?;
    signature.generic_params.push(GenericParam {
        owner: member.id(),
        position: 0,
    });
    module.define_trait(name, 1, |record, _| {
        record.associated_types.push(AssociatedTypeDef {
            declaration: output.id(),
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: vec![],
        });
        record.methods.push(signature);
        Ok(())
    })?;
    Ok((interface, member, output))
}

#[test]
fn script_override_forwards_with_distinct_impl_and_method_binders() {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("contracts"), |module| {
        let (forward, convert, projected) = contract(module, "Forward", false)?;
        let (reverse, method, output) = contract(module, "Reverse", true)?;
        module.trait_implementation_with(2, |types| {
            let source = types[0].clone();
            let destination = types[1].clone();
            let applied = forward.apply([source.clone()])?;
            let result = projected.projection(destination.clone(), applied.clone(), [])?;
            Ok(TraitImplementation {
                receiver: source,
                interface: output.bind(reverse.apply([destination.clone()])?, result)?,
                bounds: vec![bound(&destination, applied.clone())],
                bindings: vec![],
            }
            .with_script_method(method, move |body| {
                body.trait_call(
                    &convert,
                    destination,
                    applied,
                    [body.type_parameter(0)?],
                    [body.parameter(0)?, body.parameter(1)?],
                )
            }))
        })
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let prepared = assembly.try_into_send().unwrap().into_modules().unwrap();
    let definitions = prepared[0].definitions();
    let module = prepared[0].declaration();
    let implementation = &module.implementations[0];
    let impl_id = module.implementation_id(0, definitions).unwrap();
    let method_id = ModuleDecl::method_id(impl_id, "convert", definitions).unwrap();
    let parameter = |owner, position| Ty::Parameter { owner, position };
    let source = parameter(impl_id, 0);
    let destination = parameter(impl_id, 1);
    let tag = parameter(method_id, 0);
    let forward = module
        .definition(DefinitionKind::Trait, "Forward", definitions)
        .unwrap();
    let interface = NominalTy {
        declaration: forward,
        arguments: vec![source.clone()],
        associated_types: Default::default(),
    };
    let expected_output = Ty::Projection {
        receiver: Box::new(destination.clone()),
        interface: Box::new(interface.clone()),
        member: definitions
            .lookup_child(forward, DefinitionKind::AssociatedType, "Output", 0)
            .unwrap(),
        arguments: vec![],
    };
    assert_eq!(implementation.for_type, source);
    assert_eq!(
        implementation.bounds,
        [GenericBound {
            ty: destination.clone(),
            constraints: vec![interface.clone()]
        }]
    );
    let reverse = implementation.trait_type.as_ref().unwrap();
    let output = definitions
        .lookup_child(
            reverse.declaration,
            DefinitionKind::AssociatedType,
            "Output",
            0,
        )
        .unwrap();
    assert_eq!(reverse.associated_types[&output], expected_output);
    let method = &implementation.methods[0];
    assert_eq!(method.return_type, expected_output);
    assert_eq!(method.params[1].ty, tag);
    assert_eq!(method.implementation, CallableImplementation::Script);
    assert_eq!(
        module.script_bodies[&method_id],
        ScriptExpr::Call {
            callee: Box::new(ScriptExpr::TraitMethod {
                receiver: destination,
                interface,
                member: ModuleDecl::method_id(forward, "convert", definitions).unwrap()
            }),
            type_arguments: vec![tag],
            arguments: vec![
                ScriptExpr::Name("self".into()),
                ScriptExpr::Name("tag".into())
            ],
        }
    );
    assert!(module.native_declarations(definitions).unwrap().is_empty());
}
