//! Builtin Map/Set constructors and capabilities retain their existing storage model.
use crate::{
    bindings::{self, Entry, arrays, construction, hash},
    catalog::documentation,
    foundation::{arrays::array, conflict, interface, override_entry, raw},
};
use kagari_runtime::native::{
    assembly::{
        draft::{ModuleDraft, TraitImplementation},
        references::{DraftCallableRequirement, DraftMember, DraftReference},
    },
    binding::{Codec, NativeBinding, NativeResult},
    declarations::MethodDecl,
    types::Type,
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeTypeConstructor},
    ty::{GenericBound, Ty},
};

struct Keys {
    eq: DraftReference,
    hash: DraftReference,
    partial_eq: DraftReference,
    hash_call: DraftMember,
    eq_call: DraftMember,
}

impl Keys {
    fn new(module: &mut ModuleDraft<'_>) -> NativeResult<Self> {
        let eq = interface(module, "Eq", 0)?;
        let hash = interface(module, "Hash", 0)?;
        let partial_eq = interface(module, "PartialEq", 0)?;
        let hash_call = module.reference_method(&hash, "hash", 0)?;
        let eq_call = module.reference_method(&partial_eq, "eq", 0)?;
        Ok(Self {
            eq,
            hash,
            partial_eq,
            hash_call,
            eq_call,
        })
    }

    fn bounds(&self, key: &Type) -> NativeResult<Vec<GenericBound>> {
        let mut constraints = vec![];
        for protocol in [&self.eq, &self.hash, &self.partial_eq] {
            let Ty::Trait(applied) = protocol.apply([])?.abi().clone() else {
                unreachable!("trait application")
            };
            constraints.push(applied);
        }
        GenericBound::normalize(
            vec![GenericBound {
                ty: key.abi().clone(),
                constraints,
            }],
            key.definitions().expect("key binder scope"),
            &Default::default(),
        )
        .map_err(|error| conflict(format!("hash bounds: {error:?}")))
    }

    fn calls(&self, key: Type) -> NativeResult<Vec<DraftCallableRequirement>> {
        Ok(vec![
            self.hash_call
                .requirement(key.clone(), self.hash.apply([])?, [])?,
            self.eq_call
                .requirement(key, self.partial_eq.apply([])?, [])?,
        ])
    }
}

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let keys = Keys::new(module)?;
    for (map, name, constructor, arity) in [
        (true, "HashMap", NativeTypeConstructor::Map, 2),
        (false, "HashSet", NativeTypeConstructor::Set, 1),
    ] {
        let reference = module.define_type(
            name,
            TypeDefKind::Native(constructor),
            arity,
            |record, definitions| {
                record.bounds = keys.bounds(&Type::from_semantic(
                    record.generic_params[0].as_type(),
                    definitions,
                )?)?;
                Ok(())
            },
        )?;
        module.document(
            reference.id(),
            documentation::markdown(if map { MAP_DOCS } else { SET_DOCS }),
        )?;
        inherent(module, &keys, &reference, map)?;
        capabilities(module, &keys, &reference, map, arity)?;
        from_iterator(module, &keys, &reference, map, arity)?;
    }
    Ok(())
}

fn inherent(
    module: &mut ModuleDraft<'_>,
    keys: &Keys,
    reference: &DraftReference,
    map: bool,
) -> NativeResult<()> {
    module.inherent_generic(reference, |items, methods| {
        let receiver = methods.receiver();
        for protocol in [&keys.eq, &keys.hash, &keys.partial_eq] {
            methods.bound(items[0].clone(), protocol.apply([])?)?;
        }
        let calls = keys.calls(items[0].clone())?;
        methods.define(
            MethodDecl::static_method("new")
                .returns(receiver.clone())
                .documentation(inherent_docs(map, "new"))
                .configure(move |function| {
                    for call in calls {
                        function.requires_draft(call)?;
                    }
                    Ok(())
                })
                .implementation_send(NativeBinding::new(
                    vec![],
                    Codec::Value,
                    if map { hash::map_new } else { hash::set_new },
                )),
        )?;
        if !map {
            let calls = keys.calls(items[0].clone())?;
            methods.define(
                MethodDecl::static_method("from")
                    .parameter("array", array(items[0].clone())?)
                    .returns(receiver.clone())
                    .documentation(inherent_docs(map, "from"))
                    .configure(move |function| {
                        for call in calls {
                            function.requires_draft(call)?;
                        }
                        Ok(())
                    })
                    .implementation_send(NativeBinding::new(
                        vec![Codec::Value],
                        Codec::Value,
                        arrays::set,
                    )),
            )?;
        }
        let mut insert = MethodDecl::instance("insert")
            .parameter("arg1", items[0].clone())
            .returns(receiver.clone())
            .documentation(inherent_docs(map, "insert"));
        if map {
            insert = insert.parameter("arg2", items[1].clone());
        }
        let calls = keys.calls(items[0].clone())?;
        methods.define(
            insert
                .configure(move |function| {
                    for call in calls {
                        function.requires_draft(call)?;
                    }
                    Ok(())
                })
                .implementation_send(NativeBinding::new(
                    vec![Codec::Value; if map { 3 } else { 2 }],
                    Codec::Value,
                    if map {
                        hash::map_insert_fluent
                    } else {
                        hash::set_insert_fluent
                    },
                )),
        )?;
        methods.define(
            MethodDecl::instance("clear")
                .returns(receiver.clone())
                .documentation(inherent_docs(map, "clear"))
                .implementation_send(NativeBinding::new(
                    vec![Codec::Value],
                    Codec::Value,
                    if map {
                        hash::map_clear_fluent
                    } else {
                        hash::set_clear_fluent
                    },
                )),
        )?;
        Ok(())
    })
}

