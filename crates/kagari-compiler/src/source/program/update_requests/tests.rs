use std::slice;

use super::*;
use crate::source::lower::{
    instances::MirLoweringOptions, lower_to_mir, lower_to_mir_with_requests, module_contract,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        interface::InterfaceCallContract,
        shared::SharedCall,
    },
    types::named::NamedContract,
    updates::{
        BodyApplication, CallRequirement, UpdateEvidence,
        demands::{Coverage, expand_requests},
        limits::Budget,
    },
};
use kagari_hir::{CheckedAnalysis, analyze_source};
use kagari_source::source::SourceFile;
use kagari_stdlib::catalog;
use kagari_types::{
    callable::{CallableImplementation, Signature},
    scalar::BuiltinType,
    ty::NominalTy,
};

fn checked(source: &str) -> CheckedAnalysis {
    analyze_source(
        &SourceFile::new("retained.kgr", source),
        catalog::shared().unwrap(),
    )
    .unwrap()
    .into_codegen()
    .unwrap()
}

fn inventory(module: &CheckedAnalysis) -> Vec<NamedDeclaration> {
    module_contract(module, &Default::default())
        .unwrap()
        .into_records()
        .named_declarations
}

fn named(module: &CheckedAnalysis, name: &str) -> DefinitionId {
    let contract = module_contract(module, &Default::default())
        .unwrap()
        .into_records();
    contract
        .named_declarations
        .iter()
        .map(|item| item.declaration)
        .chain(contract.private_types.iter().map(|item| item.declaration))
        .chain(contract.trait_contracts.iter().map(|item| item.declaration))
        .find(|id| {
            module
                .definitions()
                .resolve(*id)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == name)
        })
        .unwrap_or_else(|| panic!("missing checked declaration {name}"))
}

#[test]
fn contract_batches_materialize_all_five_products_without_specializing_shared_bodies() {
    let checked = checked(include_str!(
        "../../../../tests/fixtures/updates/demand-products.kgr"
    ));
    let definitions = checked.definitions();
    let i32_ty = Ty::Builtin(BuiltinType::I32);
    let i64_ty = Ty::Builtin(BuiltinType::I64);
    let nominal = |declaration, arguments| NominalTy {
        declaration,
        arguments,
        associated_types: Default::default(),
    };
    let receiver = Ty::Struct(nominal(named(&checked, "Box"), vec![i32_ty.clone()]));
    let layout = Ty::Struct(nominal(named(&checked, "Unused"), vec![i64_ty.clone()]));
    let interface = nominal(named(&checked, "Read"), vec![]);
    let implementation = inventory(&checked).into_iter().find(|item| matches!(&item.contract, NamedContract::Implementation(contract) if contract.trait_type == Some(Ty::Trait(interface.clone())))).unwrap().declaration;
    let function = ConcreteFunctionIdentity {
        declaration: named(&checked, "retained"),
        arguments: vec![i32_ty.clone()],
    };
    let shared_id = named(&checked, "shared");
    let shared = ConcreteFunctionIdentity {
        declaration: shared_id,
        arguments: vec![],
    };
    let construction = ConcreteFunctionIdentity {
        declaration: implementation,
        arguments: vec![i32_ty.clone()],
    };
    let evidence = UpdateEvidence {
        source_manifest: None,
        sources: vec![],
        bodies: vec![],
        initializers: vec![],
        targets: vec![],
        selections: vec![],
    };
    let coverage = Coverage {
        evidence: &evidence,
        methods: &[],
        constructions: &[],
        layouts: &[],
        natives: &[],
    };
    let envelope = DemandEnvelope {
        requirements: vec![
            CallRequirement::Direct(function.clone()),
            CallRequirement::Shared(Box::new(SharedCall {
                instance: shared.clone(),
                implementation: CallableImplementation::Script,
                arguments: vec![i64_ty.clone()],
                signature: Signature {
                    params: vec![i64_ty.clone()],
                    result: i64_ty.clone(),
                },
                receiver_access: None,
                operations: vec![],
                constructions: vec![],
            })),
            CallRequirement::Interface(Box::new(InterfaceCallContract {
                interface: interface.clone(),
                receiver: Some(receiver.clone()),
                method_slot: 0,
                arguments: vec![],
                operations: vec![],
                constructions: vec![],
                normalizations: vec![],
            })),
            CallRequirement::Construction(Box::new(ConstructionWitness {
                requirement: ConstructionRequirement {
                    receiver,
                    interface,
                },
                selection: ConstructionSelection::Declared {
                    implementation: construction.clone(),
                    operations: vec![],
                    constructions: vec![],
                },
            })),
            CallRequirement::Layout(layout.clone()),
        ],
        ..Default::default()
    };
    let cancel = CancellationToken::default();
    let mut budget = Budget::new(Default::default(), &cancel).unwrap();
    let initial = lower_to_mir(&checked, &Default::default()).unwrap();
    let proofs = proof_catalog(
        slice::from_ref(&initial),
        &checked.definition_context().snapshot(),
        &cancel,
    )
    .unwrap();
    let batch = expand_requests(&envelope, &coverage, &proofs, &mut budget).unwrap();
    assert_eq!(batch.functions.as_slice(), slice::from_ref(&function));
    assert_eq!(
        batch.shared,
        [BodyApplication {
            body: shared.clone(),
            arguments: vec![i64_ty]
        }]
    );
    assert_eq!(batch.interfaces.len(), 1);
    assert_eq!(
        batch.constructions.as_slice(),
        slice::from_ref(&construction)
    );
    assert_eq!(batch.layouts.as_slice(), slice::from_ref(&layout));
    let mut queue = RequestQueue::default();
    let context = checked.definition_context();
    let module = checked.to_unverified(&cancel).unwrap().into_records();
    let owner = definitions
        .lookup_module(module.lowered.source.module_identity())
        .unwrap();
    assert!(
        queue
            .apply(batch, &module.aggregates, definitions, owner, &cancel)
            .unwrap()
    );
    let options = MirLoweringOptions::default();
    let result = lower_to_mir_with_requests(
        &module,
        &options,
        queue.module(owner).unwrap(),
        std::slice::from_ref(&module),
        &module.aggregates,
        &context,
    )
    .unwrap();
    assert!(
        result
            .module
            .functions
            .iter()
            .any(|body| body.instance == function)
    );
    let bodies = result
        .module
        .functions
        .iter()
        .filter(|body| body.instance.declaration == shared_id)
        .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].instance, shared);
    assert_eq!(
        bodies[0]
            .semantic
            .generic
            .as_ref()
            .unwrap()
            .parameters
            .len(),
        1
    );
    assert!(result.module.interface_instances.contains(&construction));
    let read = result
        .module
        .functions
        .iter()
        .find(|body| {
            context
                .snapshot()
                .resolve(body.instance.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "read")
        })
        .unwrap();
    assert_eq!(read.instance.arguments, [i32_ty]);
    assert!(read.semantic.generic.is_none());
    let Ty::Struct(layout) = layout else {
        unreachable!()
    };
    assert!(
        result.module.structures.iter().any(
            |item| item.declaration == layout.declaration && item.arguments == layout.arguments
        )
    );
    let limited = MirLoweringOptions {
        max_generic_instances: 0,
        ..Default::default()
    };
    assert!(
        lower_to_mir_with_requests(
            &module,
            &limited,
            queue.module(owner).unwrap(),
            std::slice::from_ref(&module),
            &module.aggregates,
            &context
        )
        .is_err()
    );
}

