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
    declaration::{AssociatedTypeDef, FnDecl, conversion::ConversionAdapter},
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
        "cmp",
        &[
            (
                "PartialEq",
                0,
                (
                    "Equality comparison. Implementations should be symmetric and transitive where defined. Equal keys must also have equal hashes when used in a hash collection.",
                    "fn same(left: i32, right: i32) -> bool { left == right }",
                ),
            ),
            (
                "Eq",
                0,
                (
                    "Marks reflexive equality in addition to `PartialEq`. It has no additional methods. Implementations must satisfy the equality laws relied on by collection algorithms.",
                    "fn unique(values: Vec<i32>) -> std::collections::List<i32> { values.distinct() }",
                ),
            ),
            (
                "PartialOrd",
                0,
                (
                    "A partial ordering consistent with equality. `partial_cmp` returns `None` for incomparable values; floating-point NaN is incomparable.",
                    "fn before(left: i32, right: i32) -> bool { left < right }",
                ),
            ),
            (
                "Ord",
                0,
                (
                    "A total ordering consistent with `Eq` and `PartialOrd`. `cmp` returns Less, Equal or Greater. Sorting relies on a consistent comparator.",
                    "fn ordered(values: Vec<i32>) -> std::collections::List<i32> { values.sorted() }",
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
        "ops",
        &[
            (
                "Add",
                1,
                (
                    "Addition with an explicitly declared output type. Built-in integer arithmetic traps on overflow; a user implementation defines its own behavior.",
                    "fn plus(left: i32, right: i32) -> i32 { left + right }",
                ),
            ),
            (
                "Sub",
                1,
                (
                    "Subtraction with an explicitly declared output type. Built-in integer subtraction traps on overflow.",
                    "fn difference(left: i32, right: i32) -> i32 { left - right }",
                ),
            ),
            (
                "Mul",
                1,
                (
                    "Multiplication with an explicitly declared output type. Built-in integer multiplication traps on overflow.",
                    "fn doubled(value: i32) -> i32 { value * 2 }",
                ),
            ),
            (
                "Div",
                1,
                (
                    "Division. Built-in integer division traps for zero divisors and signed overflow; floating-point division follows its specified numeric semantics.",
                    "fn half(value: i32) -> i32 { value / 2 }",
                ),
            ),
            (
                "Rem",
                1,
                (
                    "The remainder of division. Built-in integer remainder traps for zero divisors and signed overflow.",
                    "fn parity(value: i32) -> i32 { value % 2 }",
                ),
            ),
            (
                "BitAnd",
                1,
                (
                    "Bitwise conjunction, with an explicitly declared output type.",
                    "fn low_bits(value: i32) -> i32 { value & 15 }",
                ),
            ),
            (
                "BitOr",
                1,
                (
                    "Bitwise disjunction, with an explicitly declared output type.",
                    "fn set_bit(value: i32) -> i32 { value | 1 }",
                ),
            ),
            (
                "BitXor",
                1,
                (
                    "Bitwise exclusive disjunction, with an explicitly declared output type.",
                    "fn toggle(value: i32) -> i32 { value ^ 1 }",
                ),
            ),
            (
                "Shl",
                1,
                (
                    "Left shift. Built-in integer shifts validate the shift count and checked result.",
                    "fn shift(value: i32) -> i32 { value << 1 }",
                ),
            ),
            (
                "Shr",
                1,
                (
                    "Right shift. Built-in integer shifts validate the shift count.",
                    "fn shift(value: i32) -> i32 { value >> 1 }",
                ),
            ),
            (
                "Neg",
                0,
                (
                    "Arithmetic negation. Negating the minimum signed integer traps on overflow.",
                    "fn negative(value: i32) -> i32 { -value }",
                ),
            ),
            (
                "Not",
                0,
                (
                    "Logical or bitwise inversion, depending on the receiver type. The output is declared by the implementation.",
                    "fn invert(value: bool) -> bool { !value }",
                ),
            ),
            (
                "Index",
                1,
                (
                    "Indexed read access. Built-in sequence indexing traps when the index is out of bounds; `get` provides an optional result instead.",
                    "fn head(values: Vec<i32>) -> i32 { values[0] }",
                ),
            ),
            (
                "Fn",
                1,
                (
                    "A checked callable signature. Calls evaluate arguments left to right exactly once. Stored callable values retain their selected generation across reload.",
                    "fn apply<F: Fn(i32) -> i32>(f: F, value: i32) -> i32 { f(value) }",
                ),
            ),
            (
                "RangeBounds",
                1,
                (
                    "Exposes the start and end bounds of a range as Included, Excluded or Unbounded. Constructing bounds does not iterate the range.",
                    "fn bounds() -> core::ops::Bound<i32> { (1..4).start_bound() }",
                ),
            ),
            (
                "Try",
                0,
                (
                    "Defines the successful Output and distinct Residual of a carrier. Question-mark evaluates the value and branch once: Continue produces Output, while Break passes Residual to the enclosing return type's FromResidual implementation. Traps are not converted to residuals.",
                    "fn unwrap(value: Option<i32>) -> Option<i32> { Some(value?) }",
                ),
            ),
            (
                "FromResidual",
                1,
                (
                    "Constructs the enclosing return value on early propagation. Residual types distinguish Option, Result and ControlFlow. Result converts only the error through one infallible From implementation, preserving its original failure trace.",
                    "fn forward(value: Result<i32, String>) -> Result<i64, String> { Ok(i64::from(value?)) }",
                ),
            ),
        ],
    ),
    (
        "core",
        "convert",
        &[
            (
                "From",
                1,
                (
                    "Constructs a target value from a source. The conversion is infallible in its declared signature. Result propagation can use `From` to convert an error type.",
                    "fn widen(value: i32) -> i64 { i64::from(value) }",
                ),
            ),
            (
                "Into",
                1,
                (
                    "Converts the receiver into a target. A matching `From` implementation supplies the reverse adapter without another body.",
                    "fn widen(value: i32) -> i64 { value.into() }",
                ),
            ),
            (
                "TryFrom",
                1,
                (
                    "A checked conversion returning `Result<Self, Error>`. An unrepresentable numeric value returns an error; it does not silently truncate.",
                    "fn narrow(value: i64) -> Result<i32, core::num::TryFromIntError> { i32::try_from(value) }",
                ),
            ),
            (
                "TryInto",
                1,
                (
                    "A checked conversion of the receiver into a target. The reverse adapter uses the corresponding `TryFrom` declaration and error type.",
                    "fn narrow(value: i64) -> Result<i32, core::num::TryFromIntError> { value.try_into() }",
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
        "core",
        "default",
        &[(
            "Default",
            0,
            (
                "Constructs an implementation-defined value through the required static `default() -> Self` method, with no receiver or arguments. The canonical trait is `core::default::Default`; the prelude and `std::default::Default` expose the same declaration. Implementations are explicit: this contract neither zeroes storage nor recursively initializes fields, and user structs receive no automatic implementation. It is static-only and cannot be used as an interface value type. Calls use ordinary trait selection, including concrete, qualified and generic bound forms; a trait-qualified call needs result context to determine Self, not merely an available implementation.\n\n# Standard implementations\n\nUnit returns (); bool returns false. The eight integer types i8, i16, i32, i64, u8, u16, u32 and u64 return correctly typed zero; f32 and f64 return positive zero. String returns an empty string. Vec<T> returns fresh empty mutable storage on every call; Option<T> returns None. Neither container requires T: Default. Tuples, builtin arrays, maps, sets, ranges, Result, other enums and host/scoped values have no standard implementation.",
                "struct Player { val health: i32 }\nimpl Default for Player { fn default() -> Self { Player { health: 100 } } }\nfn player() -> Player { Player::default() }\nfn qualified() -> Player { <Player as core::default::Default>::default() }\nfn make<T: Default>() -> T { T::default() }\nstruct Element {}\nfn empty_vec() -> Vec<Element> { Vec<Element>::default() }\nfn absent() -> Option<Element> { Option<Element>::default() }\nfn zero() -> i32 { Default::default() }\nfn empty_string() -> String { String::default() }",
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
    conversion: Option<ConversionAdapter>,
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
            conversion: None,
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
            record.conversion_adapter = self.conversion;
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
                "From" | "Into" | "TryFrom" | "TryInto" | "FromStr" | "Default"
                | "FromIterator" | "Sum" | "Product" => {
                    conversion::configure(module, &mut contract)?
                }
                "Iterator" | "Iterable" => iteration(module, &mut contract)?,
                "Try" | "FromResidual" => propagation(module, &mut contract)?,
                "RangeBounds" => {
                    let result =
                        enumeration(module, "Bound", 1)?.apply([contract.owner.parameter(0)?])?;
                    for (name, description) in [
                        (
                            "start_bound",
                            "Return the lower bound with its inclusion policy.",
                        ),
                        (
                            "end_bound",
                            "Return the upper bound with its inclusion policy.",
                        ),
                    ] {
                        contract.method(
                            module,
                            MethodDecl::instance(name).returns(result.clone()),
                            description,
                        )?;
                    }
                }
                "PartialEq" | "Eq" | "PartialOrd" | "Ord" | "Hash" | "Debug" | "Display" => {
                    comparison(module, &mut contract)?
                }
                _ => operator(module, &mut contract)?,
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

fn comparison(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    let receiver = contract.owner.receiver()?;
    let (parents, method, binary, result) = match contract.name {
        "PartialEq" => (&[][..], "eq", true, Type::bool()),
        "Eq" => (&["PartialEq"][..], "", false, Type::unit()),
        "Hash" => (&[][..], "hash", false, Type::i64()),
        "Debug" => (&[][..], "debug", false, Type::scalar(BuiltinType::String)),
        "Display" => (&[][..], "display", false, Type::scalar(BuiltinType::String)),
        "PartialOrd" => {
            let ordering = enumeration(module, "Ordering", 0)?.apply([])?;
            (
                &["PartialEq"][..],
                "partial_cmp",
                true,
                enumeration(module, "Option", 1)?.apply([ordering])?,
            )
        }
        "Ord" => (
            &["Eq", "PartialOrd"][..],
            "cmp",
            true,
            enumeration(module, "Ordering", 0)?.apply([])?,
        ),
        _ => unreachable!("comparison trait"),
    };
    for name in parents {
        contract.parent(interface(module, name, 0)?.apply([])?);
    }
    if !method.is_empty() {
        let mut declaration = MethodDecl::instance(method).returns(result);
        if binary {
            declaration = declaration.parameter("arg1", receiver);
        }
        contract.method(module, declaration, contract.overview.0)?;
    }
    Ok(())
}

fn operator(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    let member = match contract.name {
        "Add" => "add",
        "Sub" => "sub",
        "Mul" => "mul",
        "Div" => "div",
        "Rem" => "rem",
        "BitAnd" => "bitand",
        "BitOr" => "bitor",
        "BitXor" => "bitxor",
        "Shl" => "shl",
        "Shr" => "shr",
        "Neg" => "neg",
        "Not" => "not",
        "Index" => "index",
        "Fn" => "call",
        _ => unreachable!("operator trait"),
    };
    let output = contract.associated(
        module,
        "Output",
        vec![],
        "The result type of the declared operation.",
    )?;
    let mut declaration = MethodDecl::instance(member).returns(contract.projection(&output)?);
    if contract.arity == 1 {
        declaration = declaration.parameter("arg1", contract.owner.parameter(0)?);
    }
    contract.method(module, declaration, contract.overview.0)
}

fn propagation(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    if contract.name == "FromResidual" {
        return contract.method(
            module,
            MethodDecl::static_method("from_residual")
                .parameter("residual", contract.owner.parameter(0)?)
                .returns(contract.owner.receiver()?),
            "Construct the enclosing early-return value from a compatible Residual.",
        );
    }
    let output = contract.associated(
        module,
        "Output",
        vec![],
        "The result type of the declared operation.",
    )?;
    let residual = contract.associated(module, "Residual", vec![],
"The distinct early-return type passed to FromResidual; successful Output values are not residuals.",
)?;
    let output = contract.projection(&output)?;
    let residual = contract.projection(&residual)?;
    contract.parent(interface(module, "FromResidual", 1)?.apply([residual.clone()])?);
    contract.method(
        module,
        MethodDecl::static_method("from_output")
            .parameter("output", output.clone())
            .returns(contract.owner.receiver()?),
        "Construct a successful carrier from Output.",
    )?;
    let result = enumeration(module, "ControlFlow", 2)?.apply([residual, output])?;
    contract.method(
        module,
        MethodDecl::instance("branch").returns(result),
        "Consume the carrier once and return Continue(Output) or Break(Residual).",
    )
}

pub(super) const CMP_MODULE_DOCS: (&str, &str) = (
    "Equality and ordering contracts. Comparisons must obey the laws required by collection algorithms.",
    "fn same(left: i32, right: i32) -> bool { left == right }",
);

pub(super) const HASH_MODULE_DOCS: (&str, &str) = (
    "Hash contracts used by key-based collections. Equal keys must hash equally, and key lookup blocks recursive structural mutation.",
    "fn lookup(values: std::collections::HashMap<i32, i32>) -> Option<i32> { values.get(7) }",
);

pub(super) const FMT_MODULE_DOCS: (&str, &str) = (
    "Diagnostic and display formatting through declared interfaces.",
    "fn render<T: core::fmt::Debug>(value: T) -> String { value.debug() }",
);

pub(super) const CONVERT_MODULE_DOCS: (&str, &str) = (
    "Explicit infallible and checked conversions, including reverse adapters and error conversion.",
    "fn widen(value: i32) -> i64 { i64::from(value) }",
);

pub(super) const DEFAULT_MODULE_DOCS: (&str, &str) = (
    "Explicit implementation-defined construction through the ordinary Default trait. No automatic struct, field or enum initialization is implied.",
    "struct Player { val health: i32 }\nimpl Default for Player { fn default() -> Self { Player { health: 100 } } }\nfn player() -> Player { Player::default() }\nfn qualified() -> Player { <Player as core::default::Default>::default() }\nfn make<T: Default>() -> T { T::default() }\nstruct Element {}\nfn empty_vec() -> Vec<Element> { Vec<Element>::default() }\nfn absent() -> Option<Element> { Option<Element>::default() }\nfn zero() -> i32 { Default::default() }\nfn empty_string() -> String { String::default() }",
);

pub(super) const STR_MODULE_DOCS: (&str, &str) = (
    "String parsing contracts with an explicit error type and complete input validation.",
    "fn parse(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
);
