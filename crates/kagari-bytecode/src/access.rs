//! Access-flow validation runs after physical operand and layout validation.

use kagari_abi::{
    operations::{IterOp, StandardEnumOp},
    scalar::BuiltinType as B,
    standard::{RuntimePrimitive as S, surface::StandardEnum},
    types::{self as abi, AbiType, NominalAbiType, PublicAbiItem, access, verify as abi_verify},
};
use kagari_common::{cancellation::CancellationToken, collection::CollectionAccess as Access};

use crate::{
    instruction::{BytecodeInstruction as I, CallTarget, ConstantOperand, Register, RuntimeHelper},
    module::{BytecodeFunction, BytecodeModule},
    program::{BytecodeProgram, ModuleRef},
    trait_bounds,
    verifier::BytecodeVerificationError as Error,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Fact {
    ty: Option<AbiType>,
    access: Option<Access>,
}
impl Fact {
    fn typed(ty: AbiType) -> Self {
        let access = ty.collection_access();
        Self {
            ty: Some(ty),
            access,
        }
    }
}
fn flows(source: &Fact, target: &AbiType) -> bool {
    if source.access == Some(Access::ReadOnly)
        && target.collection_access() == Some(Access::Mutable)
    {
        return false;
    }
    match &source.ty {
        Some(source) => source == target || source.can_weaken_to(target),
        None => true,
    }
}
fn merge(slot: &mut Option<Fact>, value: Fact) -> bool {
    if slot.as_ref() == Some(&value) {
        return false;
    }
    if let Some(old) = slot.as_ref()
        && old.access == Some(Access::ReadOnly)
        && value.access == Some(Access::Mutable)
    {
        return false;
    }
    *slot = Some(value);
    true
}

pub(super) fn verify(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    program: Option<&BytecodeProgram>,
) -> Result<(), Error> {
    let invalid = || Error::InvalidOperation {
        function: function.id,
        reason: "invalid collection access flow",
    };
    let semantic = &function.metadata.semantic;
    if !semantic.protocol_adapter_valid(function.identity.as_ref()) {
        return Err(invalid());
    }
    if semantic
        .params
        .keys()
        .any(|i| *i >= function.parameter_count as usize)
        || semantic
            .locals
            .keys()
            .any(|i| *i >= function.local_count as usize)
        || semantic
            .registers
            .keys()
            .any(|i| *i >= function.register_count as usize)
        || semantic
            .params
            .values()
            .chain(semantic.locals.values())
            .chain(semantic.registers.values())
            .chain(semantic.result.iter())
            .any(|ty| {
                !ty.within_wire_limits()
                    || !abi_verify::concrete_type_valid(ty, &Default::default())
            })
    {
        return Err(invalid());
    }
    if function.identity.is_some()
        && (semantic.params.len() != function.parameter_count as usize || semantic.result.is_none())
    {
        return Err(invalid());
    }
    for (index, ty) in &semantic.params {
        if semantic.locals.get(index).is_some_and(|local| local != ty) {
            return Err(invalid());
        }
    }
    if let Some(identity) = &function.identity {
        let name = identity
            .declaration
            .path
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join("::");
        if let Some(abi) = module.public_items.iter().find_map(|item| match item {
            PublicAbiItem::Function(abi) if abi.name == name => Some(abi),
            _ => None,
        }) {
            let apply = |ty: &AbiType| ty.instantiate(&identity.declaration, &identity.arguments);
            if abi.params.len() != semantic.params.len()
                || abi
                    .params
                    .iter()
                    .enumerate()
                    .any(|(i, p)| apply(&p.ty).as_ref() != semantic.params.get(&i))
                || apply(&abi.return_type).as_ref() != semantic.result.as_ref()
            {
                return Err(invalid());
            }
        }
    }
    let mut locals = vec![None; function.local_count as usize];
    let mut registers = vec![None; function.register_count as usize];
    let mut constants = vec![None; function.register_count as usize];
    for (index, local) in locals
        .iter_mut()
        .enumerate()
        .take(function.parameter_count as usize)
    {
        *local = Some(
            semantic
                .params
                .get(&index)
                .cloned()
                .map(Fact::typed)
                .unwrap_or_default(),
        );
    }
    let owner = |slot: ModuleRef| -> Option<&BytecodeModule> {
        program
            .and_then(|p| p.modules.get(slot.index()))
            .or_else(|| (slot.index() == 0).then_some(module))
    };
    // Register identities are fixed; repeated passes resolve backedges and locals.
    for _ in 0..64 {
        let previous = (locals.clone(), registers.clone());
        let mut changed = false;
        for instruction in &function.instructions {
            let get = |r: Register| registers[r.index()].clone();
            let mut produced: Option<(Register, Fact)> = None;
            match instruction {
                I::LoadConst { dst, constant } => {
                    let ty = match constant {
                        ConstantOperand::Unit => B::Unit,
                        ConstantOperand::Bool(_) => B::Bool,
                        ConstantOperand::I32(n) => {
                            constants[dst.index()] = usize::try_from(*n).ok();
                            B::I32
                        }
                        ConstantOperand::I64(n) => {
                            constants[dst.index()] = usize::try_from(*n).ok();
                            B::I64
                        }
                        ConstantOperand::F32(_) => B::F32,
                        ConstantOperand::F64(_) => B::F64,
                        ConstantOperand::U64(n) => {
                            constants[dst.index()] = usize::try_from(*n).ok();
                            B::U64
                        }
                        ConstantOperand::Str(_) => B::String,
                    };
                    let number = match constant {
                        ConstantOperand::I32(value) => Some(i128::from(*value)),
                        ConstantOperand::I64(value) => Some(i128::from(*value)),
                        ConstantOperand::U64(value) => Some(i128::from(*value)),
                        _ => None,
                    };
                    let ty = if let Some(value) = number
                        && let Some(AbiType::Builtin(declared)) =
                            semantic.registers.get(&dst.index())
                        && declared
                            .integer_bounds()
                            .is_some_and(|(min, max)| (min..=max).contains(&value))
                        && AbiType::Builtin(*declared).representation()
                            == AbiType::Builtin(ty).representation()
                    {
                        *declared
                    } else {
                        ty
                    };
                    produced = Some((*dst, Fact::typed(AbiType::Builtin(ty))));
                }
                I::Move { dst, src } => {
                    if let Some(value) = get(*src) {
                        produced = Some((*dst, value));
                        constants[dst.index()] = constants[src.index()];
                    }
                }
                I::LoadLocal { dst, local } => {
                    if let Some(ty) = semantic.locals.get(&local.index()) {
                        produced = Some((*dst, Fact::typed(ty.clone())));
                    } else if let Some(value) = &locals[local.index()] {
                        produced = Some((*dst, value.clone()));
                    }
                }
                I::StoreLocal { local, src } => {
                    if let Some(value) = get(*src) {
                        if let Some(expected) = semantic.locals.get(&local.index())
                            && !flows(&value, expected)
                        {
                            return Err(invalid());
                        }
                        changed |= merge(&mut locals[local.index()], value);
                    }
                }
                I::RangeBound {
                    dst,
                    value,
                    range,
                    bound,
                    ..
                } => {
                    if let Some(value) = get(*value)
                        && !flows(&value, range)
                    {
                        return Err(invalid());
                    }
                    produced = Some((*dst, Fact::typed(bound.clone())));
                }
                I::MakeRange {
                    dst,
                    start,
                    end,
                    ty,
                } => {
                    let AbiType::Range(item, _) = ty else {
                        return Err(invalid());
                    };
                    for register in start.iter().chain(end) {
                        if let Some(fact) = get(*register)
                            && !flows(&fact, item)
                        {
                            return Err(invalid());
                        }
                    }
                    produced = Some((*dst, Fact::typed(ty.clone())));
                }
                I::RepeatArray {
                    dst,
                    element,
                    value,
                    count,
                } => {
                    if let (Some(value), Some(count)) = (get(*value), get(*count)) {
                        if !flows(&value, element) {
                            return Err(invalid());
                        }
                        let item = element;
                        if !access::supports_array_repetition(
                            item,
                            |instance| {
                                let layout = module.enumerations.iter().find(|layout| {
                                    layout.declaration == instance.declaration
                                        && layout.arguments == instance.arguments
                                })?;
                                Some(
                                    layout
                                        .variants
                                        .iter()
                                        .flat_map(|variant| variant.payload.iter().cloned())
                                        .collect(),
                                )
                            },
                            &CancellationToken::default(),
                        ) {
                            return Err(invalid());
                        }
                        if !flows(&count, &AbiType::Builtin(B::USize)) {
                            return Err(invalid());
                        }
                        produced = Some((
                            *dst,
                            Fact::typed(AbiType::Array(Box::new(element.clone()), Access::Mutable)),
                        ));
                    }
                }
                I::MakeArray {
                    dst,
                    element,
                    elements,
                } => {
                    if !element.is_concrete() {
                        return Err(invalid());
                    }
                    if elements.iter().all(|register| get(*register).is_some()) {
                        for register in elements {
                            if !flows(&get(*register).unwrap(), element) {
                                return Err(invalid());
                            }
                        }
                        produced = Some((
                            *dst,
                            Fact::typed(AbiType::Array(Box::new(element.clone()), Access::Mutable)),
                        ));
                    }
                }
                I::MakeTuple { dst, elements } => {
                    if elements.iter().all(|r| get(*r).is_some()) {
                        let members = elements
                            .iter()
                            .map(|r| get(*r)?.ty)
                            .collect::<Option<Vec<_>>>();
                        produced = Some((
                            *dst,
                            members
                                .map(|m| Fact::typed(AbiType::Tuple(m)))
                                .unwrap_or_default(),
                        ));
                    }
                }
                I::ReadAggregateField { dst, base, field } => {
                    let layout = &module.structures[field.structure.index()];
                    if let Some(Fact {
                        ty: Some(AbiType::Struct(ty)),
                        ..
                    }) = get(*base)
                        && (ty.declaration != layout.declaration
                            || ty.arguments != layout.arguments)
                    {
                        return Err(invalid());
                    }
                    produced = Some((
                        *dst,
                        Fact::typed(layout.fields[field.slot as usize].ty.clone()),
                    ));
                }
                I::WriteAggregateField { base, field, value } => {
                    let layout = &module.structures[field.structure.index()];
                    if let Some(Fact {
                        ty: Some(AbiType::Struct(ty)),
                        ..
                    }) = get(*base)
                        && (ty.declaration != layout.declaration
                            || ty.arguments != layout.arguments)
                    {
                        return Err(invalid());
                    }
                    if let Some(value) = get(*value)
                        && !flows(
                            &value,
                            &module.structures[field.structure.index()].fields[field.slot as usize]
                                .ty,
                        )
                    {
                        return Err(invalid());
                    }
                }
                I::ReadEnumPayload {
                    dst,
                    value,
                    enumeration,
                    variant,
                    index,
                } => {
                    let layout = &module.enumerations[enumeration.index()];
                    if let Some(Fact {
                        ty: Some(AbiType::Enum(ty)),
                        ..
                    }) = get(*value)
                        && (ty.declaration != layout.declaration
                            || ty.arguments != layout.arguments)
                    {
                        return Err(invalid());
                    }
                    produced = Some((
                        *dst,
                        Fact::typed(
                            layout.variants[*variant as usize].payload[*index as usize].clone(),
                        ),
                    ));
                }
                I::ReadAggregateIndex { dst, base, index } => {
                    if let Some(base) = get(*base) {
                        let ty = match base.ty {
                            Some(AbiType::Array(item, _)) => Some(*item),
                            Some(AbiType::Tuple(items)) => {
                                if let Some(index) = constants[index.index()] {
                                    items.get(index).cloned()
                                } else if items.windows(2).all(|pair| pair[0] == pair[1]) {
                                    items.first().cloned()
                                } else {
                                    return Err(invalid());
                                }
                            }
                            _ => None,
                        };
                        produced = Some((*dst, ty.map(Fact::typed).unwrap_or_default()));
                    }
                }
                I::WriteAggregateIndex { base, value, .. } => {
                    if let Some(base) = get(*base) {
                        if base.access == Some(Access::ReadOnly) {
                            return Err(invalid());
                        }
                        if let (Some(AbiType::Array(item, _)), Some(value)) =
                            (&base.ty, get(*value))
                            && !flows(&value, item)
                        {
                            return Err(invalid());
                        }
                    }
                }
                I::StandardEnum { dst, value, ty, op } => {
                    if !matches!(op, StandardEnumOp::Make(_))
                        && let Some(value) = value.and_then(get)
                        && !flows(&value, ty)
                    {
                        return Err(invalid());
                    }
                    if let AbiType::StandardEnum { args, .. } = ty {
                        match op {
                            StandardEnumOp::Make(index) => {
                                let member = if args.len() == 2 { *index as usize } else { 0 };
                                if let (Some(value), Some(expected)) =
                                    (value.and_then(get), args.get(member))
                                    && !flows(&value, expected)
                                {
                                    return Err(invalid());
                                }
                                produced = Some((*dst, Fact::typed(ty.clone())));
                            }
                            StandardEnumOp::Read(index) => {
                                let member = if args.len() == 2 { *index as usize } else { 0 };
                                produced =
                                    args.get(member).cloned().map(|ty| (*dst, Fact::typed(ty)));
                            }
                            StandardEnumOp::Test(_) => {
                                produced = Some((*dst, Fact::typed(AbiType::Builtin(B::Bool))))
                            }
                        }
                    }
                }
                I::Iter { dst, value, ty, op } => {
                    if let Some(value) = value.and_then(get)
                        && !flows(&value, ty)
                    {
                        return Err(invalid());
                    }
                    let item = match ty {
                        AbiType::Tuple(_) if matches!(op, IterOp::String(_)) => {
                            let IterOp::String(kind) = op else {
                                unreachable!()
                            };
                            Some(kind.item_type())
                        }
                        AbiType::Builtin(B::String) => Some(AbiType::Builtin(B::String)),
                        AbiType::Range(item, _)
                        | AbiType::Array(item, _)
                        | AbiType::Set(item, _)
                        | AbiType::Iter(item) => Some((**item).clone()),
                        AbiType::Map { key, value, .. } => {
                            Some(AbiType::Tuple(vec![(**key).clone(), (**value).clone()]))
                        }
                        _ => None,
                    };
                    produced = item.map(|item| {
                        (
                            *dst,
                            Fact::typed(match op {
                                IterOp::New | IterOp::String(_) => AbiType::Iter(Box::new(item)),
                                IterOp::Next => AbiType::StandardEnum {
                                    kind: StandardEnum::Option,
                                    args: vec![item],
                                },
                                _ => AbiType::Builtin(B::Unit),
                            }),
                        )
                    });
                }
                I::MakeStruct {
                    dst,
                    structure,
                    fields,
                } => {
                    let layout = &module.structures[structure.index()];
                    for (value, field) in fields.iter().zip(&layout.fields) {
                        if let Some(value) = get(*value)
                            && !flows(&value, &field.ty)
                        {
                            return Err(invalid());
                        }
                    }
                    produced = Some((
                        *dst,
                        Fact::typed(AbiType::Struct(NominalAbiType {
                            declaration: layout.declaration.clone(),
                            arguments: layout.arguments.clone(),
                            associated_types: Default::default(),
                        })),
                    ));
                }
                I::MakeEnum {
                    dst,
                    enumeration,
                    variant,
                    fields,
                } => {
                    let layout = &module.enumerations[enumeration.index()];
                    for (value, expected) in fields
                        .iter()
                        .zip(&layout.variants[*variant as usize].payload)
                    {
                        if let Some(value) = get(*value)
                            && !flows(&value, expected)
                        {
                            return Err(invalid());
                        }
                    }
                    produced = Some((
                        *dst,
                        Fact::typed(AbiType::Enum(NominalAbiType {
                            declaration: layout.declaration.clone(),
                            arguments: layout.arguments.clone(),
                            associated_types: Default::default(),
                        })),
                    ));
                }
                I::MakeClosure {
                    dst,
                    function: target,
                    captures,
                } => {
                    let target = &module.functions[target.index()];
                    let signature = &target.metadata.semantic;
                    for (index, capture) in captures.iter().enumerate() {
                        if let (Some(value), Some(expected)) =
                            (get(*capture), signature.params.get(&index))
                            && !flows(&value, expected)
                        {
                            return Err(invalid());
                        }
                    }
                    let params = (captures.len()..target.parameter_count as usize)
                        .map(|i| signature.params.get(&i).cloned())
                        .collect::<Option<Vec<_>>>();
                    let ty = params
                        .zip(signature.result.clone())
                        .map(|(params, result)| AbiType::Function {
                            params,
                            result: Box::new(result),
                        });
                    produced = Some((*dst, ty.map(Fact::typed).unwrap_or_default()));
                }
                I::Binary { dst, .. }
                | I::Unary { dst, .. }
                | I::TestEnumVariant { dst, .. }
                | I::LoadModule { dst, .. }
                | I::MakePathView { dst, .. } => produced = Some((*dst, Fact::default())),
                I::MakeInterface {
                    dst,
                    value,
                    module: slot,
                    implementation,
                } => {
                    let owner = owner(*slot).ok_or_else(invalid)?;
                    let linked = &owner.interface_tables[implementation.index()];
                    let table = owner
                        .public_items
                        .iter()
                        .find_map(|item| match item {
                            PublicAbiItem::InterfaceTable(table)
                                if table.declaration == linked.declaration =>
                            {
                                table.instantiate(&linked.arguments)
                            }
                            _ => None,
                        })
                        .ok_or_else(invalid)?;
                    if let Some(value) = get(*value)
                        && !flows(&value, &table.for_type)
                    {
                        return Err(invalid());
                    }
                    produced = Some((*dst, Fact::typed(table.trait_type)));
                }
                I::UpcastInterface {
                    dst,
                    value,
                    source,
                    target,
                } => {
                    if let Some(value) = get(*value)
                        && !flows(&value, &AbiType::Trait(source.clone()))
                    {
                        return Err(invalid());
                    }
                    produced = Some((*dst, Fact::typed(AbiType::Trait(target.clone()))));
                }
                I::Convert {
                    dst,
                    src,
                    conversion,
                } => {
                    let (input, output) = conversion.contract().ok_or_else(invalid)?;
                    if let Some(value) = get(*src)
                        && !flows(&value, &input)
                    {
                        return Err(invalid());
                    }
                    produced = Some((*dst, Fact::typed(output)));
                }
                I::Numeric {
                    dst,
                    operation,
                    lhs,
                    rhs,
                } => {
                    let (left, right, output) = operation.contract().ok_or_else(invalid)?;
                    if let Some(value) = get(*lhs)
                        && !flows(&value, &left)
                    {
                        return Err(invalid());
                    }
                    if let (Some(value), Some(expected)) = (rhs.and_then(get), right)
                        && !flows(&value, &expected)
                    {
                        return Err(invalid());
                    }
                    produced = Some((*dst, Fact::typed(output)));
                }
                I::MapResultError { dst, error, ty, .. } => {
                    if let (Some(value), AbiType::StandardEnum { args, .. }) = (get(*error), ty)
                        && !flows(&value, &args[1])
                    {
                        return Err(invalid());
                    }
                    produced = Some((*dst, Fact::typed(ty.clone())));
                }
                I::SetPath { path, value, .. } => {
                    let record = &module.paths[path.index()];
                    let ty = module
                        .host_interface
                        .paths
                        .iter()
                        .filter_map(|p| p.contract(&module.host_interface).ok())
                        .find(|p| p.fingerprint().ok() == Some(record.contract_fingerprint))
                        .map(|p| AbiType::from_host_type(&p.result));
                    if let (Some(value), Some(expected)) = (get(*value), ty)
                        && !flows(&value, &expected)
                    {
                        return Err(invalid());
                    }
                }
                I::ReadPath { dst, path, .. } => {
                    let record = &module.paths[path.index()];
                    let ty = module
                        .host_interface
                        .paths
                        .iter()
                        .filter_map(|p| p.contract(&module.host_interface).ok())
                        .find(|p| p.fingerprint().ok() == Some(record.contract_fingerprint))
                        .map(|p| AbiType::from_host_type(&p.result));
                    produced = Some((*dst, ty.map(Fact::typed).unwrap_or_default()));
                }
                I::MakeCell { dst, value } => {
                    if let Some(value) = get(*value) {
                        produced = Some((*dst, value));
                    }
                }
                I::ReadCell { dst, cell } => {
                    if let Some(value) = get(*cell) {
                        produced = Some((*dst, value));
                    }
                }
                I::WriteCell { cell, value } => {
                    if let (Some(cell), Some(value)) = (get(*cell), get(*value))
                        && let Some(ty) = cell.ty
                        && !flows(&value, &ty)
                    {
                        return Err(invalid());
                    }
                }
                I::Call { dst, callee, args } => {
                    if args.iter().any(|r| get(*r).is_none()) {
                        continue;
                    }
                    let facts = args.iter().map(|r| get(*r).unwrap()).collect::<Vec<_>>();
                    let mut result = Fact::default();
                    match callee {
                        CallTarget::RuntimePrimitive(intrinsic) => {
                            if *intrinsic == S::StringPartsJoin {
                                if facts[0].ty.as_ref().is_some_and(|ty| !matches!(ty,
                                    AbiType::Array(item, _) if **item == AbiType::Builtin(B::String))) {
                                    return Err(invalid());
                                }
                                result = Fact::typed(AbiType::Builtin(B::String));
                            } else if matches!(intrinsic, S::ValueDebug | S::ValueDisplay) {
                                result = Fact::typed(AbiType::Builtin(B::String));
                            }
                        }
                        CallTarget::Function(target)
                        | CallTarget::ModuleFunction {
                            function: target, ..
                        } => {
                            let target_module =
                                if let CallTarget::ModuleFunction { module: slot, .. } = callee {
                                    owner(*slot).ok_or_else(invalid)?
                                } else {
                                    module
                                };
                            let target = &target_module.functions[target.index()].metadata.semantic;
                            for (index, value) in facts.iter().enumerate() {
                                if let Some(ty) = target.params.get(&index) {
                                    if !flows(value, ty) {
                                        return Err(invalid());
                                    }
                                } else if value.access == Some(Access::ReadOnly) {
                                    return Err(invalid());
                                }
                            }
                            result = target.result.clone().map(Fact::typed).unwrap_or_default();
                        }
                        CallTarget::InterfaceMethod {
                            module: slot,
                            interface,
                            method_slot,
                        } => {
                            let owner = owner(*slot).ok_or_else(invalid)?;
                            let (params, output) = abi::interface_method_semantics(
                                &owner.identity,
                                &owner.public_items,
                                &owner.trait_contracts,
                                interface,
                                *method_slot as usize,
                            )
                            .ok_or_else(invalid)?;
                            if let Some(AbiType::Trait(actual)) = &facts[0].ty {
                                let modules: Vec<_> = program.map_or_else(
                                    || vec![module],
                                    |program| program.modules.iter().collect(),
                                );
                                if !trait_bounds::interface_views(
                                    actual,
                                    &AbiType::Trait(actual.clone()),
                                    &modules,
                                )
                                .is_some_and(|parents| parents.contains(interface))
                                {
                                    return Err(invalid());
                                }
                            } else if abi::is_collection_interface(&interface.declaration) {
                                return Err(invalid());
                            }
                            for (value, expected) in facts.iter().skip(1).zip(params.iter().skip(1))
                            {
                                if !flows(value, expected) {
                                    return Err(invalid());
                                }
                            }
                            result = Fact::typed(output);
                        }
                        CallTarget::Native(target) => {
                            let target = &module.native_imports[target.index()];
                            for (value, parameter) in facts.iter().zip(&target.signature.params) {
                                if !flows(value, parameter) {
                                    return Err(invalid());
                                }
                            }
                            result = Fact::typed(target.signature.result.clone());
                        }

                        CallTarget::RuntimeHelper(
                            RuntimeHelper::ReflectGetField(name)
                            | RuntimeHelper::ReflectSetField(name),
                        ) => {
                            if let Some(AbiType::Struct(nominal)) = &facts[0].ty
                                && let Some(field) = module
                                    .structures
                                    .iter()
                                    .find(|s| {
                                        s.declaration == nominal.declaration
                                            && s.arguments == nominal.arguments
                                    })
                                    .and_then(|s| s.fields.iter().find(|f| f.name == *name))
                            {
                                if matches!(
                                    callee,
                                    CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(_))
                                ) {
                                    if !flows(&facts[1], &field.ty) {
                                        return Err(invalid());
                                    }
                                    result = facts[0].clone();
                                } else {
                                    result = Fact::typed(field.ty.clone());
                                }
                            }
                        }
                        CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex) => {
                            if facts[0].access == Some(Access::ReadOnly) {
                                return Err(invalid());
                            }
                            if let Some(AbiType::Array(item, _)) = &facts[0].ty
                                && !flows(&facts[2], item)
                            {
                                return Err(invalid());
                            }
                            result = facts[0].clone();
                        }
                        CallTarget::ClosureRegister { register, .. } => {
                            if let Some(Fact {
                                ty:
                                    Some(AbiType::Function {
                                        params,
                                        result: output,
                                    }),
                                ..
                            }) = get(*register)
                            {
                                for (value, expected) in facts.iter().zip(params) {
                                    if !flows(value, &expected) {
                                        return Err(invalid());
                                    }
                                }
                                result = Fact::typed(*output);
                            }
                        }
                        _ => {}
                    }
                    if let Some(dst) = dst {
                        produced = Some((*dst, result));
                    }
                }
                I::Return(Some(value)) => {
                    if let (Some(value), Some(expected)) = (get(*value), semantic.result.as_ref())
                        && !flows(&value, expected)
                    {
                        return Err(invalid());
                    }
                }
                _ => {}
            }
            if let Some((dst, mut fact)) = produced {
                if let Some(expected) = semantic.registers.get(&dst.index()) {
                    if !flows(&fact, expected) {
                        return Err(invalid());
                    }
                    fact = Fact::typed(expected.clone());
                }
                changed |= merge(&mut registers[dst.index()], fact);
            }
        }
        if !changed || previous == (locals.clone(), registers.clone()) {
            return Ok(());
        }
    }
    Err(invalid())
}
