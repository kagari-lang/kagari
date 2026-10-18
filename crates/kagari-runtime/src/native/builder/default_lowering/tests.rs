use super::{DefaultTemplateSpec, LoweredDefault, lower_default, trait_default_assumptions};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, table::DefinitionId,
    },
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        AssociatedTypeDef, FnDecl, Param, TraitDef, requirement::NativeCallableRequirement,
    },
    scalar::BuiltinType,
    ty::{
        GenericBound, GenericParam, NominalTy, Ty,
        substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
    },
};
use std::{collections::BTreeMap, slice};

struct Fixture {
    ids: DefinitionContext,
    iterable: DefinitionId,
    iterator: DefinitionId,
    item: DefinitionId,
    iter: DefinitionId,
    iterator_item: DefinitionId,
}

impl Fixture {
    fn new() -> Self {
        let ids = DefinitionContext::new().unwrap();
        let iterable = identity(&ids, DefinitionKind::Trait, "Iterable");
        let iterator = identity(&ids, DefinitionKind::Trait, "Iterator");
        let item = child(&ids, iterable, DefinitionKind::AssociatedType, "Item");
        let iter = child(&ids, iterable, DefinitionKind::AssociatedType, "Iter");
        let iterator_item = child(&ids, iterator, DefinitionKind::AssociatedType, "Item");
        Self {
            ids,
            iterable,
            iterator,
            item,
            iter,
            iterator_item,
        }
    }

    fn projection(&self, receiver: &Ty, member: DefinitionId) -> Ty {
        projection(
            receiver.clone(),
            nominal(self.iterable, vec![]),
            member,
            vec![],
        )
    }

    fn iterable(&self, item: Ty, cursor: Option<Ty>) -> NominalTy {
        let mut interface = nominal(self.iterable, vec![]);
        interface.associated_types.insert(self.item, item);
        if let Some(cursor) = cursor {
            interface.associated_types.insert(self.iter, cursor);
        }
        interface
    }

    fn source(
        &self,
        receiver: Ty,
        item: Ty,
        cursor: Ty,
    ) -> (Vec<GenericBound>, Vec<NativeCallableRequirement>) {
        let iterable = self.iterable(item.clone(), Some(cursor.clone()));
        let mut iterator = nominal(self.iterator, vec![]);
        iterator.associated_types.insert(self.iterator_item, item);
        (
            vec![
                bound(receiver.clone(), iterable.clone()),
                bound(cursor.clone(), iterator.clone()),
            ],
            vec![
                self.call(receiver, iterable, "iter", vec![]),
                self.call(cursor, iterator, "next", vec![]),
            ],
        )
    }

    fn call(
        &self,
        receiver: Ty,
        interface: NominalTy,
        name: &str,
        arguments: Vec<Ty>,
    ) -> NativeCallableRequirement {
        NativeCallableRequirement {
            receiver,
            member: child(
                &self.ids,
                interface.declaration,
                DefinitionKind::Method,
                name,
            ),
            interface,
            arguments,
        }
    }

    fn lower(&self, spec: DefaultTemplateSpec<'_>) -> LoweredDefault {
        lower_default(spec, &self.ids.snapshot(), &CancellationToken::default()).unwrap()
    }
}

fn identity(ids: &DefinitionContext, kind: DefinitionKind, name: &str) -> DefinitionId {
    let root = ids
        .intern_root(&ModuleIdentity {
            package: PackageId("example".into()),
            path: vec!["defaults".into()],
        })
        .unwrap();
    child(ids, root, kind, name)
}

fn child(
    ids: &DefinitionContext,
    owner: DefinitionId,
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    ids.intern_child(owner, kind, name, 0).unwrap()
}

fn nominal(declaration: DefinitionId, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration,
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn parameter(owner: DefinitionId, position: usize) -> Ty {
    GenericParam { owner, position }.as_type()
}

fn projection(receiver: Ty, interface: NominalTy, member: DefinitionId, arguments: Vec<Ty>) -> Ty {
    Ty::Projection {
        receiver: Box::new(receiver),
        interface: Box::new(interface),
        member,
        arguments,
    }
}

fn bound(ty: Ty, interface: NominalTy) -> GenericBound {
    GenericBound {
        ty,
        constraints: vec![interface],
    }
}

fn method(name: &str, receiver: Ty, result: Ty) -> FnDecl {
    FnDecl {
        name: name.into(),
        host_policy: None,
        method_policy: MethodPolicy::default(),
        implementation: CallableImplementation::Required,
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "self".into(),
            ty: receiver,
            mutable: false,
        }],
        return_type: result,
    }
}

