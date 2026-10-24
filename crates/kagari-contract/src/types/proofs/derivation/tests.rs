use super::*;
use crate::types::{InterfaceBridge, InterfaceTable, proofs::implementation::Implementation};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{AssociatedTypeDef, FnDecl, Param, TraitDef},
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
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

#[test]
fn selected_generic_table_derives_complete_operations_and_constructions() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(vec![&fixture.selected, &fixture.leaf]);
    let (required, application) = fixture.request(scalar());
    let cancel = CancellationToken::default();
    let witness = catalog
        .derive_selected_construction(&required, &application, None, 3, &cancel)
        .unwrap();
    assert_eq!(witness.requirement, required);
    assert!(witness.valid(&catalog, &[], &[], &cancel).unwrap());
    let ConstructionSelection::Declared {
        implementation,
        operations,
        constructions,
    } = &witness.selection
    else {
        panic!("selected declaration must be retained");
    };
    assert_eq!(implementation, &application);
    assert_eq!(operations.len(), 1);
    assert!(
        matches!(&operations[0], OperationWitness::Selected(call) if call.instance.declaration == fixture.context.snapshot().lookup_child(fixture.leaf.declaration, DefinitionKind::Method, "read", 0).unwrap())
    );
    assert_eq!(constructions.len(), 1);
    assert!(
        matches!(&constructions[0].selection, ConstructionSelection::Declared { implementation, .. } if implementation.declaration == fixture.leaf.declaration)
    );
    let mut incomplete = witness.clone();
    if let ConstructionSelection::Declared { constructions, .. } = &mut incomplete.selection {
        constructions.clear();
    }
    assert!(!incomplete.valid(&catalog, &[], &[], &cancel).unwrap());
}

#[test]
fn shared_body_forwards_only_its_declared_operation_and_construction_capabilities() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(vec![&fixture.selected]);
    let (required, application) = fixture.request(fixture.parameter.as_type());
    let mut body = fixture.body();
    let cancel = CancellationToken::default();
    let witness = catalog
        .derive_selected_construction(&required, &application, Some(&body), 3, &cancel)
        .unwrap();
    assert_eq!(
        witness.requirement.interface.associated_types[&fixture.output],
        fixture.parameter.as_type()
    );
    let ConstructionSelection::Declared {
        operations,
        constructions,
        ..
    } = &witness.selection
    else {
        panic!()
    };
    assert!(matches!(
        operations.as_slice(),
        [OperationWitness::Forward(_)]
    ));
    assert!(matches!(
        constructions.as_slice(),
        [ConstructionWitness {
            selection: ConstructionSelection::Forward,
            ..
        }]
    ));
    assert!(
        witness
            .valid(&catalog, &body.parameters, &body.bounds, &cancel)
            .unwrap()
    );
    body.bounds.clear();
    assert!(
        catalog
            .derive_selected_construction(&required, &application, Some(&body), 3, &cancel)
            .is_err()
    );
}

#[test]
fn missing_wrong_and_ambiguous_proofs_cannot_replace_the_selected_application() {
    let fixture = Fixture::new();
    let (required, application) = fixture.request(scalar());
    let cancel = CancellationToken::default();
    assert!(
        fixture
            .catalog(vec![&fixture.selected])
            .derive_selected_construction(&required, &application, None, 32, &cancel)
            .is_err()
    );
    let catalog = fixture.catalog(vec![&fixture.selected, &fixture.leaf]);
    let mut wrong = application.clone();
    wrong.declaration = fixture.leaf.declaration;
    assert!(
        catalog
            .derive_selected_construction(&required, &wrong, None, 32, &cancel)
            .is_err()
    );
    wrong = application.clone();
    wrong.arguments.clear();
    assert!(
        catalog
            .derive_selected_construction(&required, &wrong, None, 32, &cancel)
            .is_err()
    );
    let mut wrong_requirement = required.clone();
    wrong_requirement
        .interface
        .associated_types
        .insert(fixture.output, Ty::Builtin(BuiltinType::Bool));
    assert!(
        catalog
            .derive_selected_construction(&wrong_requirement, &application, None, 32, &cancel)
            .is_err()
    );
    wrong_requirement = required.clone();
    wrong_requirement.receiver = scalar();
    assert!(
        catalog
            .derive_selected_construction(&wrong_requirement, &application, None, 32, &cancel)
            .is_err()
    );
    let mut ambiguous = fixture.leaf.clone();
    let root = fixture
        .context
        .intern_root(&ModuleIdentity::single_file("derivation.kgr"))
        .unwrap();
    ambiguous.declaration = fixture
        .context
        .intern_child(root, DefinitionKind::Impl, "OtherRead", 0)
        .unwrap();
    fixture
        .context
        .intern_child(ambiguous.declaration, DefinitionKind::Method, "read", 0)
        .unwrap();
    assert!(
        fixture
            .catalog(vec![&fixture.selected, &fixture.leaf, &ambiguous])
            .derive_selected_construction(&required, &application, None, 32, &cancel)
            .is_err()
    );
    let mut forged = fixture.leaf.clone();
    forged.bridge = Some(InterfaceBridge::Intrinsic);
    assert!(
        fixture
            .catalog(vec![&fixture.selected, &forged])
            .derive_selected_construction(&required, &application, None, 32, &cancel)
            .is_err()
    );
}

