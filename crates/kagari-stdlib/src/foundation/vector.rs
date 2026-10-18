//! Vec owns sequence storage, fluent inherent methods and concrete protocol overrides.
use crate::{
    bindings::{self, arrays, construction, vectors},
    catalog::documentation,
    foundation::{arrays::array, interface, lists, override_entry, raw},
};
use kagari_runtime::native::{
    assembly::{
        authoring::PendingBinding,
        draft::{ModuleDraft, TraitImplementation},
    },
    binding::{Codec, NativeBinding, NativeResult},
    declarations::MethodDecl,
    primitive::NativePrimitive,
    storage::NativeStorage,
    types::Type,
};
use kagari_types::declaration::{TypeDefKind, native::NativeStorageLayout};

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let vector = module.define_type(
        "Vec",
        TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
        1,
        |_, _| Ok(()),
    )?;
    module.bind_storage(&vector, NativeStorage::sequence(0))?;
    module.document(vector.id(), documentation::markdown(TYPE_DOCS))?;
    module.inherent_with(
        1,
        |items| vector.apply(items.to_vec()),
        |items, methods| {
            let receiver = methods.receiver();
            methods.define(
                MethodDecl::static_method("new")
                    .returns(receiver.clone())
                    .documentation(inherent_docs("new"))
                    .implementation_send(NativeBinding::new(
                        vec![],
                        Codec::Value,
                        bindings::list_new,
                    )),
            )?;
            methods.define(
                MethodDecl::static_method("from")
                    .parameter("array", array(items[0].clone())?)
                    .returns(receiver.clone())
                    .documentation(inherent_docs("from"))
                    .implementation_send(NativeBinding::new(
                        vec![Codec::Value],
                        Codec::Value,
                        arrays::vector,
                    )),
            )?;
            methods.define(
                MethodDecl::instance("push")
                    .parameter("arg1", items[0].clone())
                    .returns(receiver.clone())
                    .documentation(inherent_docs("push"))
                    .implementation_send(scoped(
                        vec![Codec::MutableSequence, Codec::Value],
                        vectors::push_fluent,
                    )),
            )?;
            methods.define(
                MethodDecl::instance("insert")
                    .parameter("arg1", Type::u64())
                    .parameter("arg2", items[0].clone())
                    .returns(receiver.clone())
                    .documentation(inherent_docs("insert"))
                    .implementation_send(scoped(
                        vec![
                            Codec::MutableSequence,
                            Codec::Exact(Type::u64()),
                            Codec::Value,
                        ],
                        vectors::insert_fluent,
                    )),
            )?;
            methods.define(
                MethodDecl::instance("clear")
                    .returns(receiver.clone())
                    .documentation(inherent_docs("clear"))
                    .implementation_with_send(|view| Ok(vectors::clear_fluent(view.declaration()))),
            )?;
            methods.define(
                MethodDecl::instance("set")
                    .parameter("arg1", Type::u64())
                    .parameter("arg2", items[0].clone())
                    .returns(receiver.clone())
                    .documentation(inherent_docs("set"))
                    .implementation_send(NativeBinding::primitive(NativePrimitive::VecSetFluent)),
            )?;
            Ok(())
        },
    )?;
    let list = interface(module, "List", 1)?;
    let mut entries = lists::overrides(module, &list, false)?;
    for (name, binding) in [
        ("len", scoped(vec![Codec::Sequence], vectors::length)),
        ("is_empty", scoped(vec![Codec::Sequence], vectors::is_empty)),
        (
            "get",
            scoped(
                vec![Codec::Sequence, Codec::Exact(Type::u64())],
                vectors::get,
            ),
        ),
    ] {
        entries.push(override_entry(
            module,
            &list,
            name,
            0,
            PendingBinding::implementation_send(binding),
        )?);
    }
    module.trait_implementation_with(1, |items| {
        Ok(TraitImplementation {
            receiver: vector.apply(items.to_vec())?,
            interface: list.apply(items.to_vec())?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides(entries))
    })?;
    let mutable = interface(module, "MutableList", 1)?;
    let mut entries = lists::overrides(module, &mutable, true)?;
    for (name, binding) in [
        (
            "push",
            PendingBinding::implementation_send(scoped(
                vec![Codec::MutableSequence, Codec::Value],
                vectors::push,
            )),
        ),
        ("pop", raw(1, bindings::list_pop)),
        (
            "insert",
            PendingBinding::implementation_send(scoped(
                vec![
                    Codec::MutableSequence,
                    Codec::Exact(Type::u64()),
                    Codec::Value,
                ],
                vectors::insert,
            )),
        ),
        ("remove", raw(2, bindings::list_remove)),
        (
            "clear",
            PendingBinding::implementation_with_send(|view| Ok(vectors::clear(view.declaration()))),
        ),
        (
            "set",
            PendingBinding::implementation_send(NativeBinding::primitive(NativePrimitive::VecSet)),
        ),
    ] {
        entries.push(override_entry(module, &mutable, name, 0, binding)?);
    }
    module.trait_implementation_with(1, |items| {
        Ok(TraitImplementation {
            receiver: vector.apply(items.to_vec())?,
            interface: mutable.apply(items.to_vec())?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides(entries))
    })?;
    let index = interface(module, "Index", 1)?;
    let output = module.reference_associated_type(&index, "Output", 0)?;
    let entry = override_entry(
        module,
        &index,
        "index",
        0,
        PendingBinding::implementation_send(NativeBinding::primitive(NativePrimitive::VecIndex)),
    )?;
    module.trait_implementation_with(1, |items| {
        Ok(TraitImplementation {
            receiver: vector.apply(items.to_vec())?,
            interface: output.bind(index.apply([Type::u64()])?, items[0].clone())?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    let iterable = interface(module, "Iterable", 0)?;
    let item = module.reference_associated_type(&iterable, "Item", 0)?;
    let cursor = module.reference_associated_type(&iterable, "Iter", 0)?;
    let entry = override_entry(module, &iterable, "iter", 0, raw(1, bindings::iter))?;
    module.trait_implementation_with(1, |items| {
        Ok(TraitImplementation {
            receiver: vector.apply(items.to_vec())?,
            interface: cursor.bind(
                item.bind(iterable.apply([])?, items[0].clone())?,
                Type::iterator(items[0].clone())?,
            )?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    let default = interface(module, "Default", 0)?;
    let entry = override_entry(module, &default, "default", 0, raw(0, bindings::list_new))?.documentation("Return a fresh empty Vec<T>. Every call creates independent mutable storage; T need not implement Default.\n\nSee `core::default::Default` for the construction contract.");
    module.trait_implementation_with(1, |items| {
        Ok(TraitImplementation {
            receiver: vector.apply(items.to_vec())?,
            interface: default.apply([])?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    let from = interface(module, "FromIterator", 1)?;
    let from_iter = module.reference_method(&from, "from_iter", 1)?;
    let source = from_iter.parameter(0)?;
    let applied = item.bind(iterable.apply([])?, from.parameter(0)?)?;
    let source_cursor = cursor.projection(source.clone(), applied.clone(), [])?;
    let iter = module.reference_method(&iterable, "iter", 0)?;
    let iterator = interface(module, "Iterator", 0)?;
    let iterator_item = module.reference_associated_type(&iterator, "Item", 0)?;
    let next = module.reference_method(&iterator, "next", 0)?;
    let entry = override_entry(
        module,
        &from,
        "from_iter",
        1,
        raw(1, construction::list_from_iter),
    )?
    .requires(iter.requirement(source, applied, [])?)
    .requires(next.requirement(
        source_cursor,
        iterator_item.bind(iterator.apply([])?, from.parameter(0)?)?,
        [],
    )?);
    module.trait_implementation_with(1, |items| {
        Ok(TraitImplementation {
            receiver: vector.apply(items.to_vec())?,
            interface: from.apply(items.to_vec())?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;

    Ok(())
}

fn scoped(arguments: Vec<Codec>, entry: bindings::Entry) -> NativeBinding {
    NativeBinding::new(arguments, Codec::Value, entry)
}

const TYPE_DOCS: (&str, &str) = (
    "A shared mutable sequence. Copies retain the same storage identity. Indexing traps out of bounds; get returns None. Structural mutation honors active borrows and invalidates existing cursors.",
    "fn append(values: Vec<i32>) -> u64 { values.push(3); values.len() }",
);

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Shared mutable sequences with checked indexing, iteration and collection algorithms. Readonly views do not freeze aliases.",
    "fn length(values: Vec<i32>) -> u64 { values.len() }",
);

fn inherent_docs(name: &str) -> String {
    let (description, code) = match name {
        "new" => (
            "Create fresh empty mutable sequence storage.",
            "fn create() -> Vec<i32> { Vec::new() }",
        ),
        "from" => (
            "Copy an array into fresh mutable sequence storage.",
            "fn create() -> Vec<i32> { Vec::from([1, 2]) }",
        ),
        "push" => (
            "Append one item and return the same Vec. A guarded structural edit traps before committing the edit.",
            "fn example(values: Vec<i32>) { val result = values.push(1); }",
        ),
        "insert" => (
            "Insert at the given position and return the same Vec. An index beyond the length traps.",
            "fn example(values: Vec<i32>) { val result = values.insert(0, 1); }",
        ),
        "clear" => (
            "Remove all entries after validating structural mutation guards and return the same Vec.",
            "fn example(values: Vec<i32>) { val result = values.clear(); }",
        ),
        "set" => (
            "Replace the item at an existing index and return the same Vec. An out-of-bounds index traps.",
            "fn example(values: Vec<i32>) { val result = values.set(0, 1); }",
        ),
        _ => unreachable!("Vec inherent inventory"),
    };
    documentation::example(description, code)
}