fn assert_bounds(fixture: &Fixture, actual: &[GenericBound], expected: Vec<GenericBound>) {
    assert_eq!(
        actual,
        GenericBound::normalize(expected, &fixture.ids.snapshot(), &Default::default()).unwrap()
    );
}

#[test]
fn list_hidden_cursor_preserves_requirements_and_concrete_result() {
    let fixture = Fixture::new();
    let list = identity(&fixture.ids, DefinitionKind::Trait, "List");
    let ord = identity(&fixture.ids, DefinitionKind::Trait, "Ord");
    let vec = identity(&fixture.ids, DefinitionKind::AssociatedType, "Vec");
    let method_id = child(&fixture.ids, list, DefinitionKind::Method, "sorted_by_key");
    let template = identity(&fixture.ids, DefinitionKind::Function, "list_sorted_by_key");
    let receiver = Ty::SelfType(list);
    let item = parameter(list, 0);
    let key = parameter(method_id, 0);
    let cursor = projection(
        receiver.clone(),
        fixture.iterable(item.clone(), None),
        fixture.iter,
        vec![],
    );
    let arguments = vec![receiver.clone(), item.clone(), key.clone(), cursor.clone()];
    let mut method = method(
        "sorted_by_key",
        receiver.clone(),
        Ty::Trait(nominal(list, vec![item.clone()])),
    );
    method.generic_params.push(GenericParam {
        owner: method_id,
        position: 0,
    });
    method.bounds.push(bound(key.clone(), nominal(ord, vec![])));
    method.params.push(Param {
        name: "key".into(),
        mutable: false,
        ty: Ty::Function {
            params: vec![item.clone()],
            result: Box::new(key.clone()),
        },
    });
    let (mut assumptions, source_calls) = fixture.source(receiver.clone(), item.clone(), cursor);
    assumptions.push(bound(receiver, nominal(list, vec![item.clone()])));
    let mut requirements = vec![fixture.call(key, nominal(ord, vec![]), "cmp", vec![])];
    requirements.extend(source_calls);
    let concrete_result = Ty::NativeObject(nominal(vec, vec![item]));
    let original = method.clone();
    let lowered = fixture.lower(DefaultTemplateSpec {
        method: &method,
        template,
        arguments: &arguments,
        assumptions: &assumptions,
        requirements: &requirements,
        concrete_result: Some(&concrete_result),
    });
    let p = |index| parameter(template, index);
    assert_eq!(method, original);
    assert_eq!(lowered.application.arguments, arguments);
    assert_eq!(lowered.application.declaration, template);
    assert_eq!(lowered.template.name, "list_sorted_by_key");
    assert_eq!(
        lowered.template.implementation,
        CallableImplementation::Native(template)
    );
    assert_eq!(
        lowered
            .template
            .generic_params
            .iter()
            .map(GenericParam::as_type)
            .collect::<Vec<_>>(),
        (0..4).map(p).collect::<Vec<_>>()
    );
    assert_eq!(lowered.template.params[0].name, "__receiver");
    assert_eq!(lowered.template.params[0].ty, p(0));
    assert_eq!(
        lowered.template.params[1].ty,
        Ty::Function {
            params: vec![p(1)],
            result: Box::new(p(2))
        }
    );
    assert_eq!(
        lowered.template.return_type,
        Ty::Trait(nominal(list, vec![p(1)]))
    );
    assert_eq!(
        lowered.concrete_result,
        Some(Ty::NativeObject(nominal(vec, vec![p(1)])))
    );
    assert_eq!(
        lowered.binding_signature().params,
        lowered
            .template
            .params
            .iter()
            .map(|param| param.ty.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        lowered.binding_signature().result,
        lowered.concrete_result.clone().unwrap()
    );
    let (mut expected_bounds, expected_calls) = fixture.source(p(0), p(1), p(3));
    expected_bounds.extend([
        bound(p(0), nominal(list, vec![p(1)])),
        bound(p(2), nominal(ord, vec![])),
    ]);
    assert_bounds(&fixture, &lowered.template.bounds, expected_bounds);
    let mut expected = vec![fixture.call(p(2), nominal(ord, vec![]), "cmp", vec![])];
    expected.extend(expected_calls);
    assert_eq!(lowered.requirements, expected);
}

#[test]
fn paired_iterables_keep_distinct_receivers_items_and_cursors() {
    for paired in ["zip", "chain"] {
        let fixture = Fixture::new();
        let template = identity(&fixture.ids, DefinitionKind::Function, paired);
        let method_id = child(
            &fixture.ids,
            fixture.iterable,
            DefinitionKind::Method,
            paired,
        );
        let adapter = identity(&fixture.ids, DefinitionKind::AssociatedType, "Adapter");
        let left = Ty::SelfType(fixture.iterable);
        let item = fixture.projection(&left, fixture.item);
        let right = parameter(method_id, 0);
        let right_item = if paired == "zip" {
            fixture.projection(&right, fixture.item)
        } else {
            item.clone()
        };
        let left_cursor = fixture.projection(&left, fixture.iter);
        let right_cursor = if paired == "chain" {
            projection(
                right.clone(),
                fixture.iterable(item.clone(), None),
                fixture.iter,
                vec![],
            )
        } else {
            fixture.projection(&right, fixture.iter)
        };
        let mut arguments = vec![left.clone(), item.clone(), right.clone()];
        if paired == "zip" {
            arguments.push(right_item.clone());
        }
        arguments.extend([left_cursor.clone(), right_cursor.clone()]);
        let mut method = method(
            paired,
            left.clone(),
            Ty::NativeObject(nominal(adapter, vec![item.clone(), right_item.clone()])),
        );
        method.generic_params.push(GenericParam {
            owner: method_id,
            position: 0,
        });
        method.params.push(Param {
            name: "other".into(),
            ty: right.clone(),
            mutable: false,
        });
        method.bounds.push(bound(
            right.clone(),
            if paired == "zip" {
                nominal(fixture.iterable, vec![])
            } else {
                fixture.iterable(item.clone(), None)
            },
        ));
        let (mut assumptions, mut requirements) = fixture.source(left, item, left_cursor);
        let (right_bounds, right_calls) = fixture.source(right, right_item, right_cursor);
        assumptions.extend(right_bounds);
        requirements.extend(right_calls);
        let lowered = fixture.lower(DefaultTemplateSpec {
            method: &method,
            template,
            arguments: &arguments,
            assumptions: &assumptions,
            requirements: &requirements,
            concrete_result: None,
        });
        let p = |index| parameter(template, index);
        let (right_item, left_cursor, right_cursor) = if paired == "zip" {
            (3, 4, 5)
        } else {
            (1, 3, 4)
        };
        let (mut expected_bounds, mut expected_calls) = fixture.source(p(0), p(1), p(left_cursor));
        let (right_bounds, right_calls) = fixture.source(p(2), p(right_item), p(right_cursor));
        expected_bounds.extend(right_bounds);
        expected_bounds.push(bound(
            p(2),
            if paired == "zip" {
                nominal(fixture.iterable, vec![])
            } else {
                fixture.iterable(p(1), None)
            },
        ));
        expected_calls.extend(right_calls);
        assert_eq!(lowered.application.arguments, arguments);
        assert_eq!(lowered.template.generic_params.len(), arguments.len());
        assert_eq!(lowered.template.params[1].ty, p(2));
        assert_eq!(
            lowered.template.return_type,
            Ty::NativeObject(nominal(adapter, vec![p(1), p(right_item)]))
        );
        assert_eq!(lowered.requirements, expected_calls);
        assert_bounds(&fixture, &lowered.template.bounds, expected_bounds);
        assert_eq!(lowered.concrete_result, None);
        assert_eq!(
            lowered.binding_signature().result,
            lowered.template.return_type
        );
    }
}

#[test]
fn destination_defaults_keep_only_the_destination_requirement() {
    for (name, protocol, operation) in [
        ("collect", "FromIterator", "from_iter"),
        ("sum", "Sum", "sum"),
        ("product", "Product", "product"),
    ] {
        let fixture = Fixture::new();
        let destination = identity(&fixture.ids, DefinitionKind::Trait, protocol);
        let template = identity(&fixture.ids, DefinitionKind::Function, name);
        let method_id = child(&fixture.ids, fixture.iterable, DefinitionKind::Method, name);
        let receiver = Ty::SelfType(fixture.iterable);
        let item = fixture.projection(&receiver, fixture.item);
        let result = parameter(method_id, 0);
        let arguments = vec![receiver.clone(), item.clone(), result.clone()];
        let interface = nominal(destination, vec![item.clone()]);
        let mut method = method(name, receiver.clone(), result.clone());
        method.generic_params.push(GenericParam {
            owner: method_id,
            position: 0,
        });
        method.bounds.push(bound(result.clone(), interface.clone()));
        let assumptions = vec![bound(receiver.clone(), fixture.iterable(item, None))];
        let requirements = vec![fixture.call(result, interface, operation, vec![receiver])];
        let lowered = fixture.lower(DefaultTemplateSpec {
            method: &method,
            template,
            arguments: &arguments,
            assumptions: &assumptions,
            requirements: &requirements,
            concrete_result: None,
        });
        let p = |index| parameter(template, index);
        assert_eq!(lowered.application.arguments, arguments);
        assert_eq!(lowered.template.generic_params.len(), 3);
        assert_eq!(lowered.template.return_type, p(2));
        assert_eq!(
            lowered.requirements,
            vec![fixture.call(
                p(2),
                nominal(destination, vec![p(1)]),
                operation,
                vec![p(0)]
            )]
        );
        assert_bounds(
            &fixture,
            &lowered.template.bounds,
            vec![
                bound(p(0), fixture.iterable(p(1), None)),
                bound(p(2), nominal(destination, vec![p(1)])),
            ],
        );
    }
}

#[test]
fn ordinary_assumptions_preserve_symbolic_outputs_without_recursive_equalities() {
    let fixture = Fixture::new();
    let owner = identity(&fixture.ids, DefinitionKind::Trait, "Source");
    let output = child(
        &fixture.ids,
        owner,
        DefinitionKind::AssociatedType,
        "Output",
    );
    let unbounded = child(
        &fixture.ids,
        owner,
        DefinitionKind::AssociatedType,
        "Unbounded",
    );
    let method_id = child(&fixture.ids, owner, DefinitionKind::Method, "get");
    let template = identity(&fixture.ids, DefinitionKind::Function, "source_get");
    let receiver = Ty::SelfType(owner);
    let item = parameter(owner, 0);
    let extra = parameter(method_id, 0);
    let output_ty = projection(
        receiver.clone(),
        nominal(owner, vec![item.clone()]),
        output,
        vec![],
    );
    let mut method = method(
        "get",
        receiver.clone(),
        Ty::Tuple(vec![output_ty, extra.clone()]),
    );
    method.generic_params.push(GenericParam {
        owner: method_id,
        position: 0,
    });
    let trait_bound = bound(item.clone(), nominal(fixture.iterable, vec![]));
    let declaration = TraitDef {
        name: "Source".into(),
        generic_params: vec![GenericParam { owner, position: 0 }],
        bounds: vec![trait_bound.clone()],
        methods: vec![method.clone()],
        supertraits: vec![nominal(fixture.iterable, vec![])],
        associated_consts: vec![],
        conversion_adapter: None,
        associated_types: vec![
            AssociatedTypeDef {
                declaration: output,
                generic_params: vec![],
                parameter_bounds: vec![],
                bounds: vec![fixture.iterable(item.clone(), None)],
            },
            AssociatedTypeDef {
                declaration: unbounded,
                generic_params: vec![],
                parameter_bounds: vec![],
                bounds: vec![],
            },
        ],
    };
    let assumptions = trait_default_assumptions(owner, &declaration);
    assert_eq!(assumptions.len(), 3);
    assert_eq!(assumptions[0], trait_bound);
    assert_eq!(
        assumptions[2],
        bound(receiver.clone(), nominal(owner, vec![item.clone()]))
    );
    let lowered = fixture.lower(DefaultTemplateSpec {
        method: &method,
        template,
        arguments: &[receiver, item, extra],
        assumptions: &assumptions,
        requirements: &[],
        concrete_result: None,
    });
    let p = |index| parameter(template, index);
    let output = projection(p(0), nominal(owner, vec![p(1)]), output, vec![]);
    assert_eq!(
        lowered.template.return_type,
        Ty::Tuple(vec![output.clone(), p(2)])
    );
    assert_bounds(
        &fixture,
        &lowered.template.bounds,
        vec![
            bound(p(1), nominal(fixture.iterable, vec![])),
            bound(output, fixture.iterable(p(1), None)),
            bound(p(0), nominal(owner, vec![p(1)])),
        ],
    );
}

#[test]
fn nested_projection_keys_are_order_independent_and_keep_application_arguments() {
    let fixture = Fixture::new();
    let template = identity(&fixture.ids, DefinitionKind::Function, "nested");
    let receiver = Ty::SelfType(fixture.iterable);
    let inner = fixture.projection(&receiver, fixture.item);
    let middle = fixture.projection(&inner, fixture.item);
    let outer = fixture.projection(&middle, fixture.iter);
    let applied = projection(
        receiver.clone(),
        nominal(fixture.iterable, vec![inner.clone()]),
        fixture.iter,
        vec![middle.clone()],
    );
    let unlifted = projection(
        receiver.clone(),
        nominal(fixture.iterable, vec![inner.clone()]),
        fixture.iter,
        vec![Ty::Builtin(BuiltinType::Bool)],
    );
    let method = method(
        "nested",
        receiver.clone(),
        Ty::Tuple(vec![outer.clone(), applied.clone(), unlifted.clone()]),
    );
    let arguments = vec![outer, applied, middle, inner, receiver];
    let lowered = fixture.lower(DefaultTemplateSpec {
        method: &method,
        template,
        arguments: &arguments,
        assumptions: &[],
        requirements: &[],
        concrete_result: None,
    });
    let p = |index| parameter(template, index);
    assert_eq!(lowered.application.arguments, arguments);
    assert_eq!(
        lowered.template.return_type,
        Ty::Tuple(vec![
            p(0),
            p(1),
            projection(
                p(4),
                nominal(fixture.iterable, vec![p(3)]),
                fixture.iter,
                vec![Ty::Builtin(BuiltinType::Bool)]
            )
        ])
    );
}

#[test]
fn projection_keys_distinguish_interface_and_member_arguments() {
    let fixture = Fixture::new();
    let template = identity(&fixture.ids, DefinitionKind::Function, "applied_outputs");
    let family = identity(&fixture.ids, DefinitionKind::Trait, "Family");
    let output = child(
        &fixture.ids,
        family,
        DefinitionKind::AssociatedType,
        "Output",
    );
    let receiver = Ty::SelfType(family);
    let integer = Ty::Builtin(BuiltinType::I32);
    let boolean = Ty::Builtin(BuiltinType::Bool);
    let mut arguments = vec![receiver.clone()];
    for (interface_argument, member_argument) in [
        (integer.clone(), integer.clone()),
        (boolean.clone(), integer),
        (boolean.clone(), boolean),
    ] {
        arguments.push(projection(
            receiver.clone(),
            nominal(family, vec![interface_argument]),
            output,
            vec![member_argument],
        ));
    }
    let method = method(
        "applied_outputs",
        receiver,
        Ty::Tuple(arguments[1..].to_vec()),
    );
    let lowered = fixture.lower(DefaultTemplateSpec {
        method: &method,
        template,
        arguments: &arguments,
        assumptions: &[],
        requirements: &[],
        concrete_result: None,
    });
    assert_eq!(lowered.application.arguments, arguments);
    assert_eq!(
        lowered.template.return_type,
        Ty::Tuple(
            (1..4)
                .map(|position| parameter(template, position))
                .collect()
        )
    );
}

#[test]
fn duplicate_lifts_including_refined_and_normalized_projection_aliases_are_rejected() {
    let fixture = Fixture::new();
    let template = identity(&fixture.ids, DefinitionKind::Function, "duplicates");
    let receiver = Ty::SelfType(fixture.iterable);
    let item = fixture.projection(&receiver, fixture.item);
    let cursor = fixture.projection(&receiver, fixture.iter);
    let refined_cursor = projection(
        receiver.clone(),
        fixture.iterable(item.clone(), None),
        fixture.iter,
        vec![],
    );
    let method = method("duplicates", receiver.clone(), item.clone());
    let nested = fixture.projection(&item, fixture.iter);
    let already_normalized = fixture.projection(&parameter(template, 1), fixture.iter);
    for arguments in [
        vec![receiver.clone(), receiver.clone()],
        vec![
            parameter(fixture.iterable, 0),
            parameter(fixture.iterable, 0),
        ],
        vec![receiver.clone(), cursor.clone(), cursor.clone()],
        vec![receiver.clone(), item.clone(), cursor, refined_cursor],
        vec![receiver, item, nested, already_normalized],
        vec![Ty::Builtin(BuiltinType::Bool)],
    ] {
        assert!(
            lower_default(
                DefaultTemplateSpec {
                    method: &method,
                    template,
                    arguments: &arguments,
                    assumptions: &[],
                    requirements: &[],
                    concrete_result: None,
                },
                &fixture.ids.snapshot(),
                &Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn lowering_rejects_foreign_and_stale_ids_even_when_substitution_would_hide_them() {
    let fixture = Fixture::new();
    let foreign = Fixture::new();
    let template = identity(&fixture.ids, DefinitionKind::Function, "scoped");
    let receiver = Ty::SelfType(fixture.iterable);
    let method = method("scoped", receiver.clone(), receiver.clone());
    for arguments in [
        vec![Ty::SelfType(foreign.iterable)],
        vec![fixture.projection(&receiver, foreign.item)],
    ] {
        assert!(
            lower_default(
                DefaultTemplateSpec {
                    method: &method,
                    template,
                    arguments: &arguments,
                    assumptions: &[],
                    requirements: &[],
                    concrete_result: None,
                },
                &fixture.ids.snapshot(),
                &Default::default()
            )
            .is_err()
        );
    }
    let mut requirement = fixture.call(
        receiver.clone(),
        nominal(fixture.iterable, vec![]),
        "iter",
        vec![],
    );
    requirement.member = child(
        &foreign.ids,
        foreign.iterable,
        DefinitionKind::Method,
        "iter",
    );
    assert!(
        lower_default(
            DefaultTemplateSpec {
                method: &method,
                template,
                arguments: slice::from_ref(&receiver),
                assumptions: &[],
                requirements: &[requirement],
                concrete_result: None,
            },
            &fixture.ids.snapshot(),
            &Default::default()
        )
        .is_err()
    );
    let stale = fixture.ids.snapshot();
    let later = identity(&fixture.ids, DefinitionKind::Function, "later");
    for (definitions, template) in [
        (stale, later),
        (fixture.ids.snapshot(), fixture.iterable),
        (
            fixture.ids.snapshot(),
            identity(&foreign.ids, DefinitionKind::Function, "scoped"),
        ),
    ] {
        assert!(
            lower_default(
                DefaultTemplateSpec {
                    method: &method,
                    template,
                    arguments: slice::from_ref(&receiver),
                    assumptions: &[],
                    requirements: &[],
                    concrete_result: None,
                },
                &definitions,
                &Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn lowering_is_bounded_and_cancellable() {
    let fixture = Fixture::new();
    let template = identity(&fixture.ids, DefinitionKind::Function, "bounded");
    let receiver = Ty::SelfType(fixture.iterable);
    let mut method = method("bounded", receiver.clone(), receiver.clone());
    let cancel = CancellationToken::default();
    cancel.cancel();
    let error = lower_default(
        DefaultTemplateSpec {
            method: &method,
            template,
            arguments: slice::from_ref(&receiver),
            assumptions: &[],
            requirements: &[],
            concrete_result: None,
        },
        &fixture.ids.snapshot(),
        &cancel,
    )
    .unwrap_err();
    assert!(error.0.contains("Cancelled"), "{error}");
    let oversized = vec![receiver.clone(); MAX_TYPE_NODES + 1];
    assert!(
        lower_default(
            DefaultTemplateSpec {
                method: &method,
                template,
                arguments: &oversized,
                assumptions: &[],
                requirements: &[],
                concrete_result: None,
            },
            &fixture.ids.snapshot(),
            &Default::default()
        )
        .is_err()
    );
    for _ in 0..=MAX_TYPE_DEPTH {
        method.return_type = Ty::Array(Box::new(method.return_type));
    }
    assert!(
        lower_default(
            DefaultTemplateSpec {
                method: &method,
                template,
                arguments: &[receiver],
                assumptions: &[],
                requirements: &[],
                concrete_result: None,
            },
            &fixture.ids.snapshot(),
            &Default::default()
        )
        .is_err()
    );
}
