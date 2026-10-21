use crate::callable::construction::{ConstructionBody, ConstructionStep, closed_constructions};
use crate::{
    callable::{
        construction::{
            evidence::{ConstructionRequirement, ConstructionSelection, ConstructionWitness},
            requirements::required_constructions,
        },
        witness::required_operations,
    },
    types::{
        ConcreteFunctionIdentity, InterfaceBridge, InterfaceTable,
        proofs::{ProofCatalog, implementation::Implementation},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId},
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{AssociatedTypeDef, FnDecl, Param, TraitDef},
    scalar::BuiltinType,
    ty::{
        GenericBound, GenericParam, NominalTy, Ty,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use std::slice;

struct Fixture {
    context: DefinitionContext,
    marker: DefinitionId,
    contract: TraitDef,
    parameter: GenericParam,
}

impl Fixture {
    fn new() -> Self {
        let context = DefinitionContext::new().unwrap();
        let root = context
            .intern_root(&ModuleIdentity::single_file("construction.kgr"))
            .unwrap();
        let marker = context
            .intern_child(root, DefinitionKind::Trait, "Marker", 0)
            .unwrap();
        let owner = context
            .intern_child(root, DefinitionKind::Function, "erase", 0)
            .unwrap();
        Self {
            context,
            marker,
            parameter: GenericParam { owner, position: 0 },
            contract: TraitDef {
                name: "Marker".into(),
                conversion_adapter: None,
                generic_params: vec![],
                bounds: vec![],
                supertraits: vec![],
                associated_types: vec![],
                associated_consts: vec![],
                methods: vec![],
            },
        }
    }

    fn catalog(&self) -> ProofCatalog<'_> {
        ProofCatalog::new(
            vec![],
            [],
            [(self.marker, &self.contract)],
            [],
            &self.context.snapshot(),
            &Default::default(),
        )
        .unwrap()
    }

    fn interface(&self) -> NominalTy {
        NominalTy {
            declaration: self.marker,
            arguments: vec![],
            associated_types: Default::default(),
        }
    }

    fn bound(&self) -> GenericBound {
        GenericBound {
            ty: self.parameter.as_type(),
            constraints: vec![self.interface()],
        }
    }
}

#[test]
fn methodless_construction_requires_exact_selected_or_forwarded_evidence() {
    let fixture = Fixture::new();
    let cancel = CancellationToken::default();
    let definitions = fixture.context.snapshot();
    let lookup = |id: &DefinitionId| (*id == fixture.marker).then_some(&fixture.contract);
    let bounds = [fixture.bound()];
    let parameters = slice::from_ref(&fixture.parameter);
    assert!(
        required_operations(&bounds, &lookup, &cancel, &definitions)
            .unwrap()
            .is_empty()
    );
    let requirements =
        required_constructions(&bounds, parameters, &fixture.catalog(), &cancel).unwrap();
    assert_eq!(
        requirements,
        [ConstructionRequirement {
            receiver: fixture.parameter.as_type(),
            interface: fixture.interface()
        }]
    );
    let catalog = ProofCatalog::new(
        vec![],
        [],
        [(fixture.marker, &fixture.contract)],
        [],
        &definitions,
        &cancel,
    )
    .unwrap();
    let forwarded = ConstructionWitness {
        requirement: requirements[0].clone(),
        selection: ConstructionSelection::Forward,
    };
    assert!(
        forwarded
            .valid(&catalog, parameters, &bounds, &cancel)
            .unwrap()
    );
    assert!(!forwarded.valid(&catalog, parameters, &[], &cancel).unwrap());
    assert!(!forwarded.valid(&catalog, &[], &bounds, &cancel).unwrap());
    let root = fixture
        .context
        .intern_root(&ModuleIdentity::single_file("construction.kgr"))
        .unwrap();
    let declaration = fixture
        .context
        .intern_child(root, DefinitionKind::Impl, "MarkerForI32", 0)
        .unwrap();
    let table = InterfaceTable {
        declaration,
        name: "MarkerForI32".into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: Ty::Trait(fixture.interface()),
        for_type: Ty::Builtin(BuiltinType::I32),
        methods: vec![],
        associated_consts: vec![],
        associated_type_families: vec![],
        bridge: None,
    };
    let definitions = fixture.context.snapshot();
    let catalog = ProofCatalog::new(
        vec![Implementation::Interface(&table)],
        [],
        [(fixture.marker, &fixture.contract)],
        [],
        &definitions,
        &cancel,
    )
    .unwrap();
    let mut substitution = TypeSubstitution::default();
    substitution.bind(&fixture.parameter.owner, 0, &table.for_type);
    let mut selected = ConstructionWitness {
        requirement: forwarded.requirement.apply(&substitution, &cancel).unwrap(),
        selection: ConstructionSelection::Declared {
            implementation: ConcreteFunctionIdentity {
                declaration,
                arguments: vec![],
            },
            operations: vec![],
            constructions: vec![],
        },
    };
    assert!(selected.valid(&catalog, &[], &[], &cancel).unwrap());
    let mut forged_bridge = table.clone();
    forged_bridge.bridge = Some(InterfaceBridge::Intrinsic);
    let bridge_catalog = ProofCatalog::new(
        vec![Implementation::Interface(&forged_bridge)],
        [],
        [(fixture.marker, &fixture.contract)],
        [],
        &definitions,
        &cancel,
    )
    .unwrap();
    // Retaining a bridge cannot invent conformance or authorize its construction.
    assert!(
        !bridge_catalog
            .constraints_hold(&table.for_type, &[fixture.interface()], &[], &cancel)
            .unwrap()
    );
    assert!(!selected.valid(&bridge_catalog, &[], &[], &cancel).unwrap());
    selected.requirement.receiver = Ty::Builtin(BuiltinType::Bool);
    assert!(!selected.valid(&catalog, &[], &[], &cancel).unwrap());
    selected.requirement.receiver = table.for_type.clone();
    if let ConstructionSelection::Declared { implementation, .. } = &mut selected.selection {
        implementation.declaration = fixture.marker;
    }
    assert!(!selected.valid(&catalog, &[], &[], &cancel).unwrap());
}

#[test]
fn construction_capabilities_preserve_outputs_and_skip_static_only_traits() {
    let mut fixture = Fixture::new();
    let output = fixture
        .context
        .intern_child(fixture.marker, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    fixture.contract.associated_types.push(AssociatedTypeDef {
        declaration: output,
        parameter_bounds: vec![],
        generic_params: vec![],
        bounds: vec![],
    });
    let cancel = CancellationToken::default();
    let requirements = required_constructions(
        &[fixture.bound()],
        slice::from_ref(&fixture.parameter),
        &fixture.catalog(),
        &cancel,
    )
    .unwrap();
    assert_eq!(requirements.len(), 1);
    assert_eq!(
        requirements[0].interface.associated_types[&output],
        Ty::Projection {
            receiver: Box::new(fixture.parameter.as_type()),
            interface: Box::new(fixture.interface()),
            member: output,
            arguments: vec![],
        }
    );
    let mut bound = fixture.bound();
    bound.constraints[0]
        .associated_types
        .insert(output, Ty::Builtin(BuiltinType::I32));
    let requirements = required_constructions(
        slice::from_ref(&bound),
        slice::from_ref(&fixture.parameter),
        &fixture.catalog(),
        &cancel,
    )
    .unwrap();
    assert_eq!(
        requirements[0].interface.associated_types[&output],
        Ty::Builtin(BuiltinType::I32)
    );
    fixture
        .context
        .intern_child(fixture.marker, DefinitionKind::Method, "copy", 0)
        .unwrap();
    fixture.contract.methods.push(FnDecl {
        host_policy: None,
        name: "copy".into(),
        implementation: CallableImplementation::Required,
        method_policy: MethodPolicy::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "self".into(),
            ty: Ty::SelfType(fixture.marker),
            mutable: false,
        }],
        return_type: Ty::SelfType(fixture.marker),
    });
    assert!(
        required_constructions(
            slice::from_ref(&bound),
            slice::from_ref(&fixture.parameter),
            &fixture.catalog(),
            &cancel
        )
        .unwrap()
        .is_empty()
    );
    fixture.contract.methods[0].params.clear();
    assert!(
        required_constructions(
            &[bound],
            slice::from_ref(&fixture.parameter),
            &fixture.catalog(),
            &cancel
        )
        .unwrap()
        .is_empty()
    );
    cancel.cancel();
    assert_eq!(
        required_constructions(&[], &[], &fixture.catalog(), &cancel),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn construction_capabilities_include_parents_and_immediate_associated_bounds() {
    let fixture = Fixture::new();
    let root = fixture
        .context
        .intern_root(&ModuleIdentity::single_file("construction.kgr"))
        .unwrap();
    let owner = fixture
        .context
        .intern_child(root, DefinitionKind::Trait, "Container", 0)
        .unwrap();
    let member = fixture
        .context
        .intern_child(owner, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let child = TraitDef {
        name: "Container".into(),
        supertraits: vec![fixture.interface()],
        associated_types: vec![AssociatedTypeDef {
            declaration: member,
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: vec![fixture.interface()],
        }],
        ..fixture.contract.clone()
    };
    let application = NominalTy {
        declaration: owner,
        arguments: vec![],
        associated_types: Default::default(),
    };
    let mut bounds = vec![
        GenericBound {
            ty: fixture.parameter.as_type(),
            constraints: vec![application.clone()],
        },
        fixture.bound(),
    ];
    let definitions = fixture.context.snapshot();
    let cancel = CancellationToken::default();
    let catalog = ProofCatalog::new(
        vec![],
        [],
        [(fixture.marker, &fixture.contract), (owner, &child)],
        [],
        &definitions,
        &cancel,
    )
    .unwrap();
    let requirements = required_constructions(
        &bounds,
        slice::from_ref(&fixture.parameter),
        &catalog,
        &cancel,
    )
    .unwrap();
    assert_eq!(requirements.len(), 3);
    assert!(requirements.contains(&ConstructionRequirement {
        receiver: Ty::Projection {
            receiver: Box::new(fixture.parameter.as_type()),
            interface: Box::new(application),
            member,
            arguments: vec![]
        },
        interface: fixture.interface()
    }));
    bounds.reverse();
    assert_eq!(
        required_constructions(
            &bounds,
            slice::from_ref(&fixture.parameter),
            &catalog,
            &cancel
        )
        .unwrap(),
        requirements
    );
}

#[test]
fn closed_construction_evidence_substitutes_calls_and_bounds_expansion() {
    let fixture = Fixture::new();
    let root = fixture
        .context
        .intern_root(&ModuleIdentity::single_file("construction.kgr"))
        .unwrap();
    let entry = fixture
        .context
        .intern_child(root, DefinitionKind::Function, "main", 0)
        .unwrap();
    let implementation = fixture
        .context
        .intern_child(root, DefinitionKind::Impl, "MarkerForBox", 0)
        .unwrap();
    let callee = ConcreteFunctionIdentity {
        declaration: fixture.parameter.owner,
        arguments: vec![],
    };
    let scalar = Ty::Builtin(BuiltinType::I32);
    let bodies = [
        ConstructionBody {
            identity: ConcreteFunctionIdentity {
                declaration: entry,
                arguments: vec![],
            },
            parameters: vec![],
            steps: vec![ConstructionStep::Call {
                body: callee.clone(),
                arguments: vec![scalar.clone()],
            }],
        },
        ConstructionBody {
            identity: callee,
            parameters: vec![fixture.parameter.clone()],
            steps: vec![ConstructionStep::Evidence {
                operations: vec![],
                constructions: vec![ConstructionWitness {
                    requirement: ConstructionRequirement {
                        receiver: fixture.parameter.as_type(),
                        interface: fixture.interface(),
                    },
                    selection: ConstructionSelection::Declared {
                        implementation: ConcreteFunctionIdentity {
                            declaration: implementation,
                            arguments: vec![fixture.parameter.as_type()],
                        },
                        operations: vec![],
                        constructions: vec![],
                    },
                }],
            }],
        },
    ];
    assert_eq!(
        closed_constructions(&bodies, &[], 16, &Default::default()).unwrap(),
        [ConcreteFunctionIdentity {
            declaration: implementation,
            arguments: vec![scalar]
        }]
    );
    assert_eq!(
        closed_constructions(&bodies, &[], 1, &Default::default()),
        Err(TypeTransformError::LimitExceeded)
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        closed_constructions(&bodies, &[], 16, &cancel),
        Err(TypeTransformError::Cancelled)
    );
}
