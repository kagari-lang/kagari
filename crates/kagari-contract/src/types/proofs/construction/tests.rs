use crate::{
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        generic::GenericBody,
        witness::{OperationWitness, SharedMethodWitness},
    },
    types::{
        ConcreteFunctionIdentity, InterfaceTable,
        proofs::{ProofCatalog, implementation::Implementation},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId},
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        AssociatedTypeDef, FnDecl, Param, TraitDef, requirement::NativeCallableRequirement,
    },
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty, substitution::TypeTransformError},
};

struct Fixture {
    context: DefinitionContext,
    read: DefinitionId,
    view: DefinitionId,
    output: DefinitionId,
    read_contract: TraitDef,
    view_contract: TraitDef,
    selected: InterfaceTable,
    leaf: InterfaceTable,
    parameter: GenericParam,
}

fn nominal(declaration: DefinitionId) -> NominalTy {
    NominalTy {
        declaration,
        arguments: vec![],
        associated_types: Default::default(),
    }
}

fn scalar() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn contract(name: &str) -> TraitDef {
    TraitDef {
        name: name.into(),
        conversion_adapter: None,
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![],
    }
}

impl Fixture {
    fn new() -> Self {
        let context = DefinitionContext::new().unwrap();
        let root = context
            .intern_root(&ModuleIdentity::single_file("derivation.kgr"))
            .unwrap();
        let id = |kind, name| context.intern_child(root, kind, name, 0).unwrap();
        let read = id(DefinitionKind::Trait, "Read");
        let view = id(DefinitionKind::Trait, "View");
        let output = context
            .intern_child(view, DefinitionKind::AssociatedType, "Item", 0)
            .unwrap();
        let selected_id = id(DefinitionKind::Impl, "ViewForTuple");
        let leaf_id = id(DefinitionKind::Impl, "ReadForI32");
        let parameter = GenericParam {
            owner: id(DefinitionKind::Function, "erase"),
            position: 0,
        };
        let applied = GenericParam {
            owner: selected_id,
            position: 0,
        };
        context
            .intern_child(read, DefinitionKind::Method, "read", 0)
            .unwrap();
        context
            .intern_child(leaf_id, DefinitionKind::Method, "read", 0)
            .unwrap();
        let method = FnDecl {
            name: "read".into(),
            host_policy: None,
            implementation: CallableImplementation::Required,
            method_policy: MethodPolicy::default(),
            generic_params: vec![],
            bounds: vec![],
            params: vec![Param {
                name: "self".into(),
                ty: Ty::SelfType(read),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::Bool),
        };
        let mut read_contract = contract("Read");
        read_contract.methods.push(method.clone());
        let mut view_contract = contract("View");
        view_contract.associated_types.push(AssociatedTypeDef {
            declaration: output,
            parameter_bounds: vec![],
            generic_params: vec![],
            bounds: vec![],
        });
        let mut interface = nominal(view);
        interface.associated_types.insert(output, applied.as_type());
        let selected = InterfaceTable {
            declaration: selected_id,
            name: "ViewForTuple".into(),
            generic_params: vec![applied.clone()],
            bounds: vec![GenericBound {
                ty: applied.as_type(),
                constraints: vec![nominal(read)],
            }],
            trait_type: Ty::Trait(interface),
            for_type: Ty::Tuple(vec![applied.as_type()]),
            methods: vec![],
            associated_consts: vec![],
            associated_type_families: vec![],
            bridge: None,
        };
        let leaf = InterfaceTable {
            declaration: leaf_id,
            name: "ReadForI32".into(),
            generic_params: vec![],
            bounds: vec![],
            trait_type: Ty::Trait(nominal(read)),
            for_type: scalar(),
            methods: vec![FnDecl {
                implementation: CallableImplementation::Script,
                params: vec![Param {
                    name: "self".into(),
                    ty: scalar(),
                    mutable: false,
                }],
                ..method
            }],
            associated_consts: vec![],
            associated_type_families: vec![],
            bridge: None,
        };
        Self {
            context,
            read,
            view,
            output,
            read_contract,
            view_contract,
            selected,
            leaf,
            parameter,
        }
    }

    fn catalog<'a>(&'a self, tables: Vec<&'a InterfaceTable>) -> ProofCatalog<'a> {
        ProofCatalog::new(
            tables.into_iter().map(Implementation::Interface).collect(),
            [],
            [
                (self.read, &self.read_contract),
                (self.view, &self.view_contract),
            ],
            [],
            &self.context.snapshot(),
            &Default::default(),
        )
        .unwrap()
    }

    fn request(&self, argument: Ty) -> (ConstructionRequirement, ConcreteFunctionIdentity) {
        let mut interface = nominal(self.view);
        interface
            .associated_types
            .insert(self.output, argument.clone());
        (
            ConstructionRequirement {
                receiver: Ty::Tuple(vec![argument.clone()]),
                interface,
            },
            ConcreteFunctionIdentity {
                declaration: self.selected.declaration,
                arguments: vec![argument],
            },
        )
    }

    fn body(&self) -> GenericBody {
        GenericBody {
            parameters: vec![self.parameter.clone()],
            bounds: vec![GenericBound {
                ty: self.parameter.as_type(),
                constraints: vec![nominal(self.read)],
            }],
        }
    }
}

impl Fixture {
    fn operation(&self, receiver: Ty) -> NativeCallableRequirement {
        NativeCallableRequirement {
            receiver,
            interface: nominal(self.read),
            member: self
                .context
                .snapshot()
                .lookup_child(self.read, DefinitionKind::Method, "read", 0)
                .unwrap(),
            arguments: vec![],
        }
    }

