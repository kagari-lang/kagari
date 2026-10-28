//! Ordinary opaque hash collections own their declaration and implementation.
mod access;
pub(super) mod operations;
use crate::{
    catalog::documentation,
    collections::{
        hash::{
            HashPayload,
            cursor::{self, HashCursor},
        },
        map::MapStorage,
        set::SetStorage,
    },
    foundation::{
        arrays::array, conflict, enumerations::value as enums, hash::operations as hash, interface,
        override_entry,
    },
};
use kagari_runtime::native::{
    assembly::{
        authoring::PendingBinding,
        draft::{ModuleDraft, TraitImplementation},
        references::{DraftCallableRequirement, DraftMember, DraftReference},
    },
    binding::{Codec, NativeBinding, NativeResult},
    context::CallContext,
    conversion::{IntoKagari, arguments::FromKagariArguments},
    declarations::MethodDecl,
    storage::NativeStorage,
    typed::NativeContext,
    types::Type,
};
use kagari_runtime::value::Value;
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
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
    declare_collection::<MapStorage>(module, &keys, true, "HashMap", "HashMapCursor", 2)?;
    declare_collection::<SetStorage>(module, &keys, false, "HashSet", "HashSetCursor", 1)
}

fn declare_collection<P: HashPayload>(
    module: &mut ModuleDraft<'_>,
    keys: &Keys,
    map: bool,
    name: &str,
    cursor_name: &str,
    arity: usize,
) -> NativeResult<()> {
    let reference = module.define_type(
        name,
        TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
        arity,
        |record, definitions| {
            record.bounds = keys.bounds(&Type::from_semantic(
                record.generic_params[0].as_type(),
                definitions,
            )?)?;
            Ok(())
        },
    )?;
    module.bind_storage(&reference, NativeStorage::payload::<P>())?;
    let cursor = module.define_type(
        cursor_name,
        TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
        arity,
        |_, _| Ok(()),
    )?;
    module.bind_storage(&cursor, NativeStorage::payload::<HashCursor<P>>())?;
    module.document(
        reference.id(),
        documentation::markdown(if map { MAP_DOCS } else { SET_DOCS }),
    )?;
    inherent(module, keys, &reference, map)?;
    capabilities::<P>(module, keys, &reference, &cursor, map, arity)?;
    from_iterator(module, keys, &reference, map, arity)
}

fn typed<A: FromKagariArguments, R: IntoKagari>(
    entry: impl for<'a> Fn(&mut NativeContext<'a>, A) -> NativeResult<R> + Send + Sync + Clone + 'static,
) -> PendingBinding {
    PendingBinding::implementation_with_send(move |view| {
        Ok(NativeBinding::declared(view.declaration(), entry.clone()))
    })
}

fn raw(count: usize, entry: fn(&mut CallContext<'_>) -> NativeResult<Value>) -> NativeBinding {
    NativeBinding::new(vec![Codec::Value; count], Codec::Value, entry)
}

fn flat(entry: fn(&mut CallContext<'_>) -> NativeResult<Value>) -> PendingBinding {
    PendingBinding::implementation_with_send(move |view| {
        Ok(raw(
            view.declaration().records().function.params.len(),
            entry,
        ))
    })
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
                .implementation_with_send(move |view| {
                    Ok(if map {
                        NativeBinding::declared(view.declaration(), hash::map_new)
                    } else {
                        NativeBinding::declared(view.declaration(), hash::set_new)
                    })
                }),
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
                    .implementation_with_send(|view| {
                        Ok(NativeBinding::declared(
                            view.declaration(),
                            hash::from_array,
                        ))
                    }),
            )?;
        }
        let mut insert = MethodDecl::instance("insert")
            .parameter("arg1", items[0].clone())
            .returns(receiver.clone())
            .documentation(inherent_docs(map, "insert"));
        if map {
            insert = insert.parameter("arg2", items[1].clone());
        }
        methods.define(insert.implementation_with_send(move |view| {
            Ok(if map {
                raw(
                    view.declaration().records().function.params.len(),
                    access::map_insert_fluent,
                )
            } else {
                raw(
                    view.declaration().records().function.params.len(),
                    access::set_insert_fluent,
                )
            })
        }))?;
        methods.define(
            MethodDecl::instance("clear")
                .returns(receiver)
                .documentation(inherent_docs(map, "clear"))
                .implementation_with_send(move |view| {
                    Ok(if map {
                        raw(
                            view.declaration().records().function.params.len(),
                            access::clear_fluent::<MapStorage>,
                        )
                    } else {
                        raw(
                            view.declaration().records().function.params.len(),
                            access::clear_fluent::<SetStorage>,
                        )
                    })
                }),
        )?;
        Ok(())
    })
}

