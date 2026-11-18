use crate::{
    analysis::{
        AnalysisDatabase,
        external::{ExternalFunction, ExternalModule},
    },
    hir::item::function::NativeContract,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_source::{
    diagnostic::{DiagnosticKind, Severity},
    source_database::{SourceDatabase, SourceLayer},
};

use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FnDecl, Param,
        host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
        module::ModuleDecl,
        requirement::NativeCallableRequirement,
    },
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};
use std::sync::Arc;

struct Fixture {
    context: DefinitionContext,
    external: ExternalModule,
    sources: SourceDatabase,
    box_id: DefinitionId,
    native: DefinitionId,
    read: DefinitionId,
    member: DefinitionId,
    good: DefinitionId,
}

impl Fixture {
    fn new() -> Self {
        let identity = ModuleIdentity {
            package: PackageId("contracts".into()),
            path: vec![],
        };
        let context = DefinitionContext::new().unwrap();
        let root = context.intern_root(&identity).unwrap();
        let intern = |kind, name| context.intern_child(root, kind, name, 0).unwrap();
        let box_id = intern(DefinitionKind::Struct, "Box");
        let native = intern(DefinitionKind::AssociatedType, "Native");
        let read = intern(DefinitionKind::Trait, "Read");
        let good = intern(DefinitionKind::Struct, "Good");
        let member = context
            .intern_child(read, DefinitionKind::Method, "read", 0)
            .unwrap();
        let mut exports = SourceDatabase::new("/exports").unwrap();
        exports.bind_module("contracts.kgr", identity).unwrap();
        let source = exports
            .set(
                "contracts.kgr",
                include_str!("../../../tests/fixtures/typeck/native_contracts.kgr").into(),
                SourceLayer::Base,
            )
            .unwrap();
        Self {
            context,
            external: ExternalModule::new(exports.snapshot().file(source).unwrap().clone()),
            sources: SourceDatabase::new("/project").unwrap(),
            box_id,
            native,
            read,
            member,
            good,
        }
    }

    fn id(&self, kind: DefinitionKind, name: &str) -> DefinitionId {
        let root = self
            .context
            .intern_root(self.external.source.module_identity())
            .unwrap();
        self.context.intern_child(root, kind, name, 0).unwrap()
    }

    fn member(&self, owner: DefinitionId, kind: DefinitionKind, name: &str) -> DefinitionId {
        self.context.intern_child(owner, kind, name, 0).unwrap()
    }

    fn nominal(&self, declaration: DefinitionId, arguments: Vec<Ty>) -> NominalTy {
        NominalTy {
            declaration,
            arguments,
            associated_types: Default::default(),
        }
    }

    fn receiver(&self) -> Ty {
        Ty::Struct(self.nominal(
            self.box_id,
            vec![Ty::Struct(self.nominal(self.good, vec![]))],
        ))
    }

    fn contract(&self, receiver: Ty) -> NativeContract {
        NativeContract {
            host_policy: None,
            receiver_access: None,
            callable_requirements: vec![NativeCallableRequirement {
                receiver,
                interface: self.nominal(self.read, vec![]),
                member: self.member,
                arguments: vec![],
            }],
            concrete_result: None,
        }
    }

    fn diagnostics(&self, contract: NativeContract) -> Vec<DiagnosticKind> {
        self.diagnostics_for("check", contract)
    }

    fn diagnostics_for(&self, function: &str, contract: NativeContract) -> Vec<DiagnosticKind> {
        self.diagnostics_at(ExternalFunction::Function(function.into()), contract)
    }

    fn diagnostics_at(
        &self,
        target: ExternalFunction,
        contract: NativeContract,
    ) -> Vec<DiagnosticKind> {
        let mut external = self.external.clone();
        external.contracts.insert(
            target,
            Arc::new(
                DefinitionMetadata::checked(self.context.snapshot(), contract, &Default::default())
                    .unwrap(),
            ),
        );
        let mut db = AnalysisDatabase::new("/project").unwrap();
        db.set_external_modules(vec![external]);
        let snapshot = db
            .snapshot(self.sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(self.external.source.id()).unwrap();

        file.result()
            .diagnostics()
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.kind.clone())
            .collect()
    }
}

