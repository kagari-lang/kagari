use super::*;
use crate::{
    callable::witness::OperationWitness,
    effects::EffectSet,
    native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
    updates::admission::validate_semantics,
};
use kagari_types::{
    declaration::{Param, TraitDef, requirement::NativeCallableRequirement},
    language::{self, Protocol},
};
use std::slice;

#[test]
fn native_application_retains_each_exact_module_owned_adapter_environment() {
    let f = Fixture::new();
    let hash = f
        .context
        .intern(&language::identity(Protocol::Hash))
        .unwrap();
    let member = f
        .context
        .intern_child(hash, DefinitionKind::Method, "hash", 0)
        .unwrap();
    let receiver = Ty::Builtin(BuiltinType::U64);
    let result = Ty::Builtin(BuiltinType::I64);
    let required = NativeCallableRequirement {
        receiver: receiver.clone(),
        interface: NominalTy {
            declaration: hash,
            arguments: vec![],
            associated_types: Default::default(),
        },
        member,
        arguments: vec![],
    };
    let contract = TraitDef {
        name: "Hash".into(),
        conversion_adapter: None,
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![FnDecl {
            name: "hash".into(),
            host_policy: None,
            method_policy: MethodPolicy::default(),
            implementation: CallableImplementation::Required,
            generic_params: vec![],
            bounds: vec![],
            params: vec![Param {
                name: "self".into(),
                ty: Ty::SelfType(hash),
                mutable: false,
            }],
            return_type: result.clone(),
        }],
    };
    let binding = f.identity("native_binding").declaration;
    let instance = f.identity("native");
    let declaration = NativeDeclaration {
        concrete_result: None,
        declaration: instance.declaration,
        function: FnDecl {
            name: "native".into(),
            host_policy: None,
            method_policy: MethodPolicy::default(),
            implementation: CallableImplementation::Native(binding),
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: Ty::Builtin(BuiltinType::I32),
        },
        callable_requirements: vec![required.clone()],
    };
    let other = f
        .context
        .intern_root(&ModuleIdentity::single_file("dependency.kgr"))
        .unwrap();
    let mut candidate = f.evidence();
    let mut imports = Vec::new();
    let mut adapters = Vec::new();
    for owner in [f.root, other] {
        let adapter = ConcreteFunctionIdentity {
            declaration: f
                .context
                .intern_child(owner, DefinitionKind::Function, "$derived_Hash", 0)
                .unwrap(),
            arguments: vec![receiver.clone()],
        };
        let signature = Signature {
            params: vec![receiver.clone()],
            result: result.clone(),
        };
        let mut body = f.body("adapter_fixture", vec![]);
        body.requirement.origin = adapter.clone();
        body.classification = BodyClassification::Adapter(adapter.clone());
        body.signature = signature.clone();
        candidate.bodies.push(body);
        imports.push(NativeImport {
            receiver_access: None,
            result_adapter: None,
            generic: None,
            instance: instance.clone(),
            binding,
            signature: Signature {
                params: vec![],
                result: declaration.function.return_type.clone(),
            },
            requirements: vec![],
            callables: vec![OperationWitness::Selected(Box::new(
                NativeCallableApplication {
                    origin: NativeCallableOrigin::ProtocolAdapter,
                    requirement: required.clone(),
                    instance: adapter.clone(),
                    implementation: CallableImplementation::Script,
                    signature,
                    effects: EffectSet::native_call(),
                },
            ))],
        });
        adapters.push(adapter);
    }
    candidate.selections = imports
        .iter()
        .cloned()
        .map(|import| CallRequirement::Native(Box::new(import)))
        .collect();
    let definitions = f.context.snapshot();
    let catalog = ProofCatalog::new(
        vec![],
        [],
        [(hash, &contract)],
        [&declaration],
        &definitions,
        &f.cancel,
    )
    .unwrap();
    for import in &imports {
        assert!(
            import
                .matches_declaration(&declaration, &catalog, &f.cancel)
                .unwrap()
        );
    }
    validate_semantics(&candidate, &catalog, &mut f.budget()).unwrap();
    let natives: Vec<_> = imports
        .iter()
        .map(|import| NativeClosure {
            import,
            callbacks: None,
        })
        .collect();
    let mut envelope = publication_demands(&candidate, &mut f.budget()).unwrap();
    envelope
        .requirements
        .insert(0, CallRequirement::Direct(instance));
    let mut available = coverage(&candidate);
    available.natives = &natives;
    check_coverage(&envelope, &available, &catalog, &mut f.budget()).unwrap();
    assert!(
        expand_requests(&envelope, &available, &catalog, &mut f.budget())
            .unwrap()
            .is_empty()
    );
    // Either missing adapter must fail, regardless of import ordering or whether
    // the same native application was already visited through another environment.
    for missing in &adapters {
        let mut incomplete = candidate.clone();
        incomplete
            .bodies
            .retain(|body| body.requirement.origin != *missing);
        let mut available = coverage(&incomplete);
        available.natives = &natives;
        assert_eq!(
            check_coverage(&envelope, &available, &catalog, &mut f.budget()),
            Err(UpdateError::MissingTarget(missing.clone()))
        );
        assert_eq!(
            expand_requests(&envelope, &available, &catalog, &mut f.budget())
                .unwrap()
                .functions,
            slice::from_ref(missing)
        );
    }
    // A different valid placement is not a substitute for an exact retained import.
    available.natives = &natives[..1];
    assert_eq!(
        check_coverage(&envelope, &available, &catalog, &mut f.budget()),
        Err(UpdateError::InvalidEvidence)
    );
    let mut forged = imports[1].clone();
    let OperationWitness::Selected(call) = &mut forged.callables[0] else {
        panic!()
    };
    call.instance.arguments[0] = Ty::Builtin(BuiltinType::I32);
    let forged_natives = [
        NativeClosure {
            import: &imports[0],
            callbacks: None,
        },
        NativeClosure {
            import: &forged,
            callbacks: None,
        },
    ];
    available.natives = &forged_natives;
    assert_eq!(
        check_coverage(&envelope, &available, &catalog, &mut f.budget()),
        Err(UpdateError::InvalidEvidence)
    );
    let mut forged_evidence = candidate.clone();
    forged_evidence.selections[1] = CallRequirement::Native(Box::new(forged));
    assert_eq!(
        validate_semantics(&forged_evidence, &catalog, &mut f.budget()),
        Err(UpdateError::InvalidEvidence)
    );
}