#[test]
fn nested_shared_method_selection_retains_its_forwarded_dependencies() {
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
    let receiver = Ty::Tuple(vec![parameter.as_type()]);
    let mut nested = fixture.leaf.clone();
    nested.declaration = declaration;
    nested.name = "ReadForTuple".into();
    nested.generic_params = vec![parameter.clone()];
    nested.bounds = vec![GenericBound {
        ty: parameter.as_type(),
        constraints: vec![nominal(fixture.read)],
    }];
    nested.for_type = receiver.clone();
    nested.methods[0].params[0].ty = receiver;
    let catalog = fixture.catalog(vec![&fixture.selected, &nested]);
    let body = fixture.body();
    let (required, application) = fixture.request(Ty::Tuple(vec![fixture.parameter.as_type()]));
    let cancel = CancellationToken::default();
    let witness = catalog
        .derive_selected_construction(&required, &application, Some(&body), 7, &cancel)
        .unwrap();
    let ConstructionSelection::Declared {
        operations,
        constructions,
        ..
    } = &witness.selection
    else {
        panic!()
    };
    let [OperationWitness::SharedMethod(method)] = operations.as_slice() else {
        panic!("nested shared selection required")
    };
    assert_eq!(method.implementation.declaration, declaration);
    assert_eq!(
        method.implementation.arguments,
        [fixture.parameter.as_type()]
    );
    assert!(matches!(
        method.operations.as_slice(),
        [OperationWitness::Forward(_)]
    ));
    assert!(matches!(
        method.constructions.as_slice(),
        [ConstructionWitness {
            selection: ConstructionSelection::Forward,
            ..
        }]
    ));
    assert!(
        matches!(&constructions[0].selection, ConstructionSelection::Declared { implementation, .. } if implementation == &method.implementation)
    );
    assert!(
        witness
            .valid(&catalog, &body.parameters, &body.bounds, &cancel)
            .unwrap()
    );
    assert_eq!(
        catalog.derive_selected_construction(&required, &application, Some(&body), 6, &cancel),
        Err(ConstructionDerivationError::Transform(
            TypeTransformError::LimitExceeded
        ))
    );
}

#[test]
fn conformance_without_an_executable_operation_is_not_complete_evidence() {
    let mut fixture = Fixture::new();
    fixture.leaf.methods.clear();
    let catalog = fixture.catalog(vec![&fixture.selected, &fixture.leaf]);
    let (required, application) = fixture.request(scalar());
    let cancel = CancellationToken::default();
    assert!(
        catalog
            .holds(&nominal(fixture.read), &scalar(), &[], &cancel)
            .unwrap()
    );
    assert!(
        matches!(catalog.derive_selected_construction(&required, &application, None, 3, &cancel), Err(ConstructionDerivationError::MissingOperation(required)) if required.receiver == scalar() && required.interface.declaration == fixture.read)
    );
}

#[test]
fn normalized_outputs_and_inherited_views_keep_the_exact_selected_application() {
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
    let cancel = CancellationToken::default();
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
        &cancel,
    )
    .unwrap();
    let (expected, application) = fixture.request(scalar());
    let projection = Ty::Projection {
        receiver: Box::new(expected.receiver.clone()),
        interface: Box::new(nominal(fixture.view)),
        member: fixture.output,
        arguments: vec![],
    };
    let mut required = expected.clone();
    required.receiver = Ty::Tuple(vec![projection.clone()]);
    required
        .interface
        .associated_types
        .insert(fixture.output, projection);
    let witness = catalog
        .derive_selected_construction(&required, &application, None, 3, &cancel)
        .unwrap();
    assert_eq!(witness.requirement, expected);
    required = expected;
    required.interface = nominal(parent);
    let witness = catalog
        .derive_selected_construction(&required, &application, None, 3, &cancel)
        .unwrap();
    assert_eq!(witness.requirement, required);
    assert!(
        matches!(&witness.selection, ConstructionSelection::Declared { implementation, .. } if implementation == &application)
    );
    assert!(witness.valid(&catalog, &[], &[], &cancel).unwrap());
}

#[test]
fn derivation_charges_the_combined_tree_and_preserves_cancellation() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog(vec![&fixture.selected, &fixture.leaf]);
    let (required, application) = fixture.request(scalar());
    let cancel = CancellationToken::default();
    for limit in [0, 1, 2] {
        assert_eq!(
            catalog.derive_selected_construction(&required, &application, None, limit, &cancel),
            Err(ConstructionDerivationError::Transform(
                TypeTransformError::LimitExceeded
            ))
        );
    }
    cancel.cancel();
    assert_eq!(
        catalog.derive_selected_construction(&required, &application, None, 3, &cancel),
        Err(ConstructionDerivationError::Transform(
            TypeTransformError::Cancelled
        ))
    );
}
