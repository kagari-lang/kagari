//! Lazy cursor owners and authored Iterable defaults use the runtime's shared lowerer.
pub(super) mod cursor;
pub(super) mod operations;
pub(super) mod traversal;
use crate::{
    catalog::documentation,
    foundation::{
        bound, enumeration,
        enumerations::value::{self as enums, EnumSymbols},
        interface,
        iteration::{
            cursor::{self as cursors, Operation},
            operations::{self as iterable, Terminal},
        },
        override_entry, raw,
    },
    identity,
};
use kagari_common::identity::DefinitionKind;
use kagari_runtime::native::{
    assembly::{
        authoring::PendingBinding,
        draft::{ModuleDraft, TraitImplementation, defaults::DefaultDefinition},
        references::{DraftCallableRequirement, DraftMember, DraftReference},
    },
    binding::{Codec, NativeBinding, NativeResult},
    declarations::MethodDecl,
    types::Type,
};
use kagari_types::{
    declaration::{
        TypeDefKind,
        native::{NativeStorageLayout, NativeTypeConstructor},
    },
    ty::GenericBound,
};

pub(super) const ADAPTERS: &[(&str, usize, Operation)] = &[
    ("MapIterator", 2, Operation::Map),
    ("FilterIterator", 1, Operation::Filter),
    ("TakeIterator", 1, Operation::Take),
    ("FilterMapIterator", 2, Operation::FilterMap),
    ("SkipIterator", 1, Operation::Skip),
    ("EnumerateIterator", 1, Operation::Enumerate),
    ("ZipIterator", 2, Operation::Zip),
    ("ChainIterator", 1, Operation::Chain),
];

struct Iteration {
    iterable: DraftReference,
    item: DraftMember,
    cursor: DraftMember,
    iter: DraftMember,
    iterator: DraftReference,
    output: DraftMember,
    next: DraftMember,
}

impl Iteration {
    fn new(module: &mut ModuleDraft<'_>) -> NativeResult<Self> {
        let iterable = interface(module, "Iterable", 0)?;
        let item = module.reference_associated_type(&iterable, "Item", 0)?;
        let cursor = module.reference_associated_type(&iterable, "Iter", 0)?;
        let iter = module.reference_method(&iterable, "iter", 0)?;
        let iterator = interface(module, "Iterator", 0)?;
        let output = module.reference_associated_type(&iterator, "Item", 0)?;
        let next = module.reference_method(&iterator, "next", 0)?;
        Ok(Self {
            iterable,
            item,
            cursor,
            iter,
            iterator,
            output,
            next,
        })
    }

    fn source(&self, receiver: Type, item: Type, cursor: Type) -> NativeResult<Source> {
        let iterable = self.cursor.bind(
            self.item.bind(self.iterable.apply([])?, item.clone())?,
            cursor.clone(),
        )?;
        let iterator = self.output.bind(self.iterator.apply([])?, item)?;
        Ok(Source {
            assumptions: [
                bound(&receiver, iterable.clone()),
                bound(&cursor, iterator.clone()),
            ],
            requirements: [
                self.iter.requirement(receiver, iterable, [])?,
                self.next.requirement(cursor, iterator, [])?,
            ],
        })
    }
}

