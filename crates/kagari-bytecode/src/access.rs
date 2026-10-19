//! Access-flow validation runs after physical operand and layout validation.
mod scalar;
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionTable;
use kagari_contract::ids::FunctionRef;
use kagari_contract::representation::builtin_representation;
use std::{collections::BTreeMap, mem, sync::Arc};

use crate::{
    instruction::{
        BytecodeInstruction as I, CallTarget, ConstantOperand, RuntimeHelper, ScalarOperand, Slot,
    },
    module::{BytecodeFunction, BytecodeModule, slots::TypeShape},
    program::{BytecodeProgram, ModuleRef},
    suspension::FlowBudget,
    trait_bounds,
    verifier::{BytecodeVerificationError as Error, flow::Block},
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::{
    callable::receiver::{accepts_owned, accepts_view},
    operations::IterOp,
    standard::RuntimePrimitive as S,
    types::{PublicItem, inherent::executable_signature_matches},
};
use kagari_types::{
    collection::CollectionAccess as Access,
    language::binding,
    reference::{ReferenceAccess, field_read_type},
    scalar::BuiltinType as B,
    ty::{NominalTy, Ty, access, substitution::MAX_TYPE_NODES},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Fact {
    ty: Option<Arc<Ty>>,
    access: Option<Access>,
    conflict: bool,
    nodes: usize,
    fresh: bool,
}

impl Fact {
    fn holds_reference(&self) -> bool {
        self.ty
            .as_deref()
            .is_some_and(Ty::contains_stored_reference)
    }

    fn typed(ty: Ty) -> Self {
        Self {
            access: ty.collection_access(),
            ty: Some(Arc::new(ty)),
            fresh: true,
            ..Self::default()
        }
    }
}

struct TypeBudget<'a> {
    function: FunctionRef,
    flow: &'a mut FlowBudget,
    bytes: usize,
}

impl TypeBudget<'_> {
    fn invalid(&self, reason: &'static str) -> Error {
        Error::InvalidOperation {
            function: self.function,
            reason,
        }
    }

    fn work(&mut self, nodes: usize) -> Result<(), Error> {
        self.flow.work = self.flow.work.saturating_add(nodes);
        if self.flow.work > 100_000_000 {
            return Err(self.invalid("access flow work limit"));
        }
        Ok(())
    }

    fn preflight<'a>(&mut self, children: impl IntoIterator<Item = &'a Ty>) -> Result<(), Error> {
        let shape = TypeShape::measure(children, 2)
            .ok_or_else(|| self.invalid("access flow type shape limit"))?;
        self.work(shape.nodes)?;
        self.space(shape.bytes)
    }

    fn space(&self, bytes: usize) -> Result<(), Error> {
        if self.bytes.saturating_add(bytes) > 64 * 1024 * 1024 {
            return Err(self.invalid("access flow state limit"));
        }
        Ok(())
    }

    fn admit(&mut self, fact: &mut Fact) -> Result<(), Error> {
        if !fact.fresh {
            return Ok(());
        }
        let ty = fact.ty.as_deref().expect("new type fact");
        let shape = TypeShape::measure([ty], 1)
            .ok_or_else(|| self.invalid("access flow type shape limit"))?;
        self.work(shape.nodes)?;
        self.space(shape.bytes)?;
        if !ty.reference_position_valid() {
            return Err(self.invalid("invalid collection access flow"));
        }
        // Cumulative new allocation bounds peak retained trees without an identity
        // cache. Moves, joins and CFG-state clones share already charged Arcs.
        self.bytes += shape.bytes;
        fact.nodes = shape.nodes;
        fact.fresh = false;
        Ok(())
    }
}

fn flows(source: &Fact, target: &Ty) -> bool {
    if source.conflict {
        return false;
    }
    if source.access == Some(Access::ReadOnly)
        && target.collection_access() == Some(Access::Mutable)
    {
        return false;
    }
    match source.ty.as_deref() {
        Some(source) => source == target || source.can_weaken_to(target),
        None => !target.contains_stored_reference(),
    }
}

