//! Genuine Native defaults retain ordinary executable contracts without source analysis.
use crate::types::{InterfaceTable, PublicItem, proofs::ProofCatalog, verify};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, table::DefinitionId,
    },
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FnDecl, NativeDeclaration, Param, TraitDef, requirement::NativeCallableRequirement,
    },
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty, substitution::TypeTransformError},
};
use std::collections::BTreeMap;

fn module() -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["defaults".into()],
    }
}

fn identity(context: &DefinitionContext, kind: DefinitionKind, name: &str) -> DefinitionId {
    let root = context.intern_root(&module()).unwrap();
    context.intern_child(root, kind, name, 0).unwrap()
}

fn scalar() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn interface(owner: &DefinitionId, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration: *owner,
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn function(name: &str, implementation: CallableImplementation, parameter: Ty) -> FnDecl {
    FnDecl {
        host_policy: None,
        name: name.into(),
        implementation,
        method_policy: MethodPolicy::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "value".into(),
            ty: parameter,
            mutable: false,
        }],
        return_type: scalar(),
    }
}

struct Fixture {
    context: DefinitionContext,
    owner: DefinitionId,
    contract: TraitDef,
    template: NativeDeclaration,
    table: InterfaceTable,
}

impl Fixture {
    fn new() -> Self {
        let context = DefinitionContext::new().unwrap();
        let owner = identity(&context, DefinitionKind::Trait, "Echo");
        let declaration = identity(&context, DefinitionKind::Function, "default_echo");
        let parameter = GenericParam {
            owner: declaration,
            position: 0,
        };
        let mut body = function(
            "default_echo",
            CallableImplementation::Native(identity(&context, DefinitionKind::Function, "handler")),
            parameter.as_type(),
        );
        body.generic_params.push(parameter.clone());
        body.bounds.push(GenericBound {
            ty: parameter.as_type(),
            constraints: vec![interface(&owner, vec![])],
        });
        let binding = identity(&context, DefinitionKind::Function, "handler");
        let method = function(
            "echo",
            CallableImplementation::Native(binding),
            Ty::SelfType(owner),
        );
        let mut derived = method.clone();
        derived.params[0].ty = scalar();
        let implementation = identity(&context, DefinitionKind::Impl, "");
        context
            .intern_child(owner, DefinitionKind::Method, "echo", 0)
            .unwrap();
        context
            .intern_child(implementation, DefinitionKind::Method, "echo", 0)
            .unwrap();
        Self {
            context: context.clone(),
            owner,
            contract: TraitDef {
                conversion_adapter: None,
                name: "Echo".into(),
                generic_params: vec![],
                bounds: vec![],
                supertraits: vec![],
                associated_types: vec![],
                associated_consts: vec![],
                methods: vec![method],
            },
            template: NativeDeclaration {
                path_operation: None,
                concrete_result: None,
                declaration,
                function: body,
                callable_requirements: vec![],
            },
            table: InterfaceTable {
                method_origins: vec![crate::types::named::FunctionOrigin {
                    placement: context
                        .snapshot()
                        .lookup_child(implementation, DefinitionKind::Method, "echo", 0)
                        .unwrap(),
                    authored: context
                        .snapshot()
                        .lookup_child(owner, DefinitionKind::Method, "echo", 0)
                        .unwrap(),
                }],
                declaration: implementation,
                name: "script_default".into(),
                generic_params: vec![],
                bounds: vec![],
                trait_type: Ty::Trait(interface(&owner, vec![])),
                for_type: scalar(),
                methods: vec![derived],
                associated_consts: vec![],
                associated_type_families: vec![],
                bridge: None,
            },
        }
    }

    fn catalog(&self) -> Result<ProofCatalog<'_>, TypeTransformError> {
        ProofCatalog::new(
            vec![(&self.table).into()],
            [],
            [(self.owner, &self.contract)],
            [&self.template],
            &self.context.snapshot(),
            &CancellationToken::default(),
        )
    }

    fn requirement(&self) -> NativeCallableRequirement {
        let member = self
            .context
            .intern_child(self.owner, DefinitionKind::Method, "echo", 0)
            .unwrap();
        NativeCallableRequirement {
            receiver: self.table.for_type.clone(),
            interface: match &self.table.trait_type {
                Ty::Trait(ty) => ty.clone(),
                _ => unreachable!(),
            },
            member,
            arguments: vec![],
        }
    }
}