    fn witness(
        &self,
        argument: Ty,
        operations: Vec<OperationWitness>,
        constructions: Vec<ConstructionWitness>,
    ) -> ConstructionWitness {
        let (requirement, implementation) = self.request(argument);
        ConstructionWitness {
            requirement,
            selection: ConstructionSelection::Declared {
                implementation,
                operations,
                constructions,
            },
        }
    }

    fn forwarded(&self) -> (OperationWitness, ConstructionWitness) {
        let receiver = self.parameter.as_type();
        (
            OperationWitness::Forward(Box::new(self.operation(receiver.clone()))),
            ConstructionWitness {
                requirement: ConstructionRequirement {
                    receiver,
                    interface: nominal(self.read),
                },
                selection: ConstructionSelection::Forward,
            },
        )
    }

    fn concrete(&self) -> ConstructionWitness {
        let catalog = self.catalog(vec![&self.selected, &self.leaf]);
        let operation = catalog
            .select_callable(&self.operation(scalar()), &Default::default())
            .unwrap()
            .unwrap();
        self.witness(
            scalar(),
            vec![OperationWitness::Selected(Box::new(operation))],
            vec![ConstructionWitness {
                requirement: ConstructionRequirement {
                    receiver: scalar(),
                    interface: nominal(self.read),
                },
                selection: ConstructionSelection::Declared {
                    implementation: ConcreteFunctionIdentity {
                        declaration: self.leaf.declaration,
                        arguments: vec![],
                    },
                    operations: vec![],
                    constructions: vec![],
                },
            }],
        )
    }
}

#[test]
fn selected_construction_requires_both_member_and_construction_evidence() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(vec![&fixture.selected, &fixture.leaf]);
    let cancel = CancellationToken::default();
    let witness = fixture.concrete();
    assert!(witness.valid(&catalog, &[], &[], &cancel).unwrap());
    for missing_operation in [false, true] {
        let mut incomplete = witness.clone();
        let ConstructionSelection::Declared {
            operations,
            constructions,
            ..
        } = &mut incomplete.selection
        else {
            unreachable!()
        };
        if missing_operation {
            operations.clear();
        } else {
            constructions.clear();
        }
        assert!(!incomplete.valid(&catalog, &[], &[], &cancel).unwrap());
    }
    assert!(
        !witness
            .valid(&fixture.catalog(vec![&fixture.selected]), &[], &[], &cancel)
            .unwrap()
    );
}

#[test]
fn shared_construction_forwards_only_its_declared_capabilities() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(vec![&fixture.selected]);
    let (operation, construction) = fixture.forwarded();
    let witness = fixture.witness(
        fixture.parameter.as_type(),
        vec![operation],
        vec![construction],
    );
    let body = fixture.body();
    let cancel = CancellationToken::default();
    assert!(
        witness
            .valid(&catalog, &body.parameters, &body.bounds, &cancel)
            .unwrap()
    );
    assert!(
        !witness
            .valid(&catalog, &body.parameters, &[], &cancel)
            .unwrap()
    );
    assert!(!witness.valid(&catalog, &[], &[], &cancel).unwrap());
}

#[test]
fn wrong_selected_application_receiver_and_associated_outputs_fail_closed() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(vec![&fixture.selected, &fixture.leaf]);
    let cancel = CancellationToken::default();
    for change in 0..4 {
        let mut witness = fixture.concrete();
        let ConstructionSelection::Declared { implementation, .. } = &mut witness.selection else {
            unreachable!()
        };
        match change {
            0 => implementation.declaration = fixture.leaf.declaration,
            1 => implementation.arguments.clear(),
            2 => {
                witness
                    .requirement
                    .interface
                    .associated_types
                    .insert(fixture.output, Ty::Builtin(BuiltinType::Bool));
            }
            _ => witness.requirement.receiver = scalar(),
        }
        assert!(!witness.valid(&catalog, &[], &[], &cancel).unwrap());
    }
}