fn call_argument_flows(
    source: &Fact,
    target: &Ty,
    receiver_access: Option<ReferenceAccess>,
    physical: ValueType,
) -> bool {
    if receiver_access.is_some() && physical == ValueType::HostHandle {
        source
            .ty
            .as_deref()
            .is_some_and(|actual| accepts_view(receiver_access, target, actual))
    } else {
        accepts_owned(receiver_access, target) && flows(source, target)
    }
}

fn join(left: Option<&Fact>, right: Option<&Fact>, declared: Option<&Fact>) -> Option<Fact> {
    let (Some(left), Some(right)) = (left, right) else {
        return None;
    };
    if left == right {
        return Some(left.clone());
    }
    let access = if left.access == Some(Access::ReadOnly) || right.access == Some(Access::ReadOnly)
    {
        Some(Access::ReadOnly)
    } else if left.access == right.access {
        left.access
    } else {
        None
    };
    let compatible = declared.filter(|fact| {
        fact.ty
            .as_deref()
            .is_some_and(|ty| flows(left, ty) && flows(right, ty))
    });
    let incompatible = left.ty != right.ty && compatible.is_none();
    // Do not lose a scoped-reference prohibition when unlike incoming facts meet.
    let mut fact = compatible
        .or_else(|| {
            if left.holds_reference() {
                Some(left)
            } else if right.holds_reference() {
                Some(right)
            } else {
                None
            }
        })
        .cloned()
        .unwrap_or_default();
    fact.access = access;
    fact.conflict = left.conflict || right.conflict || incompatible;
    Some(fact)
}

