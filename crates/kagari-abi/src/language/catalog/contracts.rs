//! Complete syntax/value contracts, explicitly expressed in Kagari's type model.
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::ModuleDecl,
    language::{self, Protocol, primitive},
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{
        AbiType, AssociatedTypeAbi, ConstraintAbi, FunctionAbi, GenericParameterAbi,
        NominalAbiType, ParameterAbi, TraitAbi,
    },
};
use kagari_common::identity::{DefinitionId, associated_type_id};

pub(super) fn contract(kind: Protocol, parameters: &[&str]) -> TraitAbi {
    let owner = language::identity(kind);
    TraitAbi {
        name: kind.name().into(),
        supertraits: vec![],
        generic_params: parameters
            .iter()
            .enumerate()
            .map(|(position, _)| GenericParameterAbi {
                owner: owner.clone(),
                position,
            })
            .collect(),
        bounds: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        methods: vec![],
    }
}

pub(super) fn receiver(kind: Protocol) -> AbiType {
    AbiType::SelfType(language::identity(kind))
}

pub(super) fn output(kind: Protocol, name: &str) -> AbiType {
    let owner = language::identity(kind);
    AbiType::Projection {
        receiver: Box::new(receiver(kind)),
        interface: Box::new(primitive::applied(kind, vec![])),
        member: associated_type_id(&owner, name),
        arguments: vec![],
    }
}

pub(super) fn associated(
    owner: &DefinitionId,
    name: &str,
    bounds: Vec<ConstraintAbi>,
) -> AssociatedTypeAbi {
    AssociatedTypeAbi {
        declaration: associated_type_id(owner, name),
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds,
    }
}

pub(super) fn method(name: &str, params: Vec<AbiType>, result: AbiType) -> FunctionAbi {
    FunctionAbi {
        name: name.into(),
        method_policy: MethodPolicy::default(),
        implementation: CallableImplementation::Required,
        generic_params: vec![],
        bounds: vec![],
        params: params
            .into_iter()
            .enumerate()
            .map(|(index, ty)| ParameterAbi {
                name: if index == 0 {
                    "self".into()
                } else {
                    format!("arg{index}")
                },
                ty,
                mutable: false,
            })
            .collect(),
        return_type: result,
    }
}

pub(super) fn enum_type(kind: StandardEnum, args: Vec<AbiType>) -> AbiType {
    AbiType::StandardEnum { kind, args }
}

pub(super) fn option(item: AbiType) -> AbiType {
    enum_type(StandardEnum::Option, vec![item])
}

pub(super) fn unit() -> AbiType {
    AbiType::Builtin(BuiltinType::Unit)
}

pub(super) fn usize_type() -> AbiType {
    AbiType::Builtin(BuiltinType::USize)
}

pub(super) fn boolean() -> AbiType {
    AbiType::Builtin(BuiltinType::Bool)
}

pub(super) fn applied_item(kind: Protocol, item: AbiType) -> NominalAbiType {
    let mut applied = primitive::applied(kind, vec![]);
    applied
        .associated_types
        .insert(associated_type_id(&applied.declaration, "Item"), item);
    applied
}

