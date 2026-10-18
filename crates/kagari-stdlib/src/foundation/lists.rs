//! List defaults and Vec overrides share one algorithm inventory, not a semantic lowerer.
use crate::{
    bindings::{Entry, lists, vectors},
    foundation::{bound, enumeration, interface, override_entry, raw},
    identity,
};
use kagari_common::identity::DefinitionKind;
use kagari_runtime::native::{
    assembly::{
        authoring::PendingBinding,
        draft::{ModuleDraft, defaults::DefaultDefinition, traits::TraitOverride},
        references::DraftReference,
    },
    binding::{Codec, NativeBinding, NativeResult},
    declarations::MethodDecl,
    types::Type,
};
use kagari_types::declaration::{TypeDefKind, native::NativeStorageLayout};

#[derive(Clone, Copy)]
enum Callback {
    None,
    Compare,
    Key,
    Predicate,
}

#[derive(Clone, Copy)]
enum Compare {
    None,
    Order,
    Equal,
}

enum VectorBinding {
    Shared,
    Retain,
    Dedup,
}

struct Method {
    name: &'static str,
    callback: Callback,
    compare: Compare,
    entry: Entry,
    vector: VectorBinding,
}

const READ: &[Method] = &[
    Method {
        name: "sorted",
        callback: Callback::None,
        compare: Compare::Order,
        entry: lists::sorted,
        vector: VectorBinding::Shared,
    },
    Method {
        name: "sorted_by",
        callback: Callback::Compare,
        compare: Compare::None,
        entry: lists::sorted_by,
        vector: VectorBinding::Shared,
    },
    Method {
        name: "sorted_by_key",
        callback: Callback::Key,
        compare: Compare::Order,
        entry: lists::sorted_by_key,
        vector: VectorBinding::Shared,
    },
    Method {
        name: "reversed",
        callback: Callback::None,
        compare: Compare::None,
        entry: lists::reversed,
        vector: VectorBinding::Shared,
    },
    Method {
        name: "distinct",
        callback: Callback::None,
        compare: Compare::Equal,
        entry: lists::distinct,
        vector: VectorBinding::Shared,
    },
];

const WRITE: &[Method] = &[
    Method {
        name: "sort",
        callback: Callback::None,
        compare: Compare::Order,
        entry: lists::sort,
        vector: VectorBinding::Shared,
    },
    Method {
        name: "sort_by",
        callback: Callback::Compare,
        compare: Compare::None,
        entry: lists::sort_by,
        vector: VectorBinding::Shared,
    },
    Method {
        name: "sort_by_key",
        callback: Callback::Key,
        compare: Compare::Order,
        entry: lists::sort_by_key,
        vector: VectorBinding::Shared,
    },
    Method {
        name: "reverse",
        callback: Callback::None,
        compare: Compare::None,
        entry: lists::reverse,
        vector: VectorBinding::Shared,
    },
    Method {
        name: "retain",
        callback: Callback::Predicate,
        compare: Compare::None,
        entry: lists::retain,
        vector: VectorBinding::Retain,
    },
    Method {
        name: "dedup",
        callback: Callback::None,
        compare: Compare::Equal,
        entry: lists::dedup,
        vector: VectorBinding::Dedup,
    },
];

pub(super) fn defaults(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let vector = vector(module)?;
    let ordering = enumeration(module, "Ordering", 0)?.apply([])?;
    let iterable = interface(module, "Iterable", 0)?;
    let item_member = module.reference_associated_type(&iterable, "Item", 0)?;
    let cursor_member = module.reference_associated_type(&iterable, "Iter", 0)?;
    let iter = module.reference_method(&iterable, "iter", 0)?;
    let iterator = interface(module, "Iterator", 0)?;
    let iterator_item = module.reference_associated_type(&iterator, "Item", 0)?;
    let next = module.reference_method(&iterator, "next", 0)?;
    for (owner_name, methods, mutable) in [("List", READ, false), ("MutableList", WRITE, true)] {
        let owner = interface(module, owner_name, 1)?;
        let receiver = owner.receiver()?;
        let item = owner.parameter(0)?;
        let iterable = item_member.bind(iterable.apply([])?, item.clone())?;
        let cursor = cursor_member.projection(receiver.clone(), iterable.clone(), [])?;
        let iterable = cursor_member.bind(iterable, cursor.clone())?;
        let iterator = iterator_item.bind(iterator.apply([])?, item.clone())?;
        for method in methods {
            let arity = usize::from(matches!(method.callback, Callback::Key));
            let member = module.reference_method(&owner, method.name, arity)?;
            let key = if arity == 1 {
                member.parameter(0)?
            } else {
                item.clone()
            };
            let mut arguments = vec![receiver.clone(), item.clone()];
            if arity == 1 {
                arguments.push(key.clone());
            }
            arguments.push(cursor.clone());
            let mut requirements = vec![];
            let mut bounds = vec![];
            if let Some((bound_name, protocol_name, name)) = comparison(method.compare) {
                bounds.push(bound(&key, interface(module, bound_name, 0)?.apply([])?));
                let protocol = interface(module, protocol_name, 0)?;
                let compare = module.reference_method(&protocol, name, 0)?;
                requirements.push(compare.requirement(key.clone(), protocol.apply([])?, [])?);
            }
            requirements.push(iter.requirement(receiver.clone(), iterable.clone(), [])?);
            requirements.push(next.requirement(cursor.clone(), iterator.clone(), [])?);
            if mutable {
                let name = if matches!(method.callback, Callback::Predicate)
                    || matches!(method.compare, Compare::Equal)
                {
                    "remove"
                } else {
                    "set"
                };
                let mutation = module.reference_method(&owner, name, 0)?;
                requirements.push(mutation.requirement(
                    receiver.clone(),
                    owner.apply([item.clone()])?,
                    [],
                )?);
            }
            let mut declaration = MethodDecl::instance(method.name)
                .documentation(documentation(method.name))
                .returns(if mutable {
                    Type::unit()
                } else {
                    owner.apply([item.clone()])?
                });
            let callback = match method.callback {
                Callback::None => None,
                Callback::Compare => Some(Type::function(
                    [item.clone(), item.clone()],
                    ordering.clone(),
                )?),
                Callback::Key => Some(Type::function([item.clone()], key.clone())?),
                Callback::Predicate => Some(Type::function([item.clone()], Type::bool())?),
            };
            if let Some(callback) = callback {
                declaration = declaration.parameter("callback", callback);
            }
            let mut declaration = declaration.configure(move |function| {
                if arity == 1 {
                    function.type_parameter("K")?;
                }
                for required in requirements {
                    function.requires_draft(required)?;
                }
                Ok(())
            });
            if !mutable {
                declaration = declaration.produces(vector.apply([item.clone()])?);
            }
            let definition =
                DefaultDefinition::new(declaration.implementation_send(NativeBinding::new(
                    vec![
                        Codec::Value;
                        if matches!(method.callback, Callback::None) {
                            1
                        } else {
                            2
                        }
                    ],
                    Codec::Value,
                    method.entry,
                )))
                .arguments(arguments)
                .bounds(bounds)
                .assumptions([
                    bound(&receiver, iterable.clone()),
                    bound(&cursor, iterator.clone()),
                ]);
            module.define_trait_default(&owner, definition)?;
        }
    }
    Ok(())
}