fn capabilities(
    module: &mut ModuleDraft<'_>,
    keys: &Keys,
    reference: &DraftReference,
    map: bool,
    arity: usize,
) -> NativeResult<()> {
    let read = interface(module, if map { "Map" } else { "Set" }, arity)?;
    let write = interface(module, if map { "MutableMap" } else { "MutableSet" }, arity)?;
    let iterable = interface(module, "Iterable", 0)?;
    let item = module.reference_associated_type(&iterable, "Item", 0)?;
    let cursor = module.reference_associated_type(&iterable, "Iter", 0)?;
    for readonly in [false, true] {
        let methods: &[(&str, usize, Entry, bool)] = if map {
            &[
                ("len", 1, hash::map_len, false),
                ("is_empty", 1, hash::map_is_empty, false),
                ("contains_key", 2, hash::map_contains, true),
                ("get", 2, hash::map_get, true),
            ]
        } else {
            &[
                ("len", 1, hash::set_len, false),
                ("is_empty", 1, hash::set_is_empty, false),
                ("contains", 2, hash::set_contains, true),
            ]
        };
        let mut entries = vec![];
        for &(name, count, callback, keyed) in methods {
            let mut entry = override_entry(module, &read, name, 0, raw(count, callback))?;
            if keyed {
                for call in keys.calls(read.parameter(0)?)? {
                    entry = entry.requires(call);
                }
            }
            entries.push(entry);
        }
        module.trait_implementation_with(arity, |items| {
            Ok(TraitImplementation {
                receiver: receiver(reference.apply(items.to_vec())?, readonly)?,
                interface: read.apply(items.to_vec())?,
                bounds: keys.bounds(&items[0])?,
                bindings: vec![],
            }
            .with_overrides(entries))
        })?;
        let entry = override_entry(module, &iterable, "iter", 0, raw(1, bindings::iter))?;
        module.trait_implementation_with(arity, |items| {
            let element = if map {
                Type::tuple(items.to_vec())?
            } else {
                items[0].clone()
            };
            Ok(TraitImplementation {
                receiver: receiver(reference.apply(items.to_vec())?, readonly)?,
                interface: cursor.bind(
                    item.bind(iterable.apply([])?, element.clone())?,
                    Type::iterator(element)?,
                )?,
                bounds: keys.bounds(&items[0])?,
                bindings: vec![],
            }
            .with_overrides([entry]))
        })?;
    }
    let methods: &[(&str, usize, Entry, bool)] = if map {
        &[
            ("insert", 3, hash::map_insert, true),
            ("remove", 2, hash::map_remove, true),
            ("clear", 1, hash::map_clear, false),
        ]
    } else {
        &[
            ("insert", 2, hash::set_insert, true),
            ("remove", 2, hash::set_remove, true),
            ("clear", 1, hash::set_clear, false),
        ]
    };
    let mut entries = vec![];
    for &(name, count, callback, keyed) in methods {
        let mut entry = override_entry(module, &write, name, 0, raw(count, callback))?;
        if keyed {
            for call in keys.calls(write.parameter(0)?)? {
                entry = entry.requires(call);
            }
        }
        entries.push(entry);
    }
    module.trait_implementation_with(arity, |items| {
        Ok(TraitImplementation {
            receiver: reference.apply(items.to_vec())?,
            interface: write.apply(items.to_vec())?,
            bounds: keys.bounds(&items[0])?,
            bindings: vec![],
        }
        .with_overrides(entries))
    })
}

