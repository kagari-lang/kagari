use crate::aggregates::implementations::*;
use crate::{tests::common, types::TypeId};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::scalar::BuiltinType;

fn definition(context: &DefinitionContext, kind: DefinitionKind, name: &str) -> DefinitionId {
    let root = context
        .intern_root(&ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["module".into()],
        })
        .unwrap();
    context.intern_child(root, kind, name, 0).unwrap()
}

#[test]
fn bounded_trait_search_reuses_generic_matching_and_stops_growth() {
    let source = common::lower_ok("trait Marker<A> { type Output; fn get(self); }");
    let checked = crate::analyze_source(&source.source, foundation_catalog::shared().unwrap())
        .unwrap()
        .into_checked()
        .unwrap();
    let context = checked.declarations.context().clone();
    let base = &checked.aggregates;
    let contract = base
        .traits()
        .find(|t| t.declaration.name == "Marker")
        .unwrap();
    let marker = contract.id;
    let method = contract.methods[0].id;
    let owner = definition(&context, DefinitionKind::Impl, "");
    let target = context
        .intern_child(owner, DefinitionKind::Function, "get", 0)
        .unwrap();
    let parameter = GenericParameterType {
        owner,
        position: 0,
        name: "T".into(),
    };
    let applied = |argument| NominalType {
        associated_types: Default::default(),
        declaration: marker,
        arguments: vec![argument],
    };
    let signature = ImplementationSignature {
        associated_type_families: Default::default(),
        id: owner,
        trait_type: applied(TypeId::Generic(parameter.clone())),
        for_type: TypeId::Generic(parameter.clone()),
        generic_params: vec![parameter.clone()],
        bounds: Default::default(),
        methods: [(method, target)].into(),
    };
    let catalog = AggregateCatalog {
        implementations: [(signature.id, Arc::new(signature.clone()))].into(),
        ..base.clone()
    };
    let actual = TypeId::Builtin(BuiltinType::I32);
    let required = applied(actual.clone());
    let cancel = CancellationToken::default();
    let fresh_cancel = CancellationToken::default();
    let assumptions = GenericBounds::default();
    let search = |max_checks, max_depth, cancel| ImplementationSearchContext {
        assumptions: &assumptions,
        limits: ImplementationSearchLimits {
            max_checks,
            max_depth,
        },
        cancel,
    };
    for impossible in [
        NominalType {
            declaration: definition(&context, DefinitionKind::Trait, "Other"),
            ..required.clone()
        },
        NominalType {
            arguments: Vec::new(),
            ..required.clone()
        },
    ] {
        // Even an impossible header consumes its candidate work before rejection.
        for (checks, expected) in [
            (0, Err(ImplementationSearchError::LimitExceeded)),
            (1, Ok(0)),
        ] {
            assert_eq!(
                catalog.implementation_count_bounded(
                    &context.snapshot(),
                    &impossible,
                    &actual,
                    search(checks, 4, &cancel),
                ),
                expected
            );
        }
    }
    assert_eq!(
        catalog.implementation_count_bounded(
            &context.snapshot(),
            &required,
            &actual,
            search(1, 4, &cancel),
        ),
        Ok(1)
    );
    assert_eq!(
        catalog.implementation_count_bounded(
            &context.snapshot(),
            &required,
            &actual,
            search(0, 4, &cancel),
        ),
        Err(ImplementationSearchError::LimitExceeded)
    );
    assert_eq!(
        catalog.implementation_method(
            &context.snapshot(),
            &method,
            &required,
            &actual,
            search(1, 4, &cancel),
        ),
        Ok(Some((target, vec![actual.clone()])))
    );
    assert_eq!(
        catalog.implementation_method(
            &context.snapshot(),
            &method,
            &required,
            &actual,
            search(0, 4, &cancel),
        ),
        Err(ImplementationSearchError::LimitExceeded)
    );
    cancel.cancel();
    assert_eq!(
        catalog.implementation_count_bounded(
            &context.snapshot(),
            &required,
            &actual,
            search(1, 4, &cancel),
        ),
        Err(ImplementationSearchError::Cancelled)
    );
    assert_eq!(
        catalog.implementation_method(
            &context.snapshot(),
            &method,
            &required,
            &actual,
            search(1, 4, &cancel),
        ),
        Err(ImplementationSearchError::Cancelled)
    );
    let empty = AggregateCatalog::default();
    assert_eq!(
        empty.implementation_count_bounded(
            &context.snapshot(),
            &required,
            &actual,
            search(0, 0, &cancel),
        ),
        Err(ImplementationSearchError::Cancelled)
    );
    assert_eq!(
        empty.implementation_method(
            &context.snapshot(),
            &method,
            &required,
            &actual,
            search(0, 0, &cancel),
        ),
        Err(ImplementationSearchError::Cancelled)
    );
    assert_eq!(
        empty.concrete_interface_implementation(
            &context.snapshot(),
            &required,
            &actual,
            &assumptions,
            ImplementationSearchLimits {
                max_checks: 0,
                max_depth: 0,
            },
            &cancel,
        ),
        Err(ImplementationSearchError::Cancelled)
    );

    let duplicate = definition(&context, DefinitionKind::Impl, "duplicate");
    let duplicate_target = context
        .intern_child(duplicate, DefinitionKind::Function, "get", 0)
        .unwrap();
    let ambiguous = AggregateCatalog {
        implementations: [
            signature.clone(),
            ImplementationSignature {
                id: duplicate,
                trait_type: required.clone(),
                for_type: actual.clone(),
                generic_params: Vec::new(),
                methods: [(method, duplicate_target)].into(),
                ..signature.clone()
            },
        ]
        .into_iter()
        .map(|signature| (signature.id, Arc::new(signature)))
        .collect(),
        ..base.clone()
    };
    for (checks, count) in [
        (1, Err(ImplementationSearchError::LimitExceeded)),
        (2, Ok(2)),
    ] {
        assert_eq!(
            ambiguous.implementation_count_bounded(
                &context.snapshot(),
                &required,
                &actual,
                search(checks, 4, &fresh_cancel),
            ),
            count
        );
        assert_eq!(
            ambiguous.implementation_method(
                &context.snapshot(),
                &method,
                &required,
                &actual,
                search(checks, 4, &fresh_cancel),
            ),
            count.map(|_| None)
        );
    }

    // A parameter that occurs only in an output is not determined by this request.
    let mut incomplete = signature.clone();
    let unbound = GenericParameterType {
        position: 1,
        name: "U".into(),
        ..parameter.clone()
    };
    let member = context
        .intern_child(marker, DefinitionKind::AssociatedType, "Output", 0)
        .unwrap();
    incomplete
        .trait_type
        .associated_types
        .insert(member, TypeId::Generic(unbound.clone()));
    incomplete.generic_params.push(unbound);
    let incomplete_catalog = AggregateCatalog {
        implementations: [(owner, Arc::new(incomplete))].into(),
        ..base.clone()
    };
    assert_eq!(
        incomplete_catalog.implementation_count_bounded(
            &context.snapshot(),
            &required,
            &actual,
            search(1, 0, &fresh_cancel)
        ),
        Ok(0)
    );

    let mut chained = signature;
    let next = definition(&context, DefinitionKind::Trait, "Next");
    chained.bounds.insert(
        TypeId::Generic(parameter.clone()),
        vec![NominalType {
            associated_types: Default::default(),
            declaration: next,
            arguments: vec![TypeId::Generic(parameter.clone())],
        }],
    );
    let next_parameter = GenericParameterType {
        owner: definition(&context, DefinitionKind::Impl, "next"),
        position: 0,
        name: "U".into(),
    };
    let next_signature = ImplementationSignature {
        associated_type_families: Default::default(),
        id: next_parameter.owner,
        trait_type: NominalType {
            associated_types: Default::default(),
            declaration: next,
            arguments: vec![TypeId::Generic(next_parameter.clone())],
        },
        for_type: TypeId::Generic(next_parameter.clone()),
        generic_params: vec![next_parameter.clone()],
        bounds: Default::default(),
        methods: Default::default(),
    };
    let catalog = AggregateCatalog {
        implementations: [chained, next_signature]
            .into_iter()
            .map(|signature| (signature.id, Arc::new(signature)))
            .collect(),
        ..base.clone()
    };
    assert_eq!(
        catalog.implementation_count_bounded(
            &context.snapshot(),
            &required,
            &actual,
            search(100, 0, &fresh_cancel),
        ),
        Err(ImplementationSearchError::LimitExceeded)
    );
    assert_eq!(
        catalog.implementation_count_bounded(
            &context.snapshot(),
            &required,
            &actual,
            search(100, 1, &fresh_cancel),
        ),
        Ok(1)
    );
    assert_eq!(
        catalog.implementation_method(
            &context.snapshot(),
            &method,
            &required,
            &actual,
            search(100, 0, &fresh_cancel),
        ),
        Err(ImplementationSearchError::LimitExceeded)
    );
    assert_eq!(
        catalog.implementation_method(
            &context.snapshot(),
            &method,
            &required,
            &actual,
            search(100, 1, &fresh_cancel),
        ),
        Ok(Some((target, vec![actual.clone()])))
    );

    let catalog = AggregateCatalog {
        implementations: [(
            owner,
            Arc::clone(catalog.implementations.get(&owner).unwrap()),
        )]
        .into(),
        ..base.clone()
    };
    let open = TypeId::Generic(GenericParameterType {
        owner: definition(&context, DefinitionKind::Function, "caller"),
        position: 0,
        name: "V".into(),
    });
    assert_eq!(
        catalog.implementation_count_bounded(
            &context.snapshot(),
            &applied(open.clone()),
            &open,
            search(100, 1, &fresh_cancel),
        ),
        Ok(0)
    );
    assert_eq!(
        catalog.implementation_method(
            &context.snapshot(),
            &method,
            &applied(open.clone()),
            &open,
            search(100, 1, &fresh_cancel),
        ),
        Ok(None)
    );
    let assumptions = [(
        open.clone(),
        vec![NominalType {
            declaration: next,
            arguments: vec![open.clone()],
            associated_types: Default::default(),
        }],
    )]
    .into_iter()
    .collect();
    let cancel = CancellationToken::default();
    let search = ImplementationSearchContext {
        assumptions: &assumptions,
        limits: ImplementationSearchLimits {
            max_checks: 100,
            max_depth: 1,
        },
        cancel: &cancel,
    };
    assert_eq!(
        catalog.implementation_count_bounded(
            &context.snapshot(),
            &applied(open.clone()),
            &open,
            search,
        ),
        Ok(1)
    );
    assert_eq!(
        catalog.implementation_method(
            &context.snapshot(),
            &method,
            &applied(open.clone()),
            &open,
            search,
        ),
        Ok(Some((target, vec![open])))
    );
}