#[test]
fn nominal_contracts_require_exact_arity_category_and_recursive_applications() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .diagnostics(fixture.contract(fixture.receiver()))
            .is_empty()
    );
    let native_receiver = Ty::NativeObject(fixture.nominal(
        fixture.native,
        vec![Ty::Struct(fixture.nominal(fixture.good, vec![]))],
    ));
    let diagnostics = fixture.diagnostics(fixture.contract(native_receiver));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let missing = Ty::Struct(fixture.nominal(fixture.box_id, vec![]));
    let nested =
        Ty::Struct(fixture.nominal(fixture.box_id, vec![Ty::Array(Box::new(missing.clone()))]));
    for (name, receiver) in [
        ("missing argument", missing),
        (
            "extra argument",
            Ty::Struct(fixture.nominal(
                fixture.box_id,
                vec![
                    Ty::Builtin(BuiltinType::I32),
                    Ty::Builtin(BuiltinType::Bool),
                ],
            )),
        ),
        (
            "wrong semantic category",
            Ty::Struct(fixture.nominal(fixture.native, vec![Ty::Builtin(BuiltinType::I32)])),
        ),
        (
            "missing native argument",
            Ty::NativeObject(fixture.nominal(fixture.native, vec![])),
        ),
        ("malformed nested application", nested),
    ] {
        let diagnostics = fixture.diagnostics(fixture.contract(receiver));
        assert!(
            diagnostics
                .iter()
                .any(|kind| matches!(kind, DiagnosticKind::InvalidNativeSignature { .. })),
            "{name}: {diagnostics:?}"
        );
    }
}

