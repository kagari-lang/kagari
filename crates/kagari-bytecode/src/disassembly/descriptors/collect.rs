//! Collection follows descriptor edges, never executable call-graph edges.

use crate::{
    disassembly::{
        descriptors::{Category, Reference, References},
        resolution::{Resolution, TraitSource},
    },
    instruction::{BytecodeInstruction, CallTarget, ScalarOperand},
    module::CallableTarget,
};
use kagari_contract::{callable::witness::OperationWitness, types::ConcreteFunctionIdentity};
use kagari_types::ty::Ty;

impl References {
    pub(super) fn instruction(
        &mut self,
        resolution: &Resolution<'_>,
        owner: usize,
        instruction: &BytecodeInstruction,
        pending: &mut Vec<Reference>,
    ) {
        match instruction {
            BytecodeInstruction::BinaryOperands { lhs, rhs, .. }
            | BytecodeInstruction::PredicateBranch { lhs, rhs, .. } => {
                for operand in [lhs, rhs] {
                    if let ScalarOperand::Constant { constant, .. } = operand {
                        self.insert((owner, Category::Constant, constant.index()), pending);
                    }
                }
            }
            BytecodeInstruction::LoadConst { constant, .. } => {
                self.insert((owner, Category::Constant, constant.index()), pending)
            }
            BytecodeInstruction::LoadModule { slot, .. }
            | BytecodeInstruction::StoreModule { slot, .. } => {
                self.insert((owner, Category::Slot, slot.index()), pending)
            }
            BytecodeInstruction::MakeStruct { structure, .. } => {
                self.insert((owner, Category::Struct, structure.index()), pending)
            }
            BytecodeInstruction::ReadAggregateField { field, .. }
            | BytecodeInstruction::WriteAggregateField { field, .. } => {
                self.insert((owner, Category::Struct, field.structure.index()), pending)
            }
            BytecodeInstruction::MakeEnum { enumeration, .. }
            | BytecodeInstruction::TestEnumVariant { enumeration, .. }
            | BytecodeInstruction::ReadEnumPayload { enumeration, .. } => {
                self.insert((owner, Category::Enum, enumeration.index()), pending)
            }
            BytecodeInstruction::MakeInterface {
                module,
                implementation,
                ..
            } => self.insert(
                (module.index(), Category::Interface, implementation.index()),
                pending,
            ),
            BytecodeInstruction::ReadPath { path, .. }
            | BytecodeInstruction::SetPath { path, .. }
            | BytecodeInstruction::ModifyPath { path, .. }
            | BytecodeInstruction::MakePathView { path, .. } => {
                self.insert((owner, Category::Path, path.index()), pending)
            }
            BytecodeInstruction::Call { callee, .. } => match callee {
                CallTarget::Native(import) => {
                    self.insert((owner, Category::Native, import.index()), pending)
                }
                CallTarget::Shared {
                    module,
                    target,
                    contract,
                } => {
                    self.callable(module.index(), *target, pending);
                    self.operations(resolution, &contract.operations, pending);
                }
                CallTarget::InterfaceMethod { module, contract } => {
                    let reference =
                        resolution.trait_reference(module.index(), contract.interface.declaration);
                    let category = match reference.source {
                        TraitSource::PublicItem => Category::PublicTrait,
                        TraitSource::Contract => Category::TraitContract,
                    };
                    self.insert((reference.owner, category, reference.slot), pending);
                    self.operations(resolution, &contract.operations, pending);
                }
                CallTarget::ModuleFunction { .. }
                | CallTarget::Function(_)
                | CallTarget::Slot(_)
                | CallTarget::ClosureRegister { .. }
                | CallTarget::RuntimePrimitive(_)
                | CallTarget::RuntimeHelper(_) => {}
            },
            BytecodeInstruction::Await { .. }
            | BytecodeInstruction::Convert { .. }
            | BytecodeInstruction::Numeric { .. }
            | BytecodeInstruction::Iter { .. }
            | BytecodeInstruction::Move { .. }
            | BytecodeInstruction::Unary { .. }
            | BytecodeInstruction::Binary { .. }
            | BytecodeInstruction::MakeTuple { .. }
            | BytecodeInstruction::RangeBound { .. }
            | BytecodeInstruction::MakeRange { .. }
            | BytecodeInstruction::RepeatArray { .. }
            | BytecodeInstruction::MakeArray { .. }
            | BytecodeInstruction::MakeFuture { .. }
            | BytecodeInstruction::MakeClosure { .. }
            | BytecodeInstruction::MakeCell { .. }
            | BytecodeInstruction::ReadCell { .. }
            | BytecodeInstruction::WriteCell { .. }
            | BytecodeInstruction::UpcastInterface { .. }
            | BytecodeInstruction::ReadAggregateIndex { .. }
            | BytecodeInstruction::WriteAggregateIndex { .. }
            | BytecodeInstruction::Jump { .. }
            | BytecodeInstruction::Branch { .. }
            | BytecodeInstruction::Return(_)
            | BytecodeInstruction::Unreachable => {}
        }
    }