struct Source {
    assumptions: [GenericBound; 2],
    requirements: [DraftCallableRequirement; 2],
}

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let iteration = Iteration::new(module)?;
    let cursor = module.define_constructor("CollectionCursor", NativeTypeConstructor::Iter)?;
    module.document(cursor.id(), documentation::markdown(TYPE_DOCS))?;
    let entry = override_entry(
        module,
        &iteration.iterator,
        "next",
        0,
        enums::pending_option(1, iterable::next),
    )?;
    module.trait_implementation_with(1, |items| {
        Ok(TraitImplementation {
            receiver: cursor.apply(items.to_vec())?,
            interface: iteration
                .output
                .bind(iteration.iterator.apply([])?, items[0].clone())?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    for &(name, arity, operation) in ADAPTERS {
        let adapter = module.define_type(
            name,
            TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
            arity,
            |_, _| Ok(()),
        )?;
        module.bind_storage(&adapter, cursors::storage())?;
        module.document(adapter.id(), documentation::markdown(ADAPTER_DOCS))?;
        let entry = override_entry(module, &iteration.iterator, "next", 0, PendingBinding::implementation_with_send(|view| Ok(cursors::next_binding(EnumSymbols::new(view.catalog(), &[enums::OPTION])?))))?
            .documentation("Advance the retained source lazily. Recursive advancement of this cursor traps; completed source and callback effects survive failure.");
        module.trait_implementation_with(arity, |items| {
            let item = match operation {
                Operation::Enumerate => Type::tuple([Type::u64(), items[0].clone()])?,
                Operation::Zip => Type::tuple(items.to_vec())?,
                _ => items[arity - 1].clone(),
            };
            Ok(TraitImplementation {
                receiver: adapter.apply(items.to_vec())?,
                interface: iteration.output.bind(iteration.iterator.apply([])?, item)?,
                bounds: vec![],
                bindings: vec![],
            }
            .with_overrides([entry]))
        })?;
    }
    defaults(module, &iteration)?;
    let entry = override_entry(
        module,
        &iteration.iterable,
        "iter",
        0,
        raw(1, iterable::identity),
    )?;
    module.trait_implementation_with(1, |items| {
        let receiver = iteration
            .output
            .bind(iteration.iterator.apply([])?, items[0].clone())?;
        Ok(TraitImplementation {
            receiver: receiver.clone(),
            interface: iteration.cursor.bind(
                iteration
                    .item
                    .bind(iteration.iterable.apply([])?, items[0].clone())?,
                receiver,
            )?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })
}

fn defaults(module: &mut ModuleDraft<'_>, iteration: &Iteration) -> NativeResult<()> {
    let receiver = iteration.iterable.receiver()?;
    let item = iteration
        .item
        .projection(receiver.clone(), iteration.iterable.apply([])?, [])?;
    let cursor =
        iteration
            .cursor
            .projection(receiver.clone(), iteration.iterable.apply([])?, [])?;
    let option = enumeration(module, "Option", 1)?;
    for &(adapter_name, adapter_arity, operation) in ADAPTERS {
        let (name, doc) = adapter_docs(operation);
        let generic = matches!(
            operation,
            Operation::Map | Operation::FilterMap | Operation::Zip | Operation::Chain
        );
        let method = module.reference_method(&iteration.iterable, name, usize::from(generic))?;
        let parameter = if generic {
            Some(method.parameter(0)?)
        } else {
            None
        };
        let paired = matches!(operation, Operation::Zip | Operation::Chain);
        let output = match operation {
            Operation::Map | Operation::FilterMap => parameter.clone().expect("output parameter"),
            Operation::Zip => iteration.item.projection(
                parameter.clone().expect("right parameter"),
                iteration.iterable.apply([])?,
                [],
            )?,
            _ => item.clone(),
        };
        let adapter = module.reference_type(
            identity::type_id(adapter_name, DefinitionKind::AssociatedType),
            TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
            adapter_arity,
        )?;
        let result = adapter.apply(if adapter_arity == 2 {
            vec![item.clone(), output.clone()]
        } else {
            vec![item.clone()]
        })?;
        let mut declaration = MethodDecl::instance(name)
            .documentation(doc)
            .returns(result);
        declaration = match operation {
            Operation::Map => {
                declaration.parameter("callback", Type::function([item.clone()], output.clone())?)
            }
            Operation::Filter => {
                declaration.parameter("callback", Type::function([item.clone()], Type::bool())?)
            }
            Operation::Take => declaration.parameter("limit", Type::u64()),
            Operation::FilterMap => declaration.parameter(
                "callback",
                Type::function([item.clone()], option.apply([output.clone()])?)?,
            ),
            Operation::Skip => declaration.parameter("count", Type::u64()),
            Operation::Enumerate => declaration,
            Operation::Zip | Operation::Chain => {
                declaration.parameter("other", parameter.clone().expect("right parameter"))
            }
        };
        let mut arguments = vec![receiver.clone(), item.clone()];
        arguments.extend(parameter.clone());
        if matches!(operation, Operation::Zip) {
            arguments.push(output.clone());
        }
        arguments.push(cursor.clone());
        let source = iteration.source(receiver.clone(), item.clone(), cursor.clone())?;
        let mut assumptions = source.assumptions.to_vec();
        let mut requirements = source.requirements.to_vec();
        let mut bounds = vec![];
        if paired {
            let other = parameter.expect("right parameter");
            let interface = if matches!(operation, Operation::Chain) {
                iteration
                    .item
                    .bind(iteration.iterable.apply([])?, item.clone())?
            } else {
                iteration.iterable.apply([])?
            };
            let other_cursor = iteration
                .cursor
                .projection(other.clone(), interface.clone(), [])?;
            bounds.push(bound(&other, interface));
            arguments.push(other_cursor.clone());
            let source = iteration.source(other, output, other_cursor)?;
            assumptions.extend(source.assumptions);
            requirements.extend(source.requirements);
        }
        module.define_trait_default(
            &iteration.iterable,
            DefaultDefinition::new(
                declaration
                    .configure(move |function| {
                        if generic {
                            function.type_parameter(if paired { "R" } else { "U" })?;
                        }
                        for requirement in requirements {
                            function.requires_draft(requirement)?;
                        }
                        Ok(())
                    })
                    .implementation_send(cursors::adapter_binding(operation)),
            )
            .arguments(arguments)
            .bounds(bounds)
            .assumptions(assumptions),
        )?;
    }
    for (name, protocol_name, target) in [
        ("collect", "FromIterator", "from_iter"),
        ("sum", "Sum", "sum"),
        ("product", "Product", "product"),
    ] {
        let method = module.reference_method(&iteration.iterable, name, 1)?;
        let destination = method.parameter(0)?;
        let protocol = interface(module, protocol_name, 1)?;
        let target_method = module.reference_method(&protocol, target, 1)?;
        let applied = protocol.apply([item.clone()])?;
        let requirement =
            target_method.requirement(destination.clone(), applied.clone(), [receiver.clone()])?;
        module.define_trait_default(&iteration.iterable, DefaultDefinition::new(MethodDecl::instance(name)
            .returns(destination.clone())
            .documentation(format!("Consume through the requested destination's {target} implementation; preserve its checked construction and arithmetic semantics."))
            .configure(move |function| { function.type_parameter("D")?; function.requires_draft(requirement)?; Ok(()) })
            .implementation_send(NativeBinding::new(vec![Codec::Value], Codec::Value, iterable::destination)))
            .arguments([receiver.clone(), item.clone(), destination.clone()])
            .bounds([bound(&destination, applied)])
            .assumptions([bound(&receiver, iteration.item.bind(iteration.iterable.apply([])?, item.clone())?)]))?;
    }
    let fold = module.reference_method(&iteration.iterable, "fold", 1)?;
    let accumulator = fold.parameter(0)?;
    let source = iteration.source(receiver.clone(), item.clone(), cursor.clone())?;
    module.define_trait_default(&iteration.iterable, DefaultDefinition::new(MethodDecl::instance("fold")
        .parameter("initial", accumulator.clone())
        .parameter("callback", Type::function([accumulator.clone(), item.clone()], accumulator.clone())?)
        .returns(accumulator.clone())
        .documentation("Consume in source order, passing the accumulator and each item to the callback. Empty input returns initial; completed source/callback effects survive failure.")
        .configure(move |function| { function.type_parameter("A")?; for requirement in source.requirements { function.requires_draft(requirement)?; } Ok(()) })
        .implementation_with_send(|view| { let symbols = EnumSymbols::new(view.catalog(), &[enums::OPTION])?; Ok(NativeBinding::new(vec![Codec::Value; 3], Codec::Value, move |cx| iterable::fold(&symbols, cx))) }))
        .arguments([receiver.clone(), item.clone(), accumulator, cursor.clone()]).assumptions(source.assumptions))?;
    for (name, terminal, doc) in [
        (
            "count",
            Terminal::Count,
            "Consume until the first None and return the checked u64 count.",
        ),
        (
            "find",
            Terminal::Find,
            "Return the first matching item, stopping immediately; empty input returns None.",
        ),
        (
            "any",
            Terminal::Any,
            "Stop at the first true predicate; empty input returns false.",
        ),
        (
            "all",
            Terminal::All,
            "Stop at the first false predicate; empty input returns true.",
        ),
        (
            "for_each",
            Terminal::ForEach,
            "Run the action once per item in source order and return unit.",
        ),
    ] {
        let result = match terminal {
            Terminal::Count => Type::u64(),
            Terminal::Find => option.apply([item.clone()])?,
            Terminal::ForEach => Type::unit(),
            _ => Type::bool(),
        };
        let mut declaration = MethodDecl::instance(name)
            .returns(result)
            .documentation(doc);
        if !matches!(terminal, Terminal::Count) {
            declaration = declaration.parameter(
                "callback",
                Type::function(
                    [item.clone()],
                    if matches!(terminal, Terminal::ForEach) {
                        Type::unit()
                    } else {
                        Type::bool()
                    },
                )?,
            );
        }
        let source = iteration.source(receiver.clone(), item.clone(), cursor.clone())?;
        module.define_trait_default(
            &iteration.iterable,
            DefaultDefinition::new(
                declaration
                    .configure(move |function| {
                        for requirement in source.requirements {
                            function.requires_draft(requirement)?;
                        }
                        Ok(())
                    })
                    .implementation_with_send(move |view| {
                        Ok(iterable::terminal_binding(
                            terminal,
                            EnumSymbols::new(view.catalog(), &[enums::OPTION])?,
                        ))
                    }),
            )
            .arguments([receiver.clone(), item.clone(), cursor.clone()])
            .assumptions(source.assumptions),
        )?;
    }
    Ok(())
}

fn adapter_docs(operation: Operation) -> (&'static str, &'static str) {
    match operation {
        Operation::Map => (
            "map",
            "Lazily transform each element with the callback. Obtain the source cursor once; copies share progress.",
        ),
        Operation::Filter => (
            "filter",
            "Lazily return elements accepted by the predicate, preserving source order and callback effects.",
        ),
        Operation::Take => (
            "take",
            "Lazily return at most limit elements. A zero limit never advances the source.",
        ),
        Operation::FilterMap => (
            "filter_map",
            "Lazily return the callback's Some values in source order, without fusing the source.",
        ),
        Operation::Skip => (
            "skip",
            "Discard only the initial count items; aliases share the remaining skip state.",
        ),
        Operation::Enumerate => (
            "enumerate",
            "Lazily pair each item with its checked u64 position, starting at zero.",
        ),
        Operation::Zip => (
            "zip",
            "Lazily pull left then right and return pairs. A left item stays consumed if right returns None.",
        ),
        Operation::Chain => (
            "chain",
            "Lazily traverse the first cursor, then switch permanently to the second on its first None.",
        ),
    }
}

const TYPE_DOCS: (&str, &str) = (
    "A shared built-in cursor. Copies share progress. next returns None when exhausted; structural source edits invalidate subsequent reads.",
    "fn first(values: Vec<i32>) -> Option<i32> { values.iter().next() }",
);

const ADAPTER_DOCS: (&str, &str) = (
    "A lazy shared cursor. Retains its sources, any callbacks and selected next targets; aliases share progress and original generations. Recursive next on the same cursor traps.",
    "fn first(values: Vec<i32>) -> Option<i32> { values.map(|x| x + 1).filter(|x| x > 0).take(1u64).next() }",
);

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Shared iteration and iterable construction. Iterator values retain progress and reject stale source reads after structural mutation.",
    "fn total<I: Iterable<Item = i32>>(values: I) -> i32 { values.filter(|item| item > 0).sum() }",
);
