//! List defaults and Vec overrides share one algorithm inventory, not a semantic lowerer.
pub(super) mod algorithms;
use crate::{
    catalog::documentation::example,
    foundation::{
        bound, enumeration,
        enumerations::value::{self as enums, Entry, EnumSymbols},
        interface,
        lists::algorithms as lists,
        override_entry,
        vector::operations as vectors,
    },
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
                DefaultDefinition::new(declaration.implementation_with_send(move |view| {
                    let symbols =
                        EnumSymbols::new(view.catalog(), &[enums::OPTION, enums::ORDERING])?;
                    Ok(NativeBinding::new(
                        vec![
                            Codec::Value;
                            if matches!(method.callback, Callback::None) {
                                1
                            } else {
                                2
                            }
                        ],
                        Codec::Value,
                        move |cx| (method.entry)(&symbols, cx),
                    ))
                }))
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
            VectorBinding::Shared => enums::pending(
                if matches!(method.callback, Callback::None) {
                    1
                } else {
                    2
                },
                &[enums::OPTION, enums::ORDERING],
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

fn documentation(name: &str) -> String {
    let description = match name {
        "sorted" => {
            "Returns a new List in ascending `Ord` order.\n\nEqual elements keep their relative order. The original slots are unchanged;\nelements themselves are copied as values, so referenced objects remain shared."
        }
        "sorted_by" => {
            "Returns a new List ordered by `callback(left, right)`.\n\nThe callback returns Less, Equal or Greater and must define a consistent order.\nEqual elements keep their relative order. Original slots are unchanged, but\ncompleted callback effects survive a later trap."
        }
        "sorted_by_key" => {
            "Returns a new List ordered by the keys produced by `callback`.\n\nKeys are evaluated during comparisons, without caching. A callback can run\nmore than once for an element; it must yield a consistent ordering. Equal keys\nkeep their original element order. Original slots are unchanged."
        }
        "reversed" => {
            "Returns a new List in reverse input order.\n\nOriginal slots are unchanged. Empty and single-element lists retain their\ncontents. Referenced elements are shared rather than deeply cloned."
        }
        "distinct" => {
            "Returns the first occurrence of each equal element in input order.\n\nEquality uses `Eq`; no Hash bound is required. Unlike `dedup`, nonadjacent\nduplicates are removed too. Original slots are unchanged; callback effects\nfrom equality checks are not rolled back on failure."
        }
        "sort" => {
            "Sorts this list in ascending `Ord` order, returning Unit.\n\nEqual elements keep their relative order. Aliases observe the same storage.\nA failure can leave a partial reordering; completed effects are not rolled back."
        }
        "sort_by" => {
            "Sorts this list in place using `callback(left, right)`, returning Unit.\n\nThe callback returns Less, Equal or Greater and must define a consistent order.\nEqual elements retain their relative order. A trap stops later callbacks and\ncan leave a partial reordering. Vec rejects structural edits during comparison."
        }
        "sort_by_key" => {
            "Sorts this list in place by `callback` keys, returning Unit.\n\nKeys are evaluated during comparisons, without caching; callbacks can run\nrepeatedly for one element. Equal keys preserve element order. A callback trap\ncan leave a partial reordering. Vec rejects structural edits during comparison."
        }
        "reverse" => {
            "Reverses this list in place, returning Unit.\n\nAliases observe the same storage. Empty and single-element inputs are unchanged.\nIf a custom receiver fails during a write, earlier writes remain committed."
        }
        "retain" => {
            "Keeps elements for which `callback` returns true, returning Unit.\n\nVisits elements in input order and preserves the order of retained elements.\nA trap stops later callbacks and retains completed removals and callback effects.\nVec rejects structural edits from the predicate while retaining."
        }
        "dedup" => {
            "Removes consecutive equal elements, keeping the first in each run.\n\nReturns Unit and compares through `Eq`, without requiring Hash. Nonadjacent\nduplicates remain; use `distinct` to remove them in a new List. A failure\npreserves earlier removals and equality-callback effects."
        }
        _ => unreachable!("declared list method"),
    };
    let code = match name {
        "sorted" => {
            "fn main() -> bool {\n    val input = Vec::from([3, 1, 2]);\n    val result = input.sorted();\n    result.get(0u64) == Some(1) && result.get(2u64) == Some(3) && input.get(0u64) == Some(3)\n}"
        }
        "sorted_by" => {
            "fn main() -> bool {\n    val input = Vec::from([1, 3, 2]);\n    val result = input.sorted_by(|left, right| right.cmp(left));\n    result.get(0u64) == Some(3) && result.get(2u64) == Some(1) && input.get(0u64) == Some(1)\n}"
        }
        "sorted_by_key" => {
            "fn main() -> bool {\n    val input = Vec::from([1, 3, 2]);\n    val result = input.sorted_by_key(|value| -value);\n    result.get(0u64) == Some(3) && result.get(2u64) == Some(1) && input.get(0u64) == Some(1)\n}"
        }
        "reversed" => {
            "fn main() -> bool {\n    val input = Vec::from([1, 2, 3]);\n    val result = input.reversed();\n    result.get(0u64) == Some(3) && result.get(2u64) == Some(1) && input.get(0u64) == Some(1)\n}"
        }
        "distinct" => {
            "fn main() -> bool {\n    val input = Vec::from([2, 1, 2, 1, 3]);\n    val result = input.distinct();\n    result.len() == 3u64 && result.get(0u64) == Some(2) && result.get(1u64) == Some(1) && input.len() == 5u64\n}"
        }
        "sort" => {
            "fn main() -> bool {\n    val values = Vec::from([3, 1, 2]);\n    val alias = values;\n    values.sort();\n    alias.get(0u64) == Some(1) && alias.get(2u64) == Some(3)\n}"
        }
        "sort_by" => {
            "fn main() -> bool {\n    val values = Vec::from([1, 3, 2]);\n    values.sort_by(|left, right| right.cmp(left));\n    values.get(0u64) == Some(3) && values.get(2u64) == Some(1)\n}"
        }
        "sort_by_key" => {
            "fn main() -> bool {\n    val values = Vec::from([1, 3, 2]);\n    values.sort_by_key(|value| -value);\n    values.get(0u64) == Some(3) && values.get(2u64) == Some(1)\n}"
        }
        "reverse" => {
            "fn main() -> bool {\n    val values = Vec::from([1, 2, 3]);\n    values.reverse();\n    values.get(0u64) == Some(3) && values.get(2u64) == Some(1)\n}"
        }
        "retain" => {
            "fn main() -> bool {\n    val values = Vec::from([1, 2, 3, 4]);\n    values.retain(|value| value % 2 == 0);\n    values.len() == 2u64 && values.get(0u64) == Some(2) && values.get(1u64) == Some(4)\n}"
        }
        "dedup" => {
            "fn main() -> bool {\n    val values = Vec::from([1, 1, 2, 1]);\n    values.dedup();\n    values.len() == 3u64 && values.get(0u64) == Some(1) && values.get(1u64) == Some(2) && values.get(2u64) == Some(1)\n}"
        }
        _ => unreachable!("list operation example"),
    };
    example(description, code)
}
