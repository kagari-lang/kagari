//! Canonical standard trait schemas; required methods use ordinary MethodDecl lowering.
mod collections;
mod conversion;
mod documentation;
use crate::{
    catalog::documentation::markdown,
    foundation::{enumeration, interface, iteration::documentation as iteration_docs},
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
            ("Iterator", 0, iteration_docs::ITERATOR),
            ("Iterable", 0, iteration_docs::ITERABLE),
            ("FromIterator", 1, iteration_docs::FROM_ITERATOR),
            ("Sum", 1, iteration_docs::SUM),
            ("Product", 1, iteration_docs::PRODUCT),
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
                    "fn main() -> bool {\n    7 == 7 && !(7 == 8)\n}",
                ),
            ),
            (
                "Eq",
                0,
                (
                    "Marks reflexive equality in addition to `PartialEq`. It has no additional methods. Implementations must satisfy the equality laws relied on by collection algorithms.",
                    "fn main() -> bool {\n    val result = Vec::from([2, 1, 2, 1, 3]).distinct();\n    result.len() == 3u64 && result.get(0u64) == Some(2) && result.get(1u64) == Some(1)\n}",
                ),
            ),
            (
                "PartialOrd",
                0,
                (
                    "A partial ordering consistent with equality. `partial_cmp` returns `None` for incomparable values; floating-point NaN is incomparable.",
                    "fn main() -> bool {\n    2 < 3 && !(3 < 2) && !(2 < 2)\n}",
                ),
            ),
            (
                "Ord",
                0,
                (
                    "A total ordering consistent with `Eq` and `PartialOrd`. `cmp` returns Less, Equal or Greater. Sorting relies on a consistent comparator.",
                    "fn main() -> bool {\n    val input = Vec::from([3, 1, 2]);\n    val result = input.sorted();\n    result.get(0u64) == Some(1) && result.get(2u64) == Some(3) && input.get(0u64) == Some(3)\n}",
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
                "fn main() -> bool {\n    val values: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();\n    values.insert(7, 42);\n    values.get(7) == Some(42) && values.get(8) == None\n}",
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
                    "fn main() -> bool {\n    42.debug() == \"42\" && true.debug() == \"true\" && 'é'.debug() == \"'é'\"\n}",
                ),
            ),
            (
                "Display",
                0,
                (
                    "Produces a user-facing string representation through `display`. Implementations choose their formatting policy.",
                    "fn main() -> bool {\n    42.display() == \"42\" && 'é'.display() == \"é\"\n}",
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
                    "fn main() -> bool {\n    2 + 3 == 5 && -2 + 3 == 1\n}",
                ),
            ),
            (
                "Sub",
                1,
                (
                    "Subtraction with an explicitly declared output type. Built-in integer subtraction traps on overflow.",
                    "fn main() -> bool {\n    5 - 3 == 2 && 3 - 5 == -2\n}",
                ),
            ),
            (
                "Mul",
                1,
                (
                    "Multiplication with an explicitly declared output type. Built-in integer multiplication traps on overflow.",
                    "fn main() -> bool {\n    3 * 4 == 12 && -3 * 4 == -12\n}",
                ),
            ),
            (
                "Div",
                1,
                (
                    "Division. Built-in integer division traps for zero divisors and signed overflow; floating-point division follows its specified numeric semantics.",
                    "fn main() -> bool {\n    7 / 2 == 3 && -7 / 2 == -3\n}",
                ),
            ),
            (
                "Rem",
                1,
                (
                    "The remainder of division. Built-in integer remainder traps for zero divisors and signed overflow.",
                    "fn main() -> bool {\n    7 % 3 == 1 && -7 % 3 == -1\n}",
                ),
            ),
            (
                "BitAnd",
                1,
                (
                    "Bitwise conjunction, with an explicitly declared output type.",
                    "fn main() -> bool {\n    0b1010 & 0b1100 == 0b1000\n}",
                ),
            ),
            (
                "BitOr",
                1,
                (
                    "Bitwise disjunction, with an explicitly declared output type.",
                    "fn main() -> bool {\n    0b1010 | 0b0001 == 0b1011\n}",
                ),
            ),
            (
                "BitXor",
                1,
                (
                    "Bitwise exclusive disjunction, with an explicitly declared output type.",
                    "fn main() -> bool {\n    0b1010 ^ 0b0011 == 0b1001\n}",
                ),
            ),
            (
                "Shl",
                1,
                (
                    "Left shift. Built-in integer shifts validate the shift count and checked result.",
                    "fn main() -> bool {\n    3 << 2u32 == 12\n}",
                ),
            ),
            (
                "Shr",
                1,
                (
                    "Right shift. Built-in integer shifts validate the shift count.",
                    "fn main() -> bool {\n    12 >> 2u32 == 3\n}",
                ),
            ),
            (
                "Neg",
                0,
                (
                    "Arithmetic negation. Negating the minimum signed integer traps on overflow.",
                    "fn main() -> bool {\n    val value = 7;\n    -value == -7\n}",
                ),
            ),
            (
                "Not",
                0,
                (
                    "Logical or bitwise inversion, depending on the receiver type. The output is declared by the implementation.",
                    "fn main() -> bool {\n    !false && !!true\n}",
                ),
            ),
            (
                "Index",
                1,
                (
                    "Indexed read access. Built-in sequence indexing traps when the index is out of bounds; `get` provides an optional result instead.",
                    "fn main() -> bool {\n    val values = Vec::from([4, 5]);\n    values[0u64] == 4 && values[1u64] == 5\n}",
                ),
            ),
            (
                "Fn",
                1,
                (
                    "A checked callable signature. Calls evaluate arguments left to right exactly once. Stored callable values retain their selected generation across reload.",
                    "fn apply<F: Fn(i32) -> i32>(f: F, value: i32) -> i32 {\n    f(value)\n}\nfn main() -> bool {\n    apply(|value| value * 2, 21) == 42\n}",
                ),
            ),
            (
                "RangeBounds",
                1,
                (
                    "Exposes the start and end bounds of a range as Included, Excluded or Unbounded. Constructing bounds does not iterate the range.",
                    "use core::ops::Bound;\nfn main() -> bool {\n    (1..4).start_bound() == Bound::Included(1) &&\n        (1..4).end_bound() == Bound::Excluded(4)\n}",
                ),
            ),
            (
                "Try",
                0,
                (
                    "Defines the successful Output and distinct Residual of a carrier. Question-mark evaluates the value and branch once: Continue produces Output, while Break passes Residual to the enclosing return type's FromResidual implementation. Traps are not converted to residuals.",
                    "fn doubled(value: Option<i32>) -> Option<i32> {\n    Some(value? * 2)\n}\nfn main() -> bool {\n    doubled(Some(21)) == Some(42) && doubled(None) == None\n}",
                ),
            ),
            (
                "FromResidual",
                1,
                (
                    "Constructs the enclosing return value on early propagation. Residual types distinguish Option, Result and ControlFlow. Result converts only the error through one infallible From implementation, preserving its original failure trace.",
                    "fn widen(value: Result<i32, String>) -> Result<i64, String> {\n    Ok(i64::from(value?))\n}\nfn main() -> bool {\n    widen(Ok(42)) == Ok(42i64) && widen(Err(\"missing\")) == Err(\"missing\")\n}",
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
                    "fn main() -> bool {\n    i64::from(42i32) == 42i64 && i64::from(-7i32) == -7i64\n}",
                ),
            ),
            (
                "Into",
                1,
                (
                    "Converts the receiver into a target. A matching `From` implementation supplies the reverse adapter without another body.",
                    "fn main() -> bool {\n    val result: i64 = 42i32.into();\n    result == 42i64\n}",
                ),
            ),
            (
                "TryFrom",
                1,
                (
                    "A checked conversion returning `Result<Self, Error>`. An unrepresentable numeric value returns an error; it does not silently truncate.",
                    "use core::num::TryFromIntError;\nfn main() -> bool {\n    (match i32::try_from(42i64) { Ok(value) => value == 42i32, Err(_) => false, }) &&\n        (match i32::try_from(2147483648i64) { Err(TryFromIntError::OutOfRange) => true, _ => false, })\n}",
                ),
            ),
            (
                "TryInto",
                1,
                (
                    "A checked conversion of the receiver into a target. The reverse adapter uses the corresponding `TryFrom` declaration and error type.",
                    "use core::num::TryFromIntError;\n\nfn main() -> bool {\n    val success: Result<i32, TryFromIntError> = 42i64.try_into();\n    val failure: Result<i32, TryFromIntError> = 2147483648i64.try_into();\n    (\n        match success {\n            Ok(value) => value == 42,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match failure {\n            Err(TryFromIntError::OutOfRange) => true,\n            _ => false,\n        }\n    )\n}",
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
                "use core::num::ParseError;\n\nfn main() -> bool {\n    (\n        match i32::from_str(\"42\") {\n            Ok(value) => value == 42,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"\") {\n            Err(ParseError::Empty) => true,\n            _ => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"12x\") {\n            Err(ParseError::InvalidDigit) => true,\n            _ => false,\n        }\n    )\n}",
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
                "struct Player { val health: i32 }\nimpl Default for Player {\n    fn default() -> Self { Player { health: 100 } }\n}\nfn main() -> bool {\n    Player::default().health == 100 &&\n        <Player as core::default::Default>::default().health == 100 &&\n        i32::default() == 0 && String::default() == \"\"\n}",
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
                    "fn main() -> bool {\n    val values: std::collections::List<i32> = Vec::from([10, 20]);\n    values.get(0u64) == Some(10) && values.get(2u64) == None\n}",
                ),
            ),
            (
                "MutableList",
                1,
                (
                    "A mutable sequence interface. Structural edits validate active borrows and invalidate existing cursors. Completed writes and callback effects survive later failures.",
                    "fn main() -> bool {\n    val storage = Vec::from([1, 2]);\n    val view: std::collections::MutableList<i32> = storage;\n    view.push(3);\n    storage.get(2u64) == Some(3) && view.len() == 3u64\n}",
                ),
            ),
            (
                "Map",
                2,
                (
                    "A readonly key/value interface. Lookup returns `None` for a missing key. Readonly views share storage with other aliases.",
                    "fn main() -> bool {\n    val storage: std::collections::HashMap<i32, String> = std::collections::HashMap::new();\n    storage.insert(1, \"one\");\n    val view: std::collections::Map<i32, String> = storage;\n    view.get(1) == Some(\"one\") && view.get(2) == None\n}",
                ),
            ),
            (
                "MutableMap",
                2,
                (
                    "A mutable key/value interface. Insert replaces the value of an equal key. Hash and equality callbacks must not structurally mutate a collection under lookup.",
                    "fn main() -> bool {\n    val storage: std::collections::HashMap<i32, String> = std::collections::HashMap::new();\n    val view: std::collections::MutableMap<i32, String> = storage;\n    view.insert(1, \"first\");\n    view.insert(1, \"replacement\");\n    view.len() == 1u64 && view.get(1) == Some(\"replacement\")\n}",
                ),
            ),
            (
                "Set",
                1,
                (
                    "A readonly membership interface. Equal values represent the same member. Readonly views share storage with other aliases.",
                    "fn main() -> bool {\n    val values: std::collections::Set<i32> = std::collections::HashSet::from([1, 1, 2]);\n    values.len() == 2u64 && values.contains(1) && !values.contains(3)\n}",
                ),
            ),
            (
                "MutableSet",
                1,
                (
                    "A mutable membership interface. Insertion retains one representative of each equal value. Structural edits honor active borrow and lookup guards.",
                    "fn main() -> bool {\n    val storage: std::collections::HashSet<i32> = std::collections::HashSet::new();\n    val view: std::collections::MutableSet<i32> = storage;\n    view.insert(7);\n    view.insert(7);\n    view.len() == 1u64 && view.contains(7)\n}",
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
        let text =
            documentation::method(self.name, &declaration.name, description, self.overview.1);
        self.submit_documented(module, declaration, text)
    }

    fn documented_method(
        &mut self,
        module: &ModuleDraft<'_>,
        declaration: MethodDecl,
        document: (&str, &str),
    ) -> NativeResult<()> {
        let declaration = module.trait_method(self.owner.id(), declaration)?;
        self.submit_documented(module, declaration, markdown(document))
    }

    fn submit_documented(
        &mut self,
        module: &ModuleDraft<'_>,
        declaration: FnDecl,
        document: String,
    ) -> NativeResult<()> {
        let id = module.member(self.owner.id(), DefinitionKind::Method, &declaration.name)?;
        self.documents.push((id, document));
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
        module.document(self.owner.id(), markdown(self.overview))?;
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
        contract.documented_method(
            module,
            MethodDecl::instance("next").returns(result),
            iteration_docs::NEXT,
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
        contract.documented_method(
            module,
            MethodDecl::instance("iter").returns(contract.projection(&cursor)?),
            iteration_docs::ITER,
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
    "fn main() -> bool {\n    7 == 7 && !(7 == 8)\n}",
);

pub(super) const HASH_MODULE_DOCS: (&str, &str) = (
    "Hash contracts used by key-based collections. Equal keys must hash equally, and key lookup blocks recursive structural mutation.",
    "fn main() -> bool {\n    val values: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();\n    values.insert(7, 42);\n    values.get(7) == Some(42) && values.get(8) == None\n}",
);

pub(super) const FMT_MODULE_DOCS: (&str, &str) = (
    "Diagnostic and display formatting through declared interfaces.",
    "fn main() -> bool {\n    42.debug() == \"42\" && true.debug() == \"true\" && 'é'.debug() == \"'é'\"\n}",
);

pub(super) const CONVERT_MODULE_DOCS: (&str, &str) = (
    "Explicit infallible and checked conversions, including reverse adapters and error conversion.",
    "fn main() -> bool {\n    i64::from(42i32) == 42i64 && i64::from(-7i32) == -7i64\n}",
);

pub(super) const DEFAULT_MODULE_DOCS: (&str, &str) = (
    "Explicit implementation-defined construction through the ordinary Default trait. No automatic struct, field or enum initialization is implied.",
    "struct Player { val health: i32 }\nimpl Default for Player {\n    fn default() -> Self { Player { health: 100 } }\n}\nfn main() -> bool {\n    Player::default().health == 100 &&\n        <Player as core::default::Default>::default().health == 100 &&\n        i32::default() == 0 && String::default() == \"\"\n}",
);

pub(super) const STR_MODULE_DOCS: (&str, &str) = (
    "String parsing contracts with an explicit error type and complete input validation.",
    "use core::num::ParseError;\n\nfn main() -> bool {\n    (\n        match i32::from_str(\"42\") {\n            Ok(value) => value == 42,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"\") {\n            Err(ParseError::Empty) => true,\n            _ => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"12x\") {\n            Err(ParseError::InvalidDigit) => true,\n            _ => false,\n        }\n    )\n}",
);
