//! Canonical standard trait schemas; required methods use ordinary MethodDecl lowering.
mod collections;
mod conversion;
use crate::{
    catalog::documentation,
    foundation::{enumeration, interface},
    namespaces,
};
use kagari_common::identity::{DefinitionKind, table::DefinitionId};

use kagari_runtime::native::{
    assembly::{
        draft::ModuleDraft,
        references::{DraftMember, DraftReference},
    },
    binding::NativeResult,
    declarations::MethodDecl,
    types::Type,
};
use kagari_types::{
    declaration::{AssociatedTypeDef, FnDecl},
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

type TraitInventory = (&'static str, usize, (&'static str, &'static str));
type ModuleInventory = (&'static str, &'static str, &'static [TraitInventory]);

pub(super) const INVENTORY: &[ModuleInventory] = &[
    (
        "core",
        "iter",
        &[
            (
                "Iterator",
                0,
                (
                    "A shared cursor. `next` advances once and returns `Some(item)`, or `None` to stop the current consumption. A custom cursor can resume after None. Copies share progress; previously completed effects survive a failure.",
                    "fn first(values: Vec<i32>) -> Option<i32> { values.iter().next() }",
                ),
            ),
            (
                "Iterable",
                0,
                (
                    "Produces an iterator with the same Item and supplies the finite lazy transformation and terminal defaults. Iterator identity adaptation shares this API and cursor progress. Creating an adapter does not consume an item or run a callback. Structural source edits commit and invalidate subsequent cursor reads.",
                    "fn total<I: Iterable<Item = i32>>(values: I) -> i32 { values.filter(|item| item > 0).sum() }",
                ),
            ),
            (
                "FromIterator",
                1,
                (
                    "Constructs a value by consuming an iterable in order. Each item is read once. A failure retains already completed source and callback effects.",
                    "fn collect(values: Vec<i32>) -> Vec<i32> { Vec<i32>::from_iter(values) }",
                ),
            ),
            (
                "Sum",
                1,
                (
                    "Aggregates an iterable using addition. Built-in numeric sums use the additive identity for an empty input and checked arithmetic where required.",
                    "fn total(values: Vec<i32>) -> i32 { i32::sum(values) }",
                ),
            ),
            (
                "Product",
                1,
                (
                    "Aggregates an iterable using multiplication. Built-in numeric products use the multiplicative identity for an empty input and checked arithmetic where required.",
                    "fn multiply(values: Vec<i32>) -> i32 { i32::product(values) }",
                ),
            ),
        ],
    ),
    (
        "core",
        "hash",
        &[(
            "Hash",
            0,
            (
                "Computes a stable hash for the duration of a collection operation. Equal keys must hash equally. Hash and equality callbacks run synchronously; recursive structural mutation during lookup traps.",
                "fn lookup(values: std::collections::HashMap<i32, i32>) -> Option<i32> { values.get(7) }",
            ),
        )],
    ),
    (
        "core",
        "fmt",
        &[
            (
                "Debug",
                0,
                (
                    "Produces a diagnostic string representation through `debug`. Formatting does not permit reflective access outside declared members.",
                    "fn render<T: core::fmt::Debug>(value: T) -> String { value.debug() }",
                ),
            ),
            (
                "Display",
                0,
                (
                    "Produces a user-facing string representation through `display`. Implementations choose their formatting policy.",
                    "fn render<T: core::fmt::Display>(value: T) -> String { value.display() }",
                ),
            ),
        ],
    ),
    (
        "core",
        "str",
        &[(
            "FromStr",
            0,
            (
                "Parses a string into a value, returning the declared `Err` on invalid input. Built-in numeric parsing validates the complete input and representable range.",
                "fn parse(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
            ),
        )],
    ),
    (
        "std",
        "collections",
        &[
            (
                "List",
                1,
                (
                    "A readonly sequence interface. Element types are invariant. A readonly view retains shared identity, so another alias can mutate the underlying sequence.",
                    "fn first(values: std::collections::List<i32>) -> Option<i32> { values.get(0) }",
                ),
            ),
            (
                "MutableList",
                1,
                (
                    "A mutable sequence interface. Structural edits validate active borrows and invalidate existing cursors. Completed writes and callback effects survive later failures.",
                    "fn append(values: std::collections::MutableList<i32>) { values.push(3); }",
                ),
            ),
            (
                "Map",
                2,
                (
                    "A readonly key/value interface. Lookup returns `None` for a missing key. Readonly views share storage with other aliases.",
                    "fn find(values: std::collections::Map<i32, String>) -> Option<String> { values.get(1) }",
                ),
            ),
            (
                "MutableMap",
                2,
                (
                    "A mutable key/value interface. Insert replaces the value of an equal key. Hash and equality callbacks must not structurally mutate a collection under lookup.",
                    "fn put(values: std::collections::MutableMap<i32, String>) { values.insert(1, \"one\"); }",
                ),
            ),
            (
                "Set",
                1,
                (
                    "A readonly membership interface. Equal values represent the same member. Readonly views share storage with other aliases.",
                    "fn has(values: std::collections::Set<i32>) -> bool { values.contains(1) }",
                ),
            ),
            (
                "MutableSet",
                1,
                (
                    "A mutable membership interface. Insertion retains one representative of each equal value. Structural edits honor active borrow and lookup guards.",
                    "fn add(values: std::collections::MutableSet<i32>) { values.insert(1); }",
                ),
            ),
        ],
    ),
];

struct Contract {
    name: &'static str,
    arity: usize,
    overview: (&'static str, &'static str),
    documents: Vec<(DefinitionId, String)>,
    owner: DraftReference,
    methods: Vec<FnDecl>,
    associated: Vec<AssociatedTypeDef>,
    supertraits: Vec<NominalTy>,
}

impl Contract {
    fn new(
        module: &mut ModuleDraft<'_>,
        name: &'static str,
        arity: usize,
        overview: (&'static str, &'static str),
    ) -> NativeResult<Self> {
        Ok(Self {
            name,
            arity,
            overview,
            documents: vec![],
            owner: interface(module, name, arity)?,
            methods: vec![],
            associated: vec![],
            supertraits: vec![],
        })
    }

    fn method(
        &mut self,
        module: &ModuleDraft<'_>,
        declaration: MethodDecl,
        description: &str,
    ) -> NativeResult<()> {
        let declaration = module.trait_method(self.owner.id(), declaration)?;
        self.submit(module, declaration, description)
    }

    fn submit(
        &mut self,
        module: &ModuleDraft<'_>,
        declaration: FnDecl,
        description: &str,
    ) -> NativeResult<()> {
        let id = module.member(self.owner.id(), DefinitionKind::Method, &declaration.name)?;
        let text = documentation::example(
            &format!(
                "{description}\n\nSee `{}` for the behavioral contract and examples.",
                self.name
            ),
            self.overview.1,
        );
        self.documents.push((id, text));
        self.methods.push(declaration);
        Ok(())
    }

    fn associated(
        &mut self,
        module: &mut ModuleDraft<'_>,
        name: &str,
        bounds: Vec<NominalTy>,
        description: &str,
    ) -> NativeResult<DraftMember> {
        let member = module.reference_associated_type(&self.owner, name, 0)?;
        self.documents.push((member.id(), description.into()));
        self.associated.push(AssociatedTypeDef {
            declaration: member.id(),
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds,
        });
        Ok(member)
    }

    fn applied(&self) -> NativeResult<Type> {
        self.owner.apply(
            (0..self.arity)
                .map(|index| self.owner.parameter(index))
                .collect::<NativeResult<Vec<_>>>()?,
        )
    }

    fn projection(&self, member: &DraftMember) -> NativeResult<Type> {
        member.projection(self.owner.receiver()?, self.applied()?, [])
    }

    fn parent(&mut self, interface: Type) {
        self.supertraits.push(nominal(interface));
    }

    fn finish(self, module: &mut ModuleDraft<'_>) -> NativeResult<()> {
        module.document(self.owner.id(), documentation::markdown(self.overview))?;
        for (id, text) in self.documents {
            module.document(id, text)?;
        }
        module.define_trait(self.name, self.arity, |record, _| {
            record.methods = self.methods;
            record.associated_types = self.associated;
            record.supertraits = self.supertraits;
            Ok(())
        })?;
        Ok(())
    }
}

fn nominal(interface: Type) -> NominalTy {
    let Ty::Trait(interface) = interface.abi().clone() else {
        unreachable!("trait application")
    };
    interface
}

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    for &(package, path, declarations) in INVENTORY {
        if module.identity() != &namespaces::module(package, path) {
            continue;
        }
        for &(name, arity, overview) in declarations {
            let mut contract = Contract::new(module, name, arity, overview)?;
            match name {
                "List" | "MutableList" | "Map" | "MutableMap" | "Set" | "MutableSet" => {
                    collections::configure(module, &mut contract)?
                }
                "FromStr" => conversion::from_str(module, &mut contract)?,
                "FromIterator" | "Sum" | "Product" => {
                    conversion::aggregation(module, &mut contract)?
                }
                "Iterator" | "Iterable" => iteration(module, &mut contract)?,
                "Hash" | "Debug" | "Display" => {
                    let (method, result) = match name {
                        "Hash" => ("hash", Type::i64()),
                        "Debug" => ("debug", Type::scalar(BuiltinType::String)),
                        _ => ("display", Type::scalar(BuiltinType::String)),
                    };
                    contract.method(
                        module,
                        MethodDecl::instance(method).returns(result),
                        overview.0,
                    )?;
                }
                _ => unreachable!("unknown native trait recipe"),
            }
            contract.finish(module)?;
        }
    }
    Ok(())
}

fn iteration(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    let item_member = contract.associated(
        module,
        "Item",
        vec![],
        "The item yielded by this iterator or iterable.",
    )?;
    let item = contract.projection(&item_member)?;
    if contract.name == "Iterator" {
        let result = enumeration(module, "Option", 1)?.apply([item])?;
        contract.method(
            module,
            MethodDecl::instance("next").returns(result),
            contract.overview.0,
        )?;
    } else {
        let iterator = interface(module, "Iterator", 0)?;
        let output = module.reference_associated_type(&iterator, "Item", 0)?;
        let cursor = contract.associated(
            module,
            "Iter",
            vec![nominal(output.bind(iterator.apply([])?, item)?)],
            "The iterator produced by iter; its Item equals this iterable's Item.",
        )?;
        contract.method(
            module,
            MethodDecl::instance("iter").returns(contract.projection(&cursor)?),
            contract.overview.0,
        )?;
    }
    Ok(())
}

pub(super) const HASH_MODULE_DOCS: (&str, &str) = (
    "Hash contracts used by key-based collections. Equal keys must hash equally, and key lookup blocks recursive structural mutation.",
    "fn lookup(values: std::collections::HashMap<i32, i32>) -> Option<i32> { values.get(7) }",
);

pub(super) const FMT_MODULE_DOCS: (&str, &str) = (
    "Diagnostic and display formatting through declared interfaces.",
    "fn render<T: core::fmt::Debug>(value: T) -> String { value.debug() }",
);

pub(super) const STR_MODULE_DOCS: (&str, &str) = (
    "String parsing contracts with an explicit error type and complete input validation.",
    "fn parse(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
);