#[test]
fn shared_receiver_and_method_binder_identity_remains_exact() {
    let checked = checked(include_str!(
        "../../../../tests/fixtures/updates/shared-binders.kgr"
    ));
    let method = inventory(&checked)
        .into_iter()
        .find(|record| {
            let path = checked
                .definitions()
                .resolve(record.declaration)
                .unwrap()
                .to_path();
            path.path
                .first()
                .is_some_and(|part| part.kind == DefinitionKind::Impl)
                && path.path.last().is_some_and(|part| part.name == "read")
        })
        .unwrap()
        .declaration;
    let identity = ConcreteFunctionIdentity {
        declaration: method,
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
    };
    let requests = DemandRequests {
        shared: vec![BodyApplication {
            body: identity.clone(),
            arguments: vec![Ty::Builtin(BuiltinType::I64)],
        }],
        ..Default::default()
    };
    let context = checked.definition_context();
    let module = checked
        .to_unverified(&Default::default())
        .unwrap()
        .into_records();
    let lowered = lower_to_mir_with_requests(
        &module,
        &Default::default(),
        &requests,
        std::slice::from_ref(&module),
        &module.aggregates,
        &context,
    )
    .unwrap();
    let method = lowered
        .module
        .functions
        .iter()
        .find(|body| body.instance.declaration == method)
        .unwrap();
    assert_eq!(method.instance, identity);
    assert_eq!(
        method.semantic.generic.as_ref().unwrap().parameters.len(),
        1
    );
    assert_eq!(
        method.semantic.params[&1],
        method.semantic.generic.as_ref().unwrap().parameters[0].as_type()
    );
}

#[test]
fn retained_impl_members_follow_semantic_headers_after_reordering() {
    let before = checked(
        "struct A {} struct B {} impl A { fn keep<T>(x: T) -> T { x } } impl B { fn other<T>(x: T) -> T { x } }",
    );
    let after = checked(
        "struct A {} struct B {} impl B { fn other<T>(x: T) -> T { x } } impl A { fn keep<T>(x: T) -> T { x } }",
    );
    let old = named(&before, "keep");
    let new = named(&after, "keep");
    assert_ne!(
        before.definitions().resolve(old).unwrap().to_path(),
        after.definitions().resolve(new).unwrap().to_path()
    );
    let history = DefinitionMetadata::checked(
        before.definitions().clone(),
        DemandEnvelope {
            requirements: vec![CallRequirement::Direct(ConcreteFunctionIdentity {
                declaration: old,
                arguments: vec![Ty::Builtin(BuiltinType::I32)],
            })],
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    let imported = import_retained(
        &history,
        &inventory(&before),
        &inventory(&after),
        &after.definition_context(),
        &Default::default(),
    )
    .unwrap();
    assert!(
        matches!(&imported.requirements[0], CallRequirement::Direct(target) if target.declaration == new)
    );
    let changed = checked("struct A {} struct B {} impl B { fn keep<T>(x: T) -> T { x } }");
    assert!(
        import_retained(
            &history,
            &inventory(&before),
            &inventory(&changed),
            &changed.definition_context(),
            &Default::default()
        )
        .is_err()
    );
}