pub(super) fn verify(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    program: Option<&BytecodeProgram>,
    definitions: &DefinitionTable,
    blocks: &[Block],
    budget: &mut FlowBudget,
) -> Result<(), Error> {
    let invalid = || Error::InvalidOperation {
        function: function.id,
        reason: "invalid collection access flow",
    };
    let semantic = &function.metadata.semantic;
    let path_contracts = module.path_contracts(definitions).map_err(|_| invalid())?;
    if !semantic.protocol_adapter_valid(function.identity.as_ref(), definitions) {
        return Err(invalid());
    }
    if !semantic.types_valid(function.identity.as_ref(), &Default::default(), definitions)
        || !semantic.generic_layout_valid(
            &function.metadata.slots,
            &function.metadata.parameter_slots,
            function.metadata.return_type,
        )
    {
        return Err(invalid());
    }
    if semantic
        .params
        .keys()
        .any(|i| *i >= function.parameter_count as usize)
        || semantic
            .slots
            .keys()
            .any(|i| *i >= function.metadata.slots.len())
    {
        return Err(invalid());
    }
    if function.identity.is_some()
        && (semantic.params.len() != function.parameter_count as usize || semantic.result.is_none())
    {
        return Err(invalid());
    }
    if let Some(identity) = &function.identity {
        if !executable_signature_matches(&module.public_items, identity, semantic, definitions) {
            return Err(invalid());
        }
        let name = definitions
            .resolve(identity.declaration)
            .map_err(|_| invalid())?
            .segments()
            .map(|p| p.name)
            .collect::<Vec<_>>()
            .join("::");
        if let Some(abi) = module.public_items.iter().find_map(|item| match item {
            PublicItem::Function(abi) if abi.name == name => Some(abi),
            _ => None,
        }) {
            let apply = |ty: &Ty| ty.instantiate(&identity.declaration, &identity.arguments);
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
    // CFG states share immutable full types; copying a state must not multiply
    // a large nominal/tuple type by the number of blocks. Declared types have
    // this verification invocation's lifetime only, never a runtime cache.
    let count = function.metadata.slots.len();
    let bytes = (blocks.len() + 2)
        .saturating_mul(count)
        .saturating_mul(mem::size_of::<Option<Fact>>() + mem::size_of::<Option<usize>>())
        .saturating_add(2 * MAX_TYPE_NODES * mem::size_of::<(&Ty, usize)>())
        .saturating_add(
            blocks
                .len()
                .saturating_mul(4 * mem::size_of::<Vec<Fact>>() + 1),
        )
        .saturating_add(
            semantic
                .slots
                .len()
                .saturating_mul(12 * mem::size_of::<(usize, Fact)>()),
        )
        .saturating_add(
            function
                .instructions
                .iter()
                .map(I::operand_vector_len)
                .max()
                .unwrap_or(0)
                .saturating_mul(mem::size_of::<Fact>()),
        );
    let mut types = TypeBudget {
        function: function.id,
        bytes: bytes.saturating_add(budget.retained_bytes),
        flow: budget,
    };
    types.space(0)?;
    let mut declared = BTreeMap::new();
    for (&slot, ty) in &semantic.slots {
        let mut fact = Fact::typed(ty.clone());
        types.admit(&mut fact)?;
        declared.insert(slot, fact);
    }
    let mut outputs = vec![(vec![None; count], vec![None; count]); blocks.len()];
    let mut computed = vec![false; blocks.len()];
    let mut entry = (vec![None; count], vec![None; count]);
    for (index, slot) in function.metadata.parameter_slots.iter().enumerate() {
        let mut fact = semantic
            .params
            .get(&index)
            .cloned()
            .map(Fact::typed)
            .unwrap_or_default();
        types.admit(&mut fact)?;
        entry.0[slot.index()] = Some(fact);
    }
    let mut validate = false;
    let owner = |slot: ModuleRef| -> Option<&BytecodeModule> {
        program
            .and_then(|p| p.modules.get(slot.index()))
            .or_else(|| (slot.index() == 0).then_some(module))
    };
    loop {
        let mut changed = false;
        for (block_index, block) in blocks.iter().enumerate() {
            if !block.reachable {
                continue;
            }
            types.work(
                count
                    .saturating_mul(block.predecessors.len() + 2)
                    .saturating_add(block.end - block.start),
            )?;
            // An unvisited predecessor is lattice top, not an unknown value.
            // The external entry edge participates alongside actual backedges.
            let mut incoming = (block_index == 0).then(|| entry.clone());
            for &predecessor in &block.predecessors {
                if !blocks[predecessor].reachable || !computed[predecessor] {
                    continue;
                }
                if let Some(incoming) = &mut incoming {
                    for slot in 0..count {
                        incoming.0[slot] = join(
                            incoming.0[slot].as_ref(),
                            outputs[predecessor].0[slot].as_ref(),
                            declared.get(&slot),
                        );
                        if incoming.1[slot] != outputs[predecessor].1[slot] {
                            incoming.1[slot] = None;
                        }
                    }
                } else {
                    incoming = Some(outputs[predecessor].clone());
                }
            }
            let Some((mut registers, mut constants)) = incoming else {
                continue;
            };
            for instruction in &function.instructions[block.start..block.end] {
                if validate
                    && instruction.slot_inputs().iter().any(|slot| {
                        registers[slot.index()]
                            .as_ref()
                            .is_some_and(|fact| fact.conflict)
                    })
                {
                    return Err(invalid());
                }
                types.work(
                    instruction
                        .slot_inputs()
                        .iter()
                        .map(|slot| {
                            registers[slot.index()]
                                .as_ref()
                                .map_or(1, |fact| fact.nodes + 1)
                        })
                        .sum(),
                )?;
                if let I::MakeTuple { elements, .. } = instruction {
                    types.preflight(
                        elements
                            .iter()
                            .filter_map(|slot| registers[slot.index()].as_ref()?.ty.as_deref()),
                    )?;
                }
                if let I::MakeClosure {
                    function: target,
                    captures,
                    ..
                } = instruction
                {
                    let target = &module.functions[target.index()];
                    let signature = &target.metadata.semantic;
                    types.preflight(
                        (captures.len()..target.parameter_count as usize)
                            .filter_map(|index| signature.params.get(&index))
                            .chain(signature.result.as_ref()),
                    )?;
                }
                let get = |r: Slot| registers[r.index()].clone();
                let mut produced: Option<(Slot, Fact)> = None;
                let mut discarded = None;
                let checked = (|| -> Result<(), Error> {
                    match instruction {
                        I::LoadConst { dst, constant } => {
                            let constant =
                                module.constants.get(constant.index()).ok_or_else(invalid)?;
                            let ty = match constant {
                                ConstantOperand::Unit => B::Unit,
                                ConstantOperand::Bool(_) => B::Bool,
                                ConstantOperand::I32(_) => B::I32,
                                ConstantOperand::I64(_) => B::I64,
                                ConstantOperand::F32(_) => B::F32,
                                ConstantOperand::F64(_) => B::F64,
                                ConstantOperand::U64(_) => B::U64,
                                ConstantOperand::Str(_) => B::String,
                            };
                            let number = match constant {
                                ConstantOperand::I32(value) => Some(i128::from(*value)),
                                ConstantOperand::I64(value) => Some(i128::from(*value)),
                                ConstantOperand::U64(value) => Some(i128::from(*value)),
                                _ => None,
                            };
                            let ty = if let Some(value) = number
                                && let Some(Ty::Builtin(declared)) =
                                    semantic.slots.get(&dst.index())
                                && declared
                                    .integer_bounds()
                                    .is_some_and(|(min, max)| (min..=max).contains(&value))
                                && builtin_representation(*declared) == builtin_representation(ty)
                            {
                                *declared
                            } else {
                                ty
                            };
                            produced = Some((*dst, Fact::typed(Ty::Builtin(ty))));
                        }
                        I::Move { dst, src } => {
                            if let Some(value) = get(*src) {
                                produced = Some((*dst, value));
                            }
                        }
                        I::StoreModule { src, .. } => {
                            if get(*src).is_some_and(|value| value.holds_reference()) {
                                return Err(invalid());
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
                            let Ty::Range(item, _) = ty else {
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
                                if !flows(&count, &Ty::Builtin(B::U64)) {
                                    return Err(invalid());
                                }
                                produced =
                                    Some((*dst, Fact::typed(Ty::Array(Box::new(element.clone())))));
                            }
                        }
                        I::MakeArray {
                            dst,
                            element,
                            elements,
                        } => {
                            if !element.is_concrete()
                                && semantic.generic.as_ref().is_none_or(|body| {
                                    !body.types_valid([element], &Default::default(), definitions)
                                })
                            {
                                return Err(invalid());
                            }
                            if elements.iter().all(|register| get(*register).is_some()) {
                                for register in elements {
                                    if !flows(&get(*register).unwrap(), element) {
                                        return Err(invalid());
                                    }
                                }
                                produced =
                                    Some((*dst, Fact::typed(Ty::Array(Box::new(element.clone())))));
                            }
                        }
                        I::MakeTuple { dst, elements } => {
                            if elements.iter().all(|r| get(*r).is_some()) {
                                let members = elements
                                    .iter()
                                    .map(|r| get(*r)?.ty.as_deref().cloned())
                                    .collect::<Option<Vec<_>>>();
                                produced = Some((
                                    *dst,
                                    members
                                        .map(|m| Fact::typed(Ty::Tuple(m)))
                                        .unwrap_or_default(),
                                ));
                            }
                        }
                        I::ReadAggregateField { dst, base, field } => {
                            let layout = module.structures[field.structure.index()]
                                .apply(&field.arguments, &Default::default())
                                .ok_or_else(invalid)?;
                            let target = &layout.fields[field.slot as usize];
                            let nominal = Ty::Struct(NominalTy {
                                declaration: layout.declaration,
                                arguments: layout.arguments.clone(),
                                associated_types: Default::default(),
                            });
                            let receiver =
                                field
                                    .reference
                                    .map_or(nominal.clone(), |access| Ty::Reference {
                                        target: Box::new(nominal),
                                        access,
                                    });
                            if let Some(base) = get(*base)
                                && !flows(&base, &receiver)
                            {
                                return Err(invalid());
                            }
                            let ty = if let Some(access) = field.reference {
                                field_read_type(&target.ty, access, target.mutable)
                                    .ok_or_else(invalid)?
                            } else {
                                target.ty.clone()
                            };
                            produced = Some((*dst, Fact::typed(ty)));
                        }
                        I::WriteAggregateField { base, field, value } => {
                            let layout = module.structures[field.structure.index()]
                                .apply(&field.arguments, &Default::default())
                                .ok_or_else(invalid)?;
                            let nominal = Ty::Struct(NominalTy {
                                declaration: layout.declaration,
                                arguments: layout.arguments.clone(),
                                associated_types: Default::default(),
                            });
                            let receiver =
                                field
                                    .reference
                                    .map_or(nominal.clone(), |access| Ty::Reference {
                                        target: Box::new(nominal),
                                        access,
                                    });
                            if let Some(base) = get(*base)
                                && !flows(&base, &receiver)
                            {
                                return Err(invalid());
                            }
                            if let Some(value) = get(*value)
                                && !flows(&value, &layout.fields[field.slot as usize].ty)
                            {
                                return Err(invalid());
                            }
                        }
                        I::ReadEnumPayload {
                            dst,
                            value,
                            enumeration,
                            arguments,
                            variant,
                            index,
                        } => {
                            let layout = module.enumerations[enumeration.index()]
                                .apply(arguments, &Default::default())
                                .ok_or_else(invalid)?;
                            if let Some(Ty::Enum(ty)) =
                                get(*value).and_then(|fact| fact.ty.as_deref().cloned())
                                && (ty.declaration != layout.declaration
                                    || ty.arguments != layout.arguments)
                            {
                                return Err(invalid());
                            }
                            produced = Some((
                                *dst,
                                Fact::typed(
                                    layout.variants[*variant as usize].payload[*index as usize]
                                        .clone(),
                                ),
                            ));
                        }
                        I::ReadAggregateIndex { dst, base, index } => {
                            if let Some(base) = get(*base) {
                                let ty = match base.ty.as_deref().cloned() {
                                    Some(Ty::Array(item)) => Some(*item),
                                    Some(Ty::Tuple(items)) => {
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
                                if let (Some(Ty::Array(item)), Some(value)) =
                                    (base.ty.as_deref(), get(*value))
                                    && !flows(&value, item)
                                {
                                    return Err(invalid());
                                }
                            }
                        }

                        I::Await { dst, value, future } => {
                            if let Some(value) = get(*value)
                                && !flows(&value, future)
                            {
                                return Err(invalid());
                            }
                            let Ty::NativeObject(nominal) = future else {
                                return Err(invalid());
                            };
                            produced = Some((*dst, Fact::typed(nominal.arguments[0].clone())));
                        }
                        I::Iter { dst, value, ty, op } => {
                            if let Some(value) = value.and_then(get)
                                && !flows(&value, ty)
                            {
                                return Err(invalid());
                            }
                            let item = match ty {
                                Ty::Tuple(_) if matches!(op, IterOp::String(_)) => {
                                    let IterOp::String(kind) = op else {
                                        unreachable!()
                                    };
                                    Some(kind.item_type())
                                }
                                Ty::Builtin(B::String) => Some(Ty::Builtin(B::String)),
                                Ty::Range(item, _)
                                | Ty::Array(item)
                                | Ty::Set(item, _)
                                | Ty::Iter(item) => Some((**item).clone()),
                                Ty::Map { key, value, .. } => {
                                    Some(Ty::Tuple(vec![(**key).clone(), (**value).clone()]))
                                }
                                _ => None,
                            };
                            produced = item
                                .map(|item| {
                                    Ok((
                                        *dst,
                                        Fact::typed(match op {
                                            IterOp::New | IterOp::String(_) => {
                                                Ty::Iter(Box::new(item))
                                            }
                                            IterOp::Next => Ty::Enum(NominalTy {
                                                declaration: definitions
                                                    .lookup(&binding::option_declaration())
                                                    .ok_or_else(invalid)?,
                                                arguments: vec![item],
                                                associated_types: Default::default(),
                                            }),
                                        }),
                                    ))
                                })
                                .transpose()?;
                        }
                        I::MakeStruct {
                            dst,
                            structure,
                            arguments,
                            fields,
                        } => {
                            let layout = module.structures[structure.index()]
                                .apply(arguments, &Default::default())
                                .ok_or_else(invalid)?;
                            for (value, field) in fields.iter().zip(&layout.fields) {
                                if let Some(value) = get(*value)
                                    && !flows(&value, &field.ty)
                                {
                                    return Err(invalid());
                                }
                            }
                            produced = Some((
                                *dst,
                                Fact::typed(Ty::Struct(NominalTy {
                                    declaration: layout.declaration,
                                    arguments: layout.arguments.clone(),
                                    associated_types: Default::default(),
                                })),
                            ));
                        }
                        I::MakeEnum {
                            dst,
                            enumeration,
                            arguments,
                            variant,
                            fields,
                        } => {
                            let layout = module.enumerations[enumeration.index()]
                                .apply(arguments, &Default::default())
                                .ok_or_else(invalid)?;
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
                                Fact::typed(Ty::Enum(NominalTy {
                                    declaration: layout.declaration,
                                    arguments: layout.arguments.clone(),
                                    associated_types: Default::default(),
                                })),
                            ));
                        }
                        I::MakeFuture {
                            dst,
                            function: target,
                            arguments,
                            future,
                        } => {
                            let signature = &module.functions[target.index()].metadata.semantic;
                            for (index, argument) in arguments.iter().enumerate() {
                                if get(*argument).is_some_and(|fact| fact.holds_reference()) {
                                    return Err(invalid());
                                }
                                if let (Some(value), Some(expected)) =
                                    (get(*argument), signature.params.get(&index))
                                    && !flows(&value, expected)
                                {
                                    return Err(invalid());
                                }
                            }
                            produced = Some((*dst, Fact::typed(future.clone())));
                        }
                        I::MakeClosure {
                            dst,
                            function: target,
                            captures,
                        } => {
                            let target = &module.functions[target.index()];
                            let signature = &target.metadata.semantic;
                            for (index, capture) in captures.iter().enumerate() {
                                if get(*capture).is_some_and(|fact| fact.holds_reference()) {
                                    return Err(invalid());
                                }
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
                            let ty =
                                params
                                    .zip(signature.result.clone())
                                    .map(|(params, result)| Ty::Function {
                                        params,
                                        result: Box::new(result),
                                    });
                            produced = Some((*dst, ty.map(Fact::typed).unwrap_or_default()));
                        }
                        I::BinaryOperands { dst, op, lhs, rhs } => {
                            produced = Some((
                                *dst,
                                scalar::binary(function, *op, *lhs, *rhs, &get, &invalid)?,
                            ));
                        }
                        I::PredicateBranch { op, lhs, rhs, .. } => {
                            discarded =
                                Some(scalar::binary(function, *op, *lhs, *rhs, &get, &invalid)?);
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
                            arguments,
                        } => {
                            if get(*value).is_some_and(|value| value.holds_reference()) {
                                return Err(invalid());
                            }
                            let owner = owner(*slot).ok_or_else(invalid)?;
                            let linked = &owner.interface_tables[implementation.index()];
                            let table = owner
                                .public_items
                                .iter()
                                .find_map(|item| match item {
                                    PublicItem::InterfaceTable(table)
                                        if table.declaration == linked.declaration =>
                                    {
                                        table.instantiate(
                                            arguments,
                                            function
                                                .metadata
                                                .semantic
                                                .generic
                                                .as_ref()
                                                .map_or(&[][..], |body| body.parameters.as_slice()),
                                            definitions,
                                        )
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
                                && !flows(&value, &Ty::Trait(source.clone()))
                            {
                                return Err(invalid());
                            }
                            produced = Some((*dst, Fact::typed(Ty::Trait(target.clone()))));
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
                            produced = Some((
                                *dst,
                                scalar::numeric(
                                    *operation,
                                    ScalarOperand::Slot(*lhs),
                                    rhs.map(ScalarOperand::Slot),
                                    &get,
                                    &invalid,
                                )?,
                            ));
                        }
                        I::SetPath { path, value, .. } => {
                            let record = &module.paths[path.index()];
                            let ty = path_contracts
                                .iter()
                                .map(|(_, contract)| contract)
                                .find(|p| {
                                    p.fingerprint(definitions).ok()
                                        == Some(record.contract_fingerprint)
                                })
                                .map(|p| p.result.clone());
                            if let (Some(value), Some(expected)) = (get(*value), ty)
                                && !flows(&value, &expected)
                            {
                                return Err(invalid());
                            }
                        }
                        I::ReadPath { dst, path, .. } => {
                            let record = &module.paths[path.index()];
                            let ty = path_contracts
                                .iter()
                                .map(|(_, contract)| contract)
                                .find(|p| {
                                    p.fingerprint(definitions).ok()
                                        == Some(record.contract_fingerprint)
                                })
                                .map(|p| p.result.clone());
                            produced = Some((*dst, ty.map(Fact::typed).unwrap_or_default()));
                        }
                        I::MakeCell { dst, value } => {
                            if let Some(value) = get(*value) {
                                if value.holds_reference() {
                                    return Err(invalid());
                                }
                                produced = Some((*dst, value));
                            }
                        }
                        I::ReadCell { dst, cell } => {
                            if let Some(value) = get(*cell) {
                                produced = Some((*dst, value));
                            }
                        }
                        I::WriteCell { cell, value } => {
                            if get(*value).is_some_and(|value| value.holds_reference()) {
                                return Err(invalid());
                            }
                            if let (Some(cell), Some(value)) = (get(*cell), get(*value))
                                && let Some(ty) = cell.ty
                                && !flows(&value, &ty)
                            {
                                return Err(invalid());
                            }
                        }
                        I::Call { dst, callee, args } => {
                            if args.iter().any(|r| get(*r).is_none()) {
                                return Ok(());
                            }
                            let facts = args.iter().map(|r| get(*r).unwrap()).collect::<Vec<_>>();
                            let mut result = Fact::default();
                            match callee {
                                CallTarget::RuntimePrimitive(intrinsic) => {
                                    if *intrinsic == S::StringPartsJoin {
                                        if facts[0].ty.as_deref().is_some_and(|ty| {
                                            !matches!(ty,
                                    Ty::Array(item) if **item == Ty::Builtin(B::String))
                                        }) {
                                            return Err(invalid());
                                        }
                                        result = Fact::typed(Ty::Builtin(B::String));
                                    } else if matches!(intrinsic, S::ValueDebug | S::ValueDisplay) {
                                        result = Fact::typed(Ty::Builtin(B::String));
                                    }
                                }
                                CallTarget::Shared { contract, .. } => {
                                    for (index, (value, ty)) in
                                        facts.iter().zip(&contract.signature.params).enumerate()
                                    {
                                        if !call_argument_flows(
                                            value,
                                            ty,
                                            contract.receiver_access.filter(|_| index == 0),
                                            function.metadata.slots[args[index].index()],
                                        ) {
                                            return Err(invalid());
                                        }
                                    }
                                    result = Fact::typed(contract.signature.result.clone());
                                }
                                CallTarget::Function(target)
                                | CallTarget::ModuleFunction {
                                    function: target, ..
                                } => {
                                    let target_module =
                                        if let CallTarget::ModuleFunction { module: slot, .. } =
                                            callee
                                        {
                                            owner(*slot).ok_or_else(invalid)?
                                        } else {
                                            module
                                        };
                                    let target =
                                        &target_module.functions[target.index()].metadata.semantic;
                                    for (index, value) in facts.iter().enumerate() {
                                        if let Some(ty) = target.params.get(&index) {
                                            if !flows(value, ty) {
                                                return Err(invalid());
                                            }
                                        } else if value.access == Some(Access::ReadOnly)
                                            || value.holds_reference()
                                        {
                                            return Err(invalid());
                                        }
                                    }
                                    result =
                                        target.result.clone().map(Fact::typed).unwrap_or_default();
                                }
                                CallTarget::InterfaceMethod {
                                    module: slot,
                                    contract,
                                } => {
                                    let interface = &contract.interface;
                                    let owner = owner(*slot).ok_or_else(invalid)?;
                                    let signature = contract
                                        .signature_in(
                                            &owner.identity,
                                            &owner.public_items,
                                            &owner.trait_contracts,
                                            &Default::default(),
                                            definitions,
                                        )
                                        .map_err(|_| invalid())?;
                                    let params = signature.params;
                                    let output = signature.result;
                                    if contract.receiver.is_none() {
                                        if let Some(Ty::Trait(actual)) = facts[0].ty.as_deref() {
                                            let modules: Vec<_> = program.map_or_else(
                                                || vec![module],
                                                |program| program.modules.iter().collect(),
                                            );
                                            if !trait_bounds::interface_views(
                                                actual,
                                                &Ty::Trait(actual.clone()),
                                                &modules,
                                                definitions,
                                            )
                                            .is_some_and(|parents| parents.contains(interface))
                                            {
                                                return Err(invalid());
                                            }
                                        } else {
                                            // Boxed dispatch requires a checked interface value for
                                            // every trait, independently of storage or library roles.
                                            return Err(invalid());
                                        }
                                    }
                                    let skip = usize::from(contract.receiver.is_none());
                                    for (value, expected) in
                                        facts.iter().skip(skip).zip(params.iter().skip(skip))
                                    {
                                        if !flows(value, expected) {
                                            return Err(invalid());
                                        }
                                    }
                                    result = Fact::typed(output);
                                }
                                CallTarget::Native(target) => {
                                    let target = &module.native_imports[target.index()];
                                    for (index, (value, parameter)) in
                                        facts.iter().zip(&target.signature.params).enumerate()
                                    {
                                        if !call_argument_flows(
                                            value,
                                            parameter,
                                            target.receiver_access.filter(|_| index == 0),
                                            function.metadata.slots[args[index].index()],
                                        ) {
                                            return Err(invalid());
                                        }
                                    }
                                    result = Fact::typed(target.signature.result.clone());
                                }

                                CallTarget::RuntimeHelper(
                                    RuntimeHelper::ReflectGetField(name)
                                    | RuntimeHelper::ReflectSetField(name),
                                ) => {
                                    if let Some(Ty::Struct(nominal)) = facts[0].ty.as_deref()
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
                                            CallTarget::RuntimeHelper(
                                                RuntimeHelper::ReflectSetField(_)
                                            )
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
                                    if let Some(Ty::Array(item)) = facts[0].ty.as_deref()
                                        && !flows(&facts[2], item)
                                    {
                                        return Err(invalid());
                                    }
                                    result = facts[0].clone();
                                }
                                CallTarget::ClosureRegister { register, .. } => {
                                    if let Some(Ty::Function {
                                        params,
                                        result: output,
                                    }) =
                                        get(*register).and_then(|fact| fact.ty.as_deref().cloned())
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
                            } else {
                                discarded = Some(result);
                            }
                        }
                        I::Return(Some(value)) => {
                            if get(*value).is_some_and(|value| value.holds_reference()) {
                                return Err(invalid());
                            }
                            if let (Some(value), Some(expected)) =
                                (get(*value), semantic.result.as_ref())
                                && !flows(&value, expected)
                            {
                                return Err(invalid());
                            }
                        }
                        _ => {}
                    }
                    Ok(())
                })();
                if let Some(mut fact) = discarded {
                    types.admit(&mut fact)?;
                }
                if validate {
                    checked?;
                }
                let constant = match instruction {
                    I::LoadConst { constant, .. } => match &module.constants[constant.index()] {
                        ConstantOperand::I32(n) => usize::try_from(*n).ok(),
                        ConstantOperand::I64(n) => usize::try_from(*n).ok(),
                        ConstantOperand::U64(n) => usize::try_from(*n).ok(),
                        _ => None,
                    },
                    I::Move { src, .. } => constants[src.index()],
                    _ => None,
                };
                if let Some(dst) = instruction.slot_output() {
                    registers[dst.index()] = Some(Fact::default());
                    constants[dst.index()] = constant;
                }
                if let Some((dst, mut fact)) = produced {
                    types.admit(&mut fact)?;
                    if let Some(expected) = semantic.slots.get(&dst.index()) {
                        if !flows(&fact, expected) && validate {
                            return Err(invalid());
                        }
                        fact = declared[&dst.index()].clone();
                    }
                    registers[dst.index()] = Some(fact);
                }
            }
            let output = (registers, constants);
            changed |= !computed[block_index] || outputs[block_index] != output;
            outputs[block_index] = output;
            computed[block_index] = true;
        }
        if validate {
            return Ok(());
        }
        if !changed {
            validate = true;
        }
    }
}