pub(super) fn declare(module: &mut ModuleDecl) {
    for kind in [
        Protocol::PartialEq,
        Protocol::Eq,
        Protocol::Hash,
        Protocol::PartialOrd,
        Protocol::Ord,
        Protocol::Debug,
        Protocol::Display,
    ] {
        let mut declaration = contract(kind, &[]);
        let this = receiver(kind);
        match kind {
            Protocol::PartialEq => {
                declaration
                    .methods
                    .push(method("eq", vec![this.clone(), this], boolean()))
            }
            Protocol::Eq => declaration
                .supertraits
                .push(primitive::applied(Protocol::PartialEq, vec![])),
            Protocol::Hash => declaration.methods.push(method(
                "hash",
                vec![this],
                AbiType::Builtin(BuiltinType::I64),
            )),
            Protocol::PartialOrd => {
                declaration
                    .supertraits
                    .push(primitive::applied(Protocol::PartialEq, vec![]));
                declaration.methods.push(method(
                    "partial_cmp",
                    vec![this.clone(), this],
                    option(enum_type(StandardEnum::Ordering, vec![])),
                ));
            }
            Protocol::Ord => {
                declaration.supertraits.extend([
                    primitive::applied(Protocol::Eq, vec![]),
                    primitive::applied(Protocol::PartialOrd, vec![]),
                ]);
                declaration.methods.push(method(
                    "cmp",
                    vec![this.clone(), this],
                    enum_type(StandardEnum::Ordering, vec![]),
                ));
            }
            Protocol::Debug | Protocol::Display => declaration.methods.push(method(
                if kind == Protocol::Debug {
                    "debug"
                } else {
                    "display"
                },
                vec![this],
                AbiType::Builtin(BuiltinType::String),
            )),
            _ => unreachable!("value protocol list"),
        }
        module.documentation.insert(
            language::identity(kind),
            format!("Language-owned {} contract.", kind.name()),
        );
        module.traits.push(declaration);
    }
    let mut residual = contract(Protocol::From, &["S"]);
    let mut convert = method(
        "from",
        vec![residual.generic_params[0].as_type()],
        receiver(Protocol::From),
    );
    convert.params[0].name = "value".into();
    residual.methods.push(convert);
    module.traits.push(residual);
    for (kind, name) in [
        (Protocol::Add, "add"),
        (Protocol::Sub, "sub"),
        (Protocol::Mul, "mul"),
        (Protocol::Div, "div"),
        (Protocol::Rem, "rem"),
        (Protocol::BitAnd, "bitand"),
        (Protocol::BitOr, "bitor"),
        (Protocol::BitXor, "bitxor"),
        (Protocol::Shl, "shl"),
        (Protocol::Shr, "shr"),
        (Protocol::Neg, "neg"),
        (Protocol::Not, "not"),
        (Protocol::Index, "index"),
        (Protocol::Fn, "call"),
    ] {
        let parameterized =
            kind.binary_operator() || matches!(kind, Protocol::Index | Protocol::Fn);
        let mut declaration = contract(kind, if parameterized { &["Arg"] } else { &[] });
        declaration
            .associated_types
            .push(associated(&language::identity(kind), "Output", vec![]));
        let mut result = output(kind, "Output");
        let mut params = vec![receiver(kind)];
        if parameterized {
            let argument = declaration.generic_params[0].as_type();
            if let AbiType::Projection { interface, .. } = &mut result {
                interface.arguments.push(argument.clone());
            }
            params.push(argument);
        }
        declaration.methods.push(method(name, params, result));
        module.traits.push(declaration);
    }
    let mut bounds = contract(Protocol::RangeBounds, &["T"]);
    let bound = enum_type(
        StandardEnum::Bound,
        vec![bounds.generic_params[0].as_type()],
    );
    bounds.methods.extend([
        method(
            "start_bound",
            vec![receiver(Protocol::RangeBounds)],
            bound.clone(),
        ),
        method("end_bound", vec![receiver(Protocol::RangeBounds)], bound),
    ]);
    module.traits.push(bounds);
    module.documentation.insert(language::identity(Protocol::Iterator),
        "A shared cursor. Each next call advances it and returns Some(item), or None when exhausted.".into());
    module.documentation.insert(
        language::identity(Protocol::Iterable),
        "Produces an iterator whose Item matches this collection or sequence.".into(),
    );
    let mut iterator = contract(Protocol::Iterator, &[]);
    iterator.associated_types.push(associated(
        &language::identity(Protocol::Iterator),
        "Item",
        vec![],
    ));
    iterator.methods.push(method(
        "next",
        vec![receiver(Protocol::Iterator)],
        option(output(Protocol::Iterator, "Item")),
    ));
    module.traits.push(iterator);
    let mut iterable = contract(Protocol::Iterable, &[]);
    let owner = language::identity(Protocol::Iterable);
    iterable
        .associated_types
        .push(associated(&owner, "Item", vec![]));
    iterable.associated_types.push(associated(
        &owner,
        "Iter",
        vec![ConstraintAbi::Trait(applied_item(
            Protocol::Iterator,
            output(Protocol::Iterable, "Item"),
        ))],
    ));
    iterable.methods.push(method(
        "iter",
        vec![receiver(Protocol::Iterable)],
        output(Protocol::Iterable, "Iter"),
    ));
    module.traits.push(iterable);
}