#[test]
fn native_defaults_preserve_inherited_binding_and_signature() {
    let fixture = Fixture::new();
    let cancel = CancellationToken::default();
    verify::validate(
        &[
            PublicItem::Trait(fixture.contract.clone()),
            PublicItem::InterfaceTable(Box::new(fixture.table.clone())),
        ],
        &module(),
        &cancel,
        &fixture.context.snapshot(),
    )
    .unwrap();
    let catalog = fixture.catalog().unwrap();
    assert!(verify::interface_methods_match(
        &fixture.table,
        &fixture.contract,
        &|ty| catalog.normalize(ty, &cancel),
        &cancel,
        &fixture.context.snapshot()
    ));
    let selected = catalog
        .select_callable(&fixture.requirement(), &cancel)
        .unwrap()
        .unwrap();
    let method = fixture
        .context
        .snapshot()
        .lookup_child(fixture.table.declaration, DefinitionKind::Method, "echo", 0)
        .unwrap();
    assert_eq!(selected.instance.declaration, method);
    assert!(selected.instance.arguments.is_empty());
    assert_eq!(
        selected.implementation,
        fixture.contract.methods[0].implementation
    );
    assert_eq!(selected.signature.params, [scalar()]);
    assert_eq!(selected.signature.result, scalar());
    for mutation in 0..3 {
        let mut table = fixture.table.clone();
        match mutation {
            0 => {
                table.methods[0].implementation =
                    CallableImplementation::Native(fixture.template.declaration);
            }
            1 => table.methods[0].params[0].ty = Ty::Builtin(BuiltinType::Bool),
            _ => table.methods[0].return_type = Ty::Builtin(BuiltinType::Bool),
        }
        assert!(!verify::interface_methods_match(
            &table,
            &fixture.contract,
            &|ty| catalog.normalize(ty, &cancel),
            &cancel,
            &fixture.context.snapshot()
        ));
    }
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert_eq!(
        catalog.select_callable(&fixture.requirement(), &cancelled),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn required_methods_do_not_become_native_defaults() {
    let mut fixture = Fixture::new();
    fixture.contract.methods[0].implementation = CallableImplementation::Required;
    fixture.table.methods.clear();
    assert!(!verify::interface_methods_match(
        &fixture.table,
        &fixture.contract,
        &|ty| Ok(ty.clone()),
        &Default::default(),
        &fixture.context.snapshot()
    ));
}

#[test]
fn inherited_script_defaults_require_the_canonical_ordinary_materialization() {
    let mut fixture = Fixture::new();
    fixture.contract.methods[0].implementation = CallableImplementation::Script;
    fixture.table.methods[0].implementation = CallableImplementation::Script;
    let valid = |table: &InterfaceTable| {
        verify::interface_methods_match(
            table,
            &fixture.contract,
            &|ty| Ok(ty.clone()),
            &Default::default(),
            &fixture.context.snapshot(),
        )
    };
    assert!(valid(&fixture.table));
    let identity = crate::types::ConcreteFunctionIdentity {
        declaration: fixture.table.method_origins[0].placement,
        arguments: vec![],
    };
    let mut source = crate::updates::SourceRecord {
        declaration: fixture.table.method_origins[0].authored,
        generic: None,
        tokens: vec![],
        inputs: vec![],
        calls: vec![],
        bodies: vec![identity.clone()],
    };
    assert_eq!(
        crate::types::materialization::script_bodies(
            [&fixture.table],
            [&identity],
            &[source.clone()],
            &Default::default()
        ),
        Ok(())
    );
    source.declaration = identity.declaration;
    assert_eq!(
        crate::types::materialization::script_bodies(
            [&fixture.table],
            [&identity],
            &[source],
            &Default::default()
        ),
        Err(TypeTransformError::InvalidContract)
    );
    let mut forged = fixture.table.clone();
    // An explicit implementation has no inherited origin.
    forged.method_origins.clear();
    assert!(valid(&forged));
    forged.method_origins = fixture.table.method_origins.clone();
    forged.method_origins[0].authored = fixture
        .context
        .intern_child(fixture.owner, DefinitionKind::Method, "replacement", 0)
        .unwrap();
    assert!(!valid(&forged));
}