fn receiver(receiver: Type, readonly: bool) -> NativeResult<Type> {
    if !readonly {
        return Ok(receiver);
    }
    Type::from_semantic(
        receiver.abi().read_only_view().expect("builtin map/set"),
        receiver.definitions().expect("generic scope"),
    )
}

fn from_iterator(
    module: &mut ModuleDraft<'_>,
    keys: &Keys,
    reference: &DraftReference,
    map: bool,
    arity: usize,
) -> NativeResult<()> {
    let from = interface(module, "FromIterator", 1)?;
    let member = module.reference_method(&from, "from_iter", 1)?;
    let source = member.parameter(0)?;
    let iterable = interface(module, "Iterable", 0)?;
    let item = module.reference_associated_type(&iterable, "Item", 0)?;
    let cursor = module.reference_associated_type(&iterable, "Iter", 0)?;
    let iter = module.reference_method(&iterable, "iter", 0)?;
    let iterator = interface(module, "Iterator", 0)?;
    let output = module.reference_associated_type(&iterator, "Item", 0)?;
    let next = module.reference_method(&iterator, "next", 0)?;
    let write = interface(module, if map { "MutableMap" } else { "MutableSet" }, arity)?;
    let insert = module.reference_method(&write, "insert", 0)?;
    let entry = override_entry(
        module,
        &from,
        "from_iter",
        1,
        raw(1, construction::hash_from_iter),
    )?;
    module.trait_implementation_with(arity, |items| {
        let receiver = reference.apply(items.to_vec())?;
        let element = if map {
            Type::tuple(items.to_vec())?
        } else {
            items[0].clone()
        };
        let applied = item.bind(iterable.apply([])?, element.clone())?;
        let source_cursor = cursor.projection(source.clone(), applied.clone(), [])?;
        let mut entry = entry;
        for call in keys.calls(items[0].clone())? {
            entry = entry.requires(call);
        }
        entry = entry
            .requires(insert.requirement(receiver.clone(), write.apply(items.to_vec())?, [])?)
            .requires(iter.requirement(source, applied, [])?)
            .requires(next.requirement(
                source_cursor,
                output.bind(iterator.apply([])?, element.clone())?,
                [],
            )?);
        Ok(TraitImplementation {
            receiver,
            interface: from.apply([element])?,
            bounds: keys.bounds(&items[0])?,
            bindings: vec![],
        }
        .with_overrides([entry]))
    })
}

const MAP_DOCS: (&str, &str) = (
    "A shared hash map with invariant key and value types. Equal keys must hash equally. Insert replaces an existing value; get returns None for an absent key.",
    "fn find(values: std::collections::HashMap<i32, String>) -> Option<String> { values.get(1) }",
);
const SET_DOCS: (&str, &str) = (
    "A shared hash set. Equal members occupy one entry, and must hash equally. Lookup and structural mutation use checked synchronous guards.",
    "fn has(values: std::collections::HashSet<i32>) -> bool { values.contains(1) }",
);

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Sequence, map and set interfaces, concrete hash collections and lazy mapping. Callbacks run synchronously with explicit borrow, root and mutation guards.",
    "fn length(values: Vec<i32>) -> u64 { values.len() }",
);

fn inherent_docs(map: bool, name: &str) -> String {
    let ty = if map {
        "std::collections::HashMap<i32, i32>"
    } else {
        "std::collections::HashSet<i32>"
    };
    let (description, arguments) = match name {
        "new" => ("Create fresh empty hash collection storage.", ""),
        "from" => (
            "Construct a hash set from an array, preserving uniqueness of equal members.",
            "[1, 2]",
        ),
        "insert" if map => (
            "Insert a key/value entry, replacing the value of an equal key, and return the same map.",
            "0, 1",
        ),
        "insert" => (
            "Insert a member, preserving uniqueness of equal members, and return the same set.",
            "1",
        ),
        "clear" => (
            "Remove all entries after validating structural mutation guards and return the same collection.",
            "",
        ),
        _ => unreachable!("hash collection inherent inventory"),
    };
    let code = if matches!(name, "new" | "from") {
        format!(
            "fn create() -> {ty} {{ {}::{name}({arguments}) }}",
            ty.split('<').next().unwrap()
        )
    } else {
        format!("fn example(values: {ty}) {{ val result = values.{name}({arguments}); }}")
    };
    documentation::example(description, &code)
}