#[test]
fn nested_shared_method_retains_forwarded_dependencies() {
    let fixture = Fixture::new();
    let root = fixture
        .context
        .intern_root(&ModuleIdentity::single_file("derivation.kgr"))
        .unwrap();
    let declaration = fixture
        .context
        .intern_child(root, DefinitionKind::Impl, "ReadForTuple", 0)
        .unwrap();
    fixture
        .context
        .intern_child(declaration, DefinitionKind::Method, "read", 0)
        .unwrap();
    let parameter = GenericParam {
        owner: declaration,
        position: 0,
    };
    let mut nested = fixture.leaf.clone();
    nested.declaration = declaration;
    nested.name = "ReadForTuple".into();
    nested.generic_params = vec![parameter.clone()];
    nested.bounds = vec![GenericBound {
        ty: parameter.as_type(),
        constraints: vec![nominal(fixture.read)],
    }];
    nested.for_type = Ty::Tuple(vec![parameter.as_type()]);
    nested.methods[0].params[0].ty = nested.for_type.clone();
    let catalog = fixture.catalog(vec![&fixture.selected, &nested]);
    let argument = Ty::Tuple(vec![fixture.parameter.as_type()]);
    let implementation = ConcreteFunctionIdentity {
        declaration,
        arguments: vec![fixture.parameter.as_type()],
    };
    let (operation, construction) = fixture.forwarded();
    let witness = fixture.witness(
        argument.clone(),
        vec![OperationWitness::SharedMethod(Box::new(
            SharedMethodWitness {
                requirement: fixture.operation(argument.clone()),
                implementation: implementation.clone(),
                operations: vec![operation.clone()],
                constructions: vec![construction.clone()],
            },
        ))],
        vec![ConstructionWitness {
            requirement: ConstructionRequirement {
                receiver: argument,
                interface: nominal(fixture.read),
            },
            selection: ConstructionSelection::Declared {
                implementation,
                operations: vec![operation],
                constructions: vec![construction],
            },
        }],
    );
    let body = fixture.body();
    let cancel = CancellationToken::default();
    assert!(
        witness
            .valid(&catalog, &body.parameters, &body.bounds, &cancel)
            .unwrap()
    );
    assert!(
        !witness
            .valid(&catalog, &body.parameters, &[], &cancel)
            .unwrap()
    );
}

#[test]
fn normalized_outputs_and_inherited_views_preserve_the_selected_application() {
    let mut fixture = Fixture::new();
    let root = fixture
        .context
        .intern_root(&ModuleIdentity::single_file("derivation.kgr"))
        .unwrap();
    let parent = fixture
        .context
        .intern_child(root, DefinitionKind::Trait, "Parent", 0)
        .unwrap();
    let parent_contract = contract("Parent");
    fixture.view_contract.supertraits.push(nominal(parent));
    let catalog = ProofCatalog::new(
        vec![
            Implementation::Interface(&fixture.selected),
            Implementation::Interface(&fixture.leaf),
        ],
        [],
        [
            (fixture.read, &fixture.read_contract),
            (fixture.view, &fixture.view_contract),
            (parent, &parent_contract),
        ],
        [],
        &fixture.context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    let mut witness = fixture.concrete();
    let projection = Ty::Projection {
        receiver: Box::new(witness.requirement.receiver.clone()),
        interface: Box::new(nominal(fixture.view)),
        member: fixture.output,
        arguments: vec![],
    };
    witness.requirement.receiver = Ty::Tuple(vec![projection.clone()]);
    witness
        .requirement
        .interface
        .associated_types
        .insert(fixture.output, projection);
    witness.requirement = witness
        .requirement
        .normalize(&catalog, &Default::default())
        .unwrap();
    assert_eq!(witness.requirement, fixture.request(scalar()).0);
    assert!(
        witness
            .valid(&catalog, &[], &[], &Default::default())
            .unwrap()
    );
    witness.requirement = fixture.request(scalar()).0;
    witness.requirement.interface = nominal(parent);
    assert!(
        witness
            .valid(&catalog, &[], &[], &Default::default())
            .unwrap()
    );
}

#[test]
fn supplied_construction_tree_is_bounded_and_cancellation_is_preserved() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(vec![&fixture.selected, &fixture.leaf]);
    let mut witness = fixture.concrete();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        witness.valid(&catalog, &[], &[], &cancel),
        Err(TypeTransformError::Cancelled)
    );
    let ConstructionSelection::Declared { operations, .. } = &mut witness.selection else {
        unreachable!()
    };
    *operations = vec![fixture.forwarded().0; 4097];
    assert!(!witness.structurally_valid(&[], catalog.definitions(), &Default::default()));
}
