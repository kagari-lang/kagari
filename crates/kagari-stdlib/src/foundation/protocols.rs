//! Canonical standard trait schemas; required methods use ordinary MethodDecl lowering.
mod collections;
mod conversion;
use crate::{
    catalog::documentation,
    foundation::{enumeration, interface},
    namespaces,
};
use kagari_common::identity::DefinitionKind;
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

type ModuleInventory = (&'static str, &'static str, &'static [(&'static str, usize)]);

pub(super) const INVENTORY: &[ModuleInventory] = &[
    (
        "core",
        "iter",
        &[
            ("Iterator", 0),
            ("Iterable", 0),
            ("FromIterator", 1),
            ("Sum", 1),
            ("Product", 1),
        ],
    ),
    (
        "core",
        "cmp",
        &[("PartialEq", 0), ("Eq", 0), ("PartialOrd", 0), ("Ord", 0)],
    ),
    ("core", "hash", &[("Hash", 0)]),
    ("core", "fmt", &[("Debug", 0), ("Display", 0)]),
    (
        "core",
        "ops",
        &[
            ("Add", 1),
            ("Sub", 1),
            ("Mul", 1),
            ("Div", 1),
            ("Rem", 1),
            ("BitAnd", 1),
            ("BitOr", 1),
            ("BitXor", 1),
            ("Shl", 1),
            ("Shr", 1),
            ("Neg", 0),
            ("Not", 0),
            ("Index", 1),
            ("Fn", 1),
            ("RangeBounds", 1),
            ("Try", 0),
            ("FromResidual", 1),
        ],
    ),
    (
        "core",
        "convert",
        &[("From", 1), ("Into", 1), ("TryFrom", 1), ("TryInto", 1)],
    ),
    ("core", "str", &[("FromStr", 0)]),
    ("core", "default", &[("Default", 0)]),
    (
        "std",
        "collections",
        &[
            ("List", 1),
            ("MutableList", 1),
            ("Map", 2),
            ("MutableMap", 2),
            ("Set", 1),
            ("MutableSet", 1),
        ],
    ),
];

struct Contract {
    name: &'static str,
    arity: usize,
    owner: DraftReference,
    methods: Vec<FnDecl>,
    associated: Vec<AssociatedTypeDef>,
    supertraits: Vec<NominalTy>,
    conversion: Option<ConversionAdapter>,
}

impl Contract {
    fn new(module: &mut ModuleDraft<'_>, name: &'static str, arity: usize) -> NativeResult<Self> {
        Ok(Self {
            name,
            arity,
            owner: interface(module, name, arity)?,
            methods: vec![],
            associated: vec![],
            supertraits: vec![],
            conversion: None,
        })
    }

    fn method(&mut self, module: &ModuleDraft<'_>, declaration: MethodDecl) -> NativeResult<()> {
        self.methods
            .push(module.trait_method(self.owner.id(), declaration)?);
        Ok(())
    }

    fn associated(
        &mut self,
        module: &mut ModuleDraft<'_>,
        name: &str,
        bounds: Vec<NominalTy>,
    ) -> NativeResult<DraftMember> {
        let member = module.reference_associated_type(&self.owner, name, 0)?;
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
        module.document(self.owner.id(), documentation::trait_markdown(self.name))?;
        for method in &self.methods {
            let id = module.member(self.owner.id(), DefinitionKind::Method, &method.name)?;
            module.document(
                id,
                documentation::implementation_docs(self.name, &method.name),
            )?;
        }
        for member in &self.associated {
            let name = module
                .definitions()
                .resolve(member.declaration)
                .expect("associated identity")
                .last()
                .expect("member")
                .name
                .to_owned();
            module.document(member.declaration, documentation::associated_docs(&name))?;
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
        for &(name, arity) in declarations {
            let mut contract = Contract::new(module, name, arity)?;
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
                    for name in ["start_bound", "end_bound"] {
                        contract
                            .method(module, MethodDecl::instance(name).returns(result.clone()))?;
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
    let item_member = contract.associated(module, "Item", vec![])?;
    let item = contract.projection(&item_member)?;
    if contract.name == "Iterator" {
        let result = enumeration(module, "Option", 1)?.apply([item])?;
        contract.method(module, MethodDecl::instance("next").returns(result))?;
    } else {
        let iterator = interface(module, "Iterator", 0)?;
        let output = module.reference_associated_type(&iterator, "Item", 0)?;
        let cursor = contract.associated(
            module,
            "Iter",
            vec![nominal(output.bind(iterator.apply([])?, item)?)],
        )?;
        contract.method(
            module,
            MethodDecl::instance("iter").returns(contract.projection(&cursor)?),
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
        contract.method(module, declaration)?;
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
    let output = contract.associated(module, "Output", vec![])?;
    let mut declaration = MethodDecl::instance(member).returns(contract.projection(&output)?);
    if contract.arity == 1 {
        declaration = declaration.parameter("arg1", contract.owner.parameter(0)?);
    }
    contract.method(module, declaration)
}

fn propagation(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    if contract.name == "FromResidual" {
        return contract.method(
            module,
            MethodDecl::static_method("from_residual")
                .parameter("residual", contract.owner.parameter(0)?)
                .returns(contract.owner.receiver()?),
        );
    }
    let output = contract.associated(module, "Output", vec![])?;
    let residual = contract.associated(module, "Residual", vec![])?;
    let output = contract.projection(&output)?;
    let residual = contract.projection(&residual)?;
    contract.parent(interface(module, "FromResidual", 1)?.apply([residual.clone()])?);
    contract.method(
        module,
        MethodDecl::static_method("from_output")
            .parameter("output", output.clone())
            .returns(contract.owner.receiver()?),
    )?;
    let result = enumeration(module, "ControlFlow", 2)?.apply([residual, output])?;
    contract.method(module, MethodDecl::instance("branch").returns(result))
}