pub(super) fn overrides(
    module: &mut ModuleDraft<'_>,
    owner: &DraftReference,
    mutable: bool,
) -> NativeResult<Vec<TraitOverride>> {
    let mut overrides = vec![];
    for method in if mutable { WRITE } else { READ } {
        let arity = usize::from(matches!(method.callback, Callback::Key));
        let member = module.reference_method(owner, method.name, arity)?;
        let binding = match method.vector {
            VectorBinding::Retain => PendingBinding::implementation_with_send(|view| {
                Ok(vectors::retain(view.declaration()))
            }),
            VectorBinding::Dedup => {
                PendingBinding::implementation_with_send(|view| vectors::dedup(view.declaration()))
            }
            VectorBinding::Shared => raw(
                if matches!(method.callback, Callback::None) {
                    1
                } else {
                    2
                },
                method.entry,
            ),
        };
        let mut entry = override_entry(module, owner, method.name, arity, binding)?
            .documentation(documentation(method.name));
        if let Some((_, protocol_name, name)) = comparison(method.compare) {
            let protocol = interface(module, protocol_name, 0)?;
            let compare = module.reference_method(&protocol, name, 0)?;
            let key = if arity == 1 {
                member.parameter(0)?
            } else {
                owner.parameter(0)?
            };
            entry = entry.requires(compare.requirement(key, protocol.apply([])?, [])?);
        }
        if !mutable {
            entry = entry.produces(owner.receiver()?);
        }
        overrides.push(entry);
    }
    Ok(overrides)
}

fn comparison(compare: Compare) -> Option<(&'static str, &'static str, &'static str)> {
    match compare {
        Compare::None => None,
        Compare::Order => Some(("Ord", "Ord", "cmp")),
        Compare::Equal => Some(("Eq", "PartialEq", "eq")),
    }
}

pub(super) fn vector(module: &mut ModuleDraft<'_>) -> NativeResult<DraftReference> {
    module.reference_type(
        identity::type_id("Vec", DefinitionKind::AssociatedType),
        TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
        1,
    )
}

fn documentation(name: &str) -> &'static str {
    match name {
        "sorted" => "Return a stably sorted List using T: Ord; preserve the original slots.",
        "sorted_by" => {
            "Return a stably sorted List using the comparator; preserve the original slots."
        }
        "sorted_by_key" => {
            "Return a stably sorted List. Evaluate keys during comparisons without caching them."
        }
        "reversed" => "Return a reversed List without changing the original slots.",
        "distinct" => {
            "Return first occurrences in input order using T: Eq; no Hash bound is required."
        }
        "sort" => "Stably sort in place using T: Ord. Failure may leave a partial reordering.",
        "sort_by" => {
            "Stably sort in place using the comparator. Failure stops later callbacks; no rollback is promised."
        }
        "sort_by_key" => {
            "Stably sort in place with keys evaluated during comparisons. Failure does not promise rollback."
        }
        "reverse" => {
            "Reverse the list in place. Custom receiver failures may leave completed writes."
        }
        "retain" => {
            "Keep matching elements in input order. Failure preserves completed removals and callback effects."
        }
        "dedup" => {
            "Remove consecutive equal elements using T: Eq, preserving the first in each run. Failure preserves completed removals."
        }
        _ => unreachable!("declared list method"),
    }
}