fn capabilities<P: HashPayload>(
    module: &mut ModuleDraft<'_>,
    keys: &Keys,
    reference: &DraftReference,
    cursor_type: &DraftReference,
    map: bool,
    arity: usize,
) -> NativeResult<()> {
    let read = interface(module, if map { "Map" } else { "Set" }, arity)?;
    let write = interface(module, if map { "MutableMap" } else { "MutableSet" }, arity)?;
    let iterable = interface(module, "Iterable", 0)?;
    let item = module.reference_associated_type(&iterable, "Item", 0)?;
    let cursor = module.reference_associated_type(&iterable, "Iter", 0)?;
    let mut entries = vec![
        override_entry(module, &read, "len", 0, flat(access::len::<P>))?,
        override_entry(module, &read, "is_empty", 0, flat(access::is_empty::<P>))?,
        override_entry(
            module,
            &read,
            if map { "contains_key" } else { "contains" },
            0,
            flat(access::contains::<P>),
        )?,
    ];
    if map {
        entries.push(override_entry(
            module,
            &read,
            "get",
            0,
            enums::pending_option(2, access::map_get),
        )?);
    }
    module.trait_implementation_with(arity, |items| {
        Ok(TraitImplementation {
            receiver: reference.apply(items.to_vec())?,
            interface: read.apply(items.to_vec())?,
            bounds: keys.bounds(&items[0])?,
            bindings: vec![],
        }
        .with_overrides(entries))
    })?;
    let entry = override_entry(module, &iterable, "iter", 0, typed(cursor::iter::<P>))?;
    module.trait_implementation_with(arity, |items| {
        let element = if map {
            Type::tuple(items.to_vec())?
        } else {
            items[0].clone()
        };
        Ok(TraitImplementation {
            receiver: reference.apply(items.to_vec())?,
            interface: cursor.bind(
                item.bind(iterable.apply([])?, element)?,
                cursor_type.apply(items.to_vec())?,
            )?,
            bounds: keys.bounds(&items[0])?,
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    let iterator = interface(module, "Iterator", 0)?;
    let output = module.reference_associated_type(&iterator, "Item", 0)?;
    let entry = override_entry(
        module,
        &iterator,
        "next",
        0,
        if map {
            enums::pending_option(1, hash::map_next)
        } else {
            enums::pending_option(1, hash::set_next)
        },
    )?;
    module.trait_implementation_with(arity, |items| {
        let element = if map {
            Type::tuple(items.to_vec())?
        } else {
            items[0].clone()
        };
        Ok(TraitImplementation {
            receiver: cursor_type.apply(items.to_vec())?,
            interface: output.bind(iterator.apply([])?, element)?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    let methods = if map {
        vec![
            ("insert", flat(access::map_insert)),
            ("remove", flat(access::map_remove)),
            ("clear", flat(access::clear::<MapStorage>)),
        ]
    } else {
        vec![
            ("insert", flat(access::set_insert)),
            ("remove", flat(access::set_remove)),
            ("clear", flat(access::clear::<SetStorage>)),
        ]
    };
    let entries = methods
        .into_iter()
        .map(|(name, binding)| override_entry(module, &write, name, 0, binding))
        .collect::<NativeResult<Vec<_>>>()?;
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
        if map {
            typed(hash::map_from_iter)
        } else {
            typed(hash::set_from_iter)
        },
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