    fn callable(&mut self, owner: usize, target: CallableTarget, pending: &mut Vec<Reference>) {
        match target {
            CallableTarget::Native(import) => {
                self.insert((owner, Category::Native, import.index()), pending)
            }
            CallableTarget::Script(_) => {}
        }
    }

    fn interface(
        &mut self,
        resolution: &Resolution<'_>,
        identity: &ConcreteFunctionIdentity,
        pending: &mut Vec<Reference>,
    ) {
        let (owner, slot) = resolution.interface(identity);
        self.insert((owner, Category::Interface, slot), pending);
    }

    fn operations(
        &mut self,
        resolution: &Resolution<'_>,
        operations: &[OperationWitness],
        pending: &mut Vec<Reference>,
    ) {
        for operation in operations.iter().flat_map(OperationWitness::walk) {
            match operation {
                OperationWitness::Selected(call) => {
                    if let Some((owner, slot)) = resolution.native(call) {
                        self.insert((owner, Category::Native, slot), pending);
                    }
                }
                OperationWitness::SharedMethod(method) => {
                    self.interface(resolution, &method.implementation, pending)
                }
                OperationWitness::SharedAdapter(_) | OperationWitness::Forward(_) => {}
            }
        }
    }

    pub(super) fn dependencies(
        &mut self,
        resolution: &Resolution<'_>,
        reference: Reference,
        pending: &mut Vec<Reference>,
    ) {
        let (owner, category, slot) = reference;
        let module = &resolution.program.modules[owner];
        match category {
            Category::Interface => {
                let table = &module.interface_tables[slot];
                for method in &table.methods {
                    self.callable(owner, method.target, pending);
                }
                for parent in &table.parents {
                    self.interface(resolution, &parent.implementation, pending);
                }
                if let Some(view) = &table.view {
                    for adapter in &view.results {
                        self.interface(resolution, &adapter.implementation, pending);
                    }
                }
            }
            Category::Native => {
                let import = &module.native_imports[slot];
                self.operations(resolution, &import.callables, pending);
                if let Some(adapter) = &import.result_adapter {
                    self.interface(resolution, &adapter.implementation, pending);
                }
            }
            Category::Struct => {
                for field in &module.structures[slot].fields {
                    self.layout_types(resolution, owner, &field.ty, pending);
                }
            }
            Category::Enum => {
                for variant in &module.enumerations[slot].variants {
                    for ty in &variant.payload {
                        self.layout_types(resolution, owner, ty, pending);
                    }
                }
            }
            Category::Constant
            | Category::Path
            | Category::Slot
            | Category::PublicTrait
            | Category::TraitContract => {}
        }
    }

    fn layout_types(
        &mut self,
        resolution: &Resolution<'_>,
        owner: usize,
        ty: &Ty,
        pending: &mut Vec<Reference>,
    ) {
        let mut types = vec![ty];
        while let Some(ty) = types.pop() {
            match ty {
                Ty::Struct(nominal) | Ty::Enum(nominal) => {
                    let (category, found) = match ty {
                        Ty::Struct(_) => (Category::Struct, resolution.structure(owner, nominal)),
                        Ty::Enum(_) => (Category::Enum, resolution.enumeration(owner, nominal)),
                        _ => unreachable!("nominal aggregate"),
                    };
                    // A semantic type need not have a materialized executable layout.
                    if let Some((owner, slot)) = found {
                        self.insert((owner, category, slot), pending);
                    }
                    types.extend(&nominal.arguments);
                    types.extend(nominal.associated_types.values());
                }
                Ty::NativeObject(nominal) | Ty::Trait(nominal) => {
                    types.extend(&nominal.arguments);
                    types.extend(nominal.associated_types.values());
                }
                Ty::Projection {
                    receiver,
                    interface,
                    arguments,
                    ..
                } => {
                    types.push(receiver);
                    types.extend(&interface.arguments);
                    types.extend(interface.associated_types.values());
                    types.extend(arguments);
                }
                Ty::Tuple(items) => types.extend(items),
                Ty::Function { params, result } => {
                    types.extend(params);
                    types.push(result);
                }
                Ty::Iter(item) | Ty::Range(item, _) | Ty::Array(item) | Ty::Set(item, _) => {
                    types.push(item)
                }
                Ty::Map { key, value, .. } => types.extend([key.as_ref(), value.as_ref()]),
                Ty::Reference { target, .. } => types.push(target),
                Ty::Host(_) | Ty::SelfType(_) | Ty::Parameter { .. } | Ty::Builtin(_) => {}
            }
        }
    }
}