#[test]
fn host_policies_preserve_unused_and_inherited_signature_binders() {
    let fixture = Fixture::new();
    let policy = HostCallPolicy {
        passing: vec![HostPassingStyle::Owned],
        effects: HostFunctionEffects::default(),
    };
    for (target, name, generic) in [
        (ExternalFunction::Function("plain".into()), "plain", false),
        (ExternalFunction::Function("scalar".into()), "scalar", true),
        (
            ExternalFunction::ImplMethod {
                owner: 3,
                name: "scalar".into(),
            },
            "scalar",
            true,
        ),
    ] {
        let declaration = fixture.id(DefinitionKind::Function, name);
        let mut structured = ModuleDecl::new(fixture.external.source.module_identity().clone());
        structured.functions.push(FnDecl {
            host_policy: Some(policy.clone()),
            method_policy: MethodPolicy::default(),
            name: name.into(),
            implementation: CallableImplementation::Native(declaration),
            generic_params: if generic {
                vec![GenericParam {
                    owner: declaration,
                    position: 0,
                }]
            } else {
                vec![]
            },
            bounds: vec![],
            params: vec![Param {
                name: "value".into(),
                ty: Ty::Builtin(BuiltinType::I32),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::I32),
        });
        let definitions = structured.collect_definitions(&fixture.context).unwrap();
        assert_eq!(
            structured.validate(&|_| None, &definitions).is_ok(),
            !generic
        );
        let diagnostics = fixture.diagnostics_at(
            target,
            NativeContract {
                host_policy: Some(policy.clone()),
                ..Default::default()
            },
        );
        if generic {
            assert!(
                matches!(diagnostics.as_slice(), [DiagnosticKind::InvalidNativeSignature { function, binding }] if function == "scalar" && binding == "invalid Host passing or effect policy"),
                "{diagnostics:?}"
            );
        } else {
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
        }
    }
}

#[test]
fn supplied_contract_types_retain_binder_scope_and_declared_bounds() {
    let fixture = Fixture::new();
    let check = fixture.id(DefinitionKind::Function, "check");
    let parameter = Ty::Parameter {
        owner: check,
        position: 0,
    };
    let generic_receiver = Ty::Struct(fixture.nominal(fixture.box_id, vec![parameter.clone()]));
    assert!(
        fixture
            .diagnostics(fixture.contract(generic_receiver))
            .is_empty()
    );
    let bounded_method = fixture.member(fixture.read, DefinitionKind::Method, "bounded");
    let good = Ty::Struct(fixture.nominal(fixture.good, vec![]));
    let mut method_contract = fixture.contract(fixture.receiver());
    method_contract.callable_requirements[0].member = bounded_method;
    for arguments in [vec![good.clone()], vec![parameter], vec![]] {
        method_contract.callable_requirements[0].arguments = arguments;
        let diagnostics = fixture.diagnostics(method_contract.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
    method_contract.callable_requirements[0].arguments = vec![Ty::Builtin(BuiltinType::Bool)];
    let diagnostics = fixture.diagnostics(method_contract);
    assert!(
        diagnostics
            .iter()
            .any(|kind| matches!(kind, DiagnosticKind::GenericBoundNotSatisfied { .. })),
        "{diagnostics:?}"
    );

    let diagnostics = fixture.diagnostics(fixture.contract(good.clone()));
    assert!(
        diagnostics
            .iter()
            .any(|kind| matches!(kind, DiagnosticKind::GenericBoundNotSatisfied { .. })),
        "{diagnostics:?}"
    );
    for (name, receiver) in [
        (
            "out of range binder",
            Ty::Parameter {
                owner: check,
                position: 1,
            },
        ),
        (
            "foreign method binder",
            Ty::Parameter {
                owner: bounded_method,
                position: 0,
            },
        ),
        (
            "missing semantic declaration",
            Ty::Struct(fixture.nominal(fixture.id(DefinitionKind::Struct, "Absent"), vec![])),
        ),
        (
            "unsatisfied nominal bound",
            Ty::Struct(fixture.nominal(
                fixture.id(DefinitionKind::Struct, "Bounded"),
                vec![Ty::Builtin(BuiltinType::Bool)],
            )),
        ),
    ] {
        let diagnostics = fixture.diagnostics(fixture.contract(receiver));
        assert!(
            diagnostics
                .iter()
                .any(|kind| matches!(kind, DiagnosticKind::InvalidNativeSignature { .. })),
            "{name}: {diagnostics:?}"
        );
    }
}

#[test]
fn projections_check_interface_and_member_applications_without_reinterpreting_output_keys() {
    let fixture = Fixture::new();
    let family = fixture.id(DefinitionKind::Trait, "Family");
    let output = fixture.member(family, DefinitionKind::AssociatedType, "Output");
    let good = Ty::Struct(fixture.nominal(fixture.good, vec![]));
    let projection = Ty::Projection {
        receiver: Box::new(Ty::Parameter {
            owner: fixture.id(DefinitionKind::Function, "check"),
            position: 0,
        }),
        interface: Box::new(fixture.nominal(family, vec![good.clone()])),
        member: output,
        arguments: vec![good.clone()],
    };
    let receiver = |ty| Ty::Struct(fixture.nominal(fixture.box_id, vec![ty]));
    let diagnostics = fixture.diagnostics(fixture.contract(receiver(projection.clone())));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let mut output_binding = fixture.contract(fixture.receiver());
    output_binding.callable_requirements[0]
        .interface
        .associated_types
        .insert(
            fixture.member(fixture.read, DefinitionKind::AssociatedType, "Item"),
            good,
        );
    let diagnostics = fixture.diagnostics(output_binding.clone());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    for (name, interface_count, member_count, wrong_member) in [
        ("missing trait argument", 0, 1, false),
        ("extra trait argument", 2, 1, false),
        ("missing family argument", 1, 0, false),
        ("extra family argument", 1, 2, false),
        ("wrong member kind", 1, 1, true),
    ] {
        let mut malformed = projection.clone();
        let Ty::Projection {
            interface,
            arguments,
            member,
            ..
        } = &mut malformed
        else {
            unreachable!()
        };
        interface
            .arguments
            .resize(interface_count, Ty::Builtin(BuiltinType::I32));
        arguments.resize(member_count, Ty::Builtin(BuiltinType::I32));
        if wrong_member {
            *member = fixture.member(family, DefinitionKind::Method, "read");
        }
        let diagnostics = fixture.diagnostics(fixture.contract(receiver(malformed)));
        assert!(
            diagnostics
                .iter()
                .any(|kind| matches!(kind, DiagnosticKind::InvalidNativeSignature { .. })),
            "{name}: {diagnostics:?}"
        );
    }
    let mut invalid_trait_bound = projection.clone();
    let Ty::Projection { interface, .. } = &mut invalid_trait_bound else {
        unreachable!()
    };
    interface.arguments[0] = Ty::Builtin(BuiltinType::Bool);
    let diagnostics = fixture.diagnostics(fixture.contract(receiver(invalid_trait_bound)));
    assert!(
        diagnostics
            .iter()
            .any(|kind| matches!(kind, DiagnosticKind::GenericBoundNotSatisfied { .. })),
        "{diagnostics:?}"
    );
    let mut invalid_family_bound = projection;
    let Ty::Projection { arguments, .. } = &mut invalid_family_bound else {
        unreachable!()
    };
    arguments[0] = Ty::Builtin(BuiltinType::Bool);
    let diagnostics = fixture.diagnostics(fixture.contract(receiver(invalid_family_bound)));
    assert!(
        diagnostics
            .iter()
            .any(|kind| matches!(kind, DiagnosticKind::InvalidAssociatedType { .. })),
        "{diagnostics:?}"
    );
    let malformed_output = Ty::Struct(fixture.nominal(fixture.box_id, vec![]));
    *output_binding.callable_requirements[0]
        .interface
        .associated_types
        .values_mut()
        .next()
        .unwrap() = malformed_output;
    let diagnostics = fixture.diagnostics(output_binding);
    assert!(
        diagnostics
            .iter()
            .any(|kind| matches!(kind, DiagnosticKind::InvalidNativeSignature { .. })),
        "{diagnostics:?}"
    );
}

#[test]
fn projected_receivers_use_declared_output_bounds_with_exact_substitutions() {
    let fixture = Fixture::new();
    let producer = fixture.id(DefinitionKind::Trait, "Producer");
    let good = Ty::Struct(fixture.nominal(fixture.good, vec![]));
    let value = fixture.member(producer, DefinitionKind::AssociatedType, "Value");
    let mut contract = fixture.contract(Ty::Projection {
        receiver: Box::new(Ty::Parameter {
            owner: fixture.id(DefinitionKind::Function, "projected"),
            position: 0,
        }),
        interface: Box::new(fixture.nominal(producer, vec![good.clone()])),
        member: value,
        arguments: vec![good.clone()],
    });
    contract.callable_requirements[0]
        .interface
        .associated_types
        .insert(
            fixture.member(fixture.read, DefinitionKind::AssociatedType, "Item"),
            Ty::Tuple(vec![good.clone(), good]),
        );
    let diagnostics = fixture.diagnostics_for("projected", contract.clone());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    for name in ["Unbounded", "Wrong"] {
        let mut invalid = contract.clone();
        let Ty::Projection { member, .. } = &mut invalid.callable_requirements[0].receiver else {
            unreachable!()
        };
        *member = fixture.member(producer, DefinitionKind::AssociatedType, name);
        let diagnostics = fixture.diagnostics_for("projected", invalid);
        assert!(
            diagnostics
                .iter()
                .any(|kind| matches!(kind, DiagnosticKind::GenericBoundNotSatisfied { .. })),
            "{name}: {diagnostics:?}"
        );
    }
    let Ty::Projection { receiver, .. } = &mut contract.callable_requirements[0].receiver else {
        unreachable!()
    };
    **receiver = Ty::Parameter {
        owner: fixture.id(DefinitionKind::Function, "check"),
        position: 0,
    };
    let diagnostics = fixture.diagnostics(contract);
    assert!(
        diagnostics
            .iter()
            .any(|kind| matches!(kind, DiagnosticKind::InvalidNativeSignature { .. })),
        "{diagnostics:?}"
    );
}

#[test]
fn native_trait_contracts_use_the_owning_self_bound() {
    let fixture = Fixture::new();
    let owner = fixture.id(DefinitionKind::Trait, "NativeRead");
    let contract = NativeContract {
        host_policy: None,
        receiver_access: None,
        callable_requirements: vec![NativeCallableRequirement {
            receiver: Ty::SelfType(owner),
            interface: fixture.nominal(owner, vec![Ty::Parameter { owner, position: 0 }]),
            member: fixture.member(owner, DefinitionKind::Method, "read"),
            arguments: vec![],
        }],
        concrete_result: None,
    };
    let target = ExternalFunction::TraitMethod {
        owner: "NativeRead".into(),
        name: "defaulted".into(),
    };
    let diagnostics = fixture.diagnostics_at(target.clone(), contract.clone());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let other = fixture.id(DefinitionKind::Trait, "Other");
    let mut unrelated = contract;
    unrelated.callable_requirements[0].interface = fixture.nominal(other, vec![]);
    unrelated.callable_requirements[0].member = fixture.member(other, DefinitionKind::Method, "op");
    let diagnostics = fixture.diagnostics_at(target.clone(), unrelated.clone());
    assert!(
        matches!(diagnostics.as_slice(), [DiagnosticKind::GenericBoundNotSatisfied { trait_name, .. }] if trait_name == "Other"),
        "{diagnostics:?}"
    );

    let result = NativeContract {
        host_policy: None,
        receiver_access: None,
        callable_requirements: vec![],
        concrete_result: Some(Ty::SelfType(owner)),
    };
    let diagnostics = fixture.diagnostics_at(
        ExternalFunction::TraitMethod {
            owner: "NativeRead".into(),
            name: "produce".into(),
        },
        result,
    );
    assert!(
        matches!(diagnostics.as_slice(), [DiagnosticKind::GenericBoundNotSatisfied { trait_name, .. }] if trait_name == "Other"),
        "{diagnostics:?}"
    );

    let mut method_bound = fixture.contract(fixture.receiver());
    method_bound.callable_requirements[0].member =
        fixture.member(fixture.read, DefinitionKind::Method, "bounded");
    method_bound.callable_requirements[0].arguments = vec![Ty::SelfType(owner)];
    let diagnostics = fixture.diagnostics_at(target, method_bound);
    assert!(
        matches!(diagnostics.as_slice(), [DiagnosticKind::GenericBoundNotSatisfied { trait_name, .. }] if trait_name == "Marker"),
        "{diagnostics:?}"
    );

    let derived = fixture.id(DefinitionKind::Trait, "DerivedRead");
    unrelated.callable_requirements[0].receiver = Ty::SelfType(derived);
    let diagnostics = fixture.diagnostics_at(
        ExternalFunction::TraitMethod {
            owner: "DerivedRead".into(),
            name: "defaulted".into(),
        },
        unrelated,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let diagnostics = fixture.diagnostics_at(
        ExternalFunction::TraitMethod {
            owner: "DerivedRead".into(),
            name: "produce".into(),
        },
        NativeContract {
            host_policy: None,
            receiver_access: None,
            callable_requirements: vec![],
            concrete_result: Some(Ty::SelfType(derived)),
        },
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn concrete_results_use_the_same_recursive_nominal_validation() {
    let fixture = Fixture::new();
    let diagnostics = fixture.diagnostics_for(
        "produce",
        NativeContract {
            host_policy: None,
            receiver_access: None,
            callable_requirements: vec![],
            concrete_result: Some(fixture.receiver()),
        },
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let malformed = Ty::Struct(fixture.nominal(fixture.box_id, vec![]));
    let diagnostics = fixture.diagnostics_for(
        "produce",
        NativeContract {
            host_policy: None,
            receiver_access: None,
            callable_requirements: vec![],
            concrete_result: Some(malformed),
        },
    );
    assert!(
        diagnostics
            .iter()
            .any(|kind| matches!(kind, DiagnosticKind::InvalidNativeSignature { .. })),
        "{diagnostics:?}"
    );
}
