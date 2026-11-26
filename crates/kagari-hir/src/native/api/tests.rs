use super::*;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{AnalysisDatabase, core_source::packaged_modules},
    hir::origin::NodeOrigin,
    native::{adoption::adopt, api::preparation::NativeFiles},
    tests::native_fixture::{function_module, module},
};
use kagari_common::identity::{
    PackageId, map::DefinitionContext, mapping::DefinitionMappingError,
    scoped::DefinitionReferences, table::DefinitionTableError,
};
use kagari_source::source::SourceFile;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    scalar::BuiltinType,
    ty::{GenericBound, NominalTy, Ty},
};
use std::sync::Arc;

fn lower_last(providers: Vec<Arc<DefinitionMetadata<ModuleDecl>>>) -> Arc<LoweredModule> {
    let paths = Arc::new(LibraryPaths::new(&providers, packaged_modules()));
    NativeFiles::new(providers, paths, "/native", &Default::default())
        .unwrap()
        .attach(&Default::default())
        .unwrap()
        .pop()
        .unwrap()
}

#[test]
fn structured_lowering_preserves_ordered_requirements_concrete_results_and_default_owners() {
    let (definitions, mut definition) = module().as_ref().clone().into_parts();
    let template = definition
        .definition(DefinitionKind::Function, "default_read", &definitions)
        .unwrap();
    let interface = definition
        .definition(DefinitionKind::Trait, "NativeRead", &definitions)
        .unwrap();
    let requirements = ["read", "fixed", "read"].map(|name| NativeCallableRequirement {
        receiver: definition
            .functions
            .iter()
            .find(|function| function.name == "default_read")
            .unwrap()
            .generic_params[0]
            .as_type(),
        interface: NominalTy {
            declaration: interface,
            arguments: vec![],
            associated_types: Default::default(),
        },
        member: ModuleDecl::method_id(interface, name, &definitions).unwrap(),
        arguments: vec![],
    });
    let echo = definition
        .functions
        .iter_mut()
        .find(|function| function.name == "echo")
        .unwrap();
    let result = echo.generic_params[0].as_type();
    echo.return_type = Ty::Trait(requirements[0].interface.clone());
    echo.bounds.push(GenericBound {
        ty: result.clone(),
        constraints: vec![requirements[0].interface.clone()],
    });
    definition
        .concrete_results
        .insert(echo.generic_params[0].owner, result);
    definition
        .callable_requirements
        .insert(template, requirements.into());
    let module = Arc::new(
        DefinitionMetadata::checked(definitions, definition, &Default::default()).unwrap(),
    );
    let mut providers = standard_modules();
    providers.push(module);
    let providers = adopt(
        &providers,
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    let module = providers.last().unwrap();
    let lowered = lower_last(providers.clone());
    assert!(lowered.source_surface().is_none());
    assert_eq!(lowered.module.traits.len(), 1);
    assert_eq!(lowered.module.traits[0].methods.len(), 2);
    assert!(
        lowered.module.traits[0]
            .methods
            .iter()
            .all(|method| method.has_default)
    );
    {
        let foreign = DefinitionContext::new()
            .unwrap()
            .intern_root(lowered.source.module_identity())
            .unwrap();
        let mut forged = lowered.as_ref().clone();
        forged.module.traits[0].methods[0].origin = NodeOrigin::Structured {
            declaration: foreign,
        };
        let error = forged
            .visit_definition_ids(
                &mut |id| {
                    if *id == foreign {
                        module.definitions().resolve(*id)?;
                    }
                    Ok(())
                },
                &Default::default(),
            )
            .unwrap_err();
        assert_eq!(
            error,
            DefinitionMappingError::Identity(DefinitionTableError::ForeignTable)
        );
    }
    let natives = lowered
        .module
        .functions
        .iter()
        .filter(|function| matches!(function.implementation, FunctionImplementation::Native(_)))
        .collect::<Vec<_>>();
    assert_eq!(natives.len(), 4);
    for function in natives {
        let entry = module
            .records()
            .native_declarations(module.definitions())
            .unwrap()
            .into_iter()
            .find(|declaration| declaration.function.name == function.name)
            .unwrap()
            .declaration;
        let FunctionImplementation::Native(contract) = &function.implementation else {
            panic!("installed entry must own a Native implementation");
        };
        assert_eq!(
            contract.callable_requirements.as_slice(),
            module
                .records()
                .callable_requirements
                .get(&entry)
                .map(Vec::as_slice)
                .unwrap_or_default()
        );
        assert_eq!(
            contract.concrete_result.as_ref(),
            module.records().concrete_results.get(&entry)
        );
        if function.name == "default_read" {
            assert_eq!(contract.callable_requirements.len(), 3);
            assert_ne!(
                contract.callable_requirements[0],
                contract.callable_requirements[1]
            );
            assert_eq!(
                contract.callable_requirements[0],
                contract.callable_requirements[2]
            );
        }
        if function.name == "echo" {
            assert!(matches!(
                contract.concrete_result,
                Some(Ty::Parameter { .. })
            ));
        }
    }
    for function in &lowered.module.functions {
        if function.implementation.script_body().is_some() {
            let FunctionImplementation::Script(body) = function.implementation else {
                panic!("native default must retain its forwarding proof body");
            };
            let call = lowered.module.block(body).tail_expr.unwrap();
            let crate::hir::expr::ExprKind::Call {
                args, type_args, ..
            } = &lowered.module.expr(call).kind
            else {
                panic!("forwarding default must be a real checked call");
            };
            assert_eq!(args.len(), function.params.len());
            assert_eq!(type_args.as_ref().map_or(0, |arguments| arguments.len()), 1);
            let origin = lowered.module.expr(call).origin;
            assert_eq!(origin, function.origin);
            assert!(matches!(origin, NodeOrigin::Structured { .. }));
        }
    }
}

#[test]
fn registration_keys_keep_opaque_package_and_module_path_distinct() {
    let module = |package: &str, path: &[&str]| ModuleIdentity {
        package: PackageId(package.into()),
        path: path.iter().map(|part| (*part).into()).collect(),
    };
    let cases = [
        (module("a/b", &["c"]), "left"),
        (module("a", &["b", "c"]), "right"),
        (module("a%2Fb", &["c"]), "literal_escape"),
    ];
    let mut db = AnalysisDatabase::default();
    db.set_native_modules(
        cases
            .iter()
            .map(|(owner, name)| {
                function_module(owner.clone(), name, vec![], Ty::Builtin(BuiltinType::I32))
            })
            .collect(),
    );
    let keys = |db: &mut AnalysisDatabase| {
        let snapshot = db
            .declarations(Default::default(), &Default::default())
            .unwrap();
        let ids = cases.each_ref().map(|(owner, name)| {
            let file = snapshot
                .files()
                .find(|file| file.source().module_identity() == owner)
                .unwrap();
            assert!(file.lowered().source_surface().is_none());
            assert_eq!(file.lowered().module.functions[0].name, *name);
            file.source().id()
        });
        assert_eq!(BTreeSet::from(ids).len(), cases.len());
        ids
    };
    let first = keys(&mut db);
    assert_eq!(keys(&mut db), first);
    // Replaying inputs preserves owners and declarations, not session-local FileIds.
    keys(&mut db.inputs().database().unwrap());
}

#[test]
fn one_source_analysis_reports_invalid_registration_as_an_error() {
    let (definitions, mut invalid) = module().as_ref().clone().into_parts();
    invalid.functions.push(invalid.functions[0].clone());
    let source = SourceFile::new("invalid-registration.kgr", "fn main() {}");
    let invalid = DefinitionMetadata::checked(definitions, invalid, &Default::default()).unwrap();
    let result = crate::analyze_source(&source, vec![Arc::new(invalid)]);
    assert!(matches!(
        result,
        Err(crate::analysis::error::AnalysisError::NativeApi(_))
    ));
}

mod authored_facade;
mod exact;
mod fields;
mod forwarding;
mod header_predicates;
mod health;
mod host;
mod orphan;
mod reference;
mod script;

#[test]
fn native_trait_defaults_remain_native_without_script_bodies() {
    let (definitions, mut definition) = module().as_ref().clone().into_parts();
    let owner = definition
        .definition(DefinitionKind::Trait, "NativeRead", &definitions)
        .unwrap();
    let method = ModuleDecl::method_id(owner, "read", &definitions).unwrap();
    definition.traits[0].methods[0].implementation = CallableImplementation::Native(method);
    definition.script_bodies.remove(&method);
    let metadata = Arc::new(
        DefinitionMetadata::checked(definitions, definition, &Default::default()).unwrap(),
    );
    assert!(
        metadata
            .records()
            .native_declarations(metadata.definitions())
            .unwrap()
            .iter()
            .any(|declaration| declaration.declaration == method)
    );
    let mut providers = standard_modules();
    providers.push(metadata.clone());
    let providers = adopt(
        &providers,
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    let lowered = lower_last(providers);
    let method = lowered.module.traits[0]
        .methods
        .iter()
        .find(|method| method.name == "read")
        .unwrap();
    assert!(method.has_default);
    assert!(matches!(
        lowered.module.functions[method.function.index()].implementation,
        FunctionImplementation::Native(_)
    ));
    assert!(
        lowered.module.functions[method.function.index()]
            .implementation
            .script_body()
            .is_none()
    );
}
