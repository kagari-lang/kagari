//! Authenticate supplied ordinary Script expressions at the installed-library boundary.
//!
//! Product origins and instruction certificates establish internal consistency, not
//! the semantics of an installed final method. The independent authority here is the
//! installed module's ordinary expression, never a digest supplied by the artifact.
use super::NativeRegistry;
use crate::{
    error::RuntimeError,
    module::{VerifiedProgram, admission::DefinitionAdmission},
    native::binding::NativeResult,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction as I, CallTarget, Slot},
    module::{BytecodeFunction, BytecodeModule},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionId},
};
use kagari_contract::types::{
    ConcreteFunctionIdentity, InterfaceTable, PublicItem, named::NamedContract, trait_contract,
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{FnDecl, module::ModuleDecl, script::ScriptExpr},
    scalar::BuiltinType,
    ty::{
        NominalTy, Ty,
        matching::{ImplementationPattern, match_receiver, projection_output},
        substitution::{TypeSubstitution, normalize_projections, resolve_associated_outputs},
    },
};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InstalledScript {
    function: FnDecl,
    body: ScriptExpr,
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Script executable differs from its installed canonical body")
}

impl NativeRegistry {
    pub(crate) fn install_scripts(&mut self, module: &ModuleDecl) -> NativeResult<()> {
        let definitions = self.catalog.definitions();
        let mut functions = Vec::new();
        for function in &module.functions {
            functions.push((
                module
                    .definition(DefinitionKind::Function, &function.name, &definitions)
                    .map_err(|_| invalid())?,
                function,
            ));
        }
        for contract in &module.traits {
            let owner = module
                .definition(DefinitionKind::Trait, &contract.name, &definitions)
                .map_err(|_| invalid())?;
            for function in &contract.methods {
                functions.push((
                    ModuleDecl::method_id(owner, &function.name, &definitions)
                        .map_err(|_| invalid())?,
                    function,
                ));
            }
        }
        for (index, implementation) in module.implementations.iter().enumerate() {
            let owner = module
                .implementation_id(index, &definitions)
                .map_err(|_| invalid())?;
            for function in &implementation.methods {
                functions.push((
                    ModuleDecl::method_id(owner, &function.name, &definitions)
                        .map_err(|_| invalid())?,
                    function,
                ));
            }
        }
        for (id, function) in functions {
            let Some(body) = module.script_bodies.get(&id) else {
                // Parsed Script source can be supplied without a structured body. It
                // grants no independent executable authority for a final default.
                continue;
            };
            let expected = InstalledScript {
                function: function.clone(),
                body: body.clone(),
            };
            if self
                .scripts
                .get_id(id)
                .is_some_and(|previous| previous != &expected)
            {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting canonical Script owner",
                ));
            }
            self.scripts
                .insert_id(id, expected)
                .map_err(|_| invalid())?;
        }
        Ok(())
    }

    pub(super) fn validate_scripts(
        &self,
        module: &BytecodeModule,
        program: &VerifiedProgram,
        admission: &DefinitionAdmission,
    ) -> NativeResult<()> {
        let cancel = CancellationToken::default();
        // Fail closed when a product asks for final library semantics without an
        // independently installed expression. Origins alone cannot authorize it.
        for item in &module.public_items {
            let PublicItem::InterfaceTable(table) = item else {
                continue;
            };
            let Ty::Trait(interface) = &table.trait_type else {
                return Err(invalid());
            };
            let contract = program
                .modules()
                .iter()
                .find_map(|owner| {
                    trait_contract(
                        &owner.identity,
                        &owner.public_items,
                        &owner.trait_contracts,
                        &interface.declaration,
                        program.definitions(),
                    )
                })
                .ok_or_else(invalid)?;
            for method in &contract.methods {
                if method.implementation != CallableImplementation::Script
                    || method.method_policy.override_allowed
                {
                    continue;
                }
                let canonical = program
                    .definitions()
                    .lookup_child(
                        interface.declaration,
                        DefinitionKind::Method,
                        &method.name,
                        0,
                    )
                    .ok_or_else(invalid)?;
                let installed = admission.runtime_id(canonical)?;
                if self.scripts.get_id(installed).is_none() {
                    return Err(invalid());
                }
            }
        }
        for function in &module.functions {
            let identity = function.identity.as_ref().ok_or_else(invalid)?;
            let placement = identity.declaration;
            let inherited = program
                .modules()
                .iter()
                .flat_map(|owner| &owner.public_items)
                .filter_map(|item| match item {
                    PublicItem::InterfaceTable(table) => Some(table),
                    _ => None,
                })
                .find_map(|table| {
                    table
                        .method_origins
                        .iter()
                        .find(|origin| origin.placement == placement)
                        .map(|origin| (table, origin.authored))
                });
            let canonical = inherited
                .map(|(_, id)| id)
                .or_else(|| {
                    module
                        .function_origins
                        .iter()
                        .find(|origin| origin.placement == placement)
                        .map(|origin| origin.authored)
                })
                .unwrap_or(placement);
            let installed = admission.runtime_id(canonical)?;
            let Some(script) = self.scripts.get_id(installed) else {
                continue;
            };
            // Only identities are remapped; this never consults source, HIR, a
            // solver, or compiler-produced assertions about expected instructions.
            let body = admission.product(&script.body, &cancel)?;
            let function_contract = admission.product(&script.function, &cancel)?;
            let arguments = identity
                .arguments
                .iter()
                .cloned()
                .chain(
                    function
                        .metadata
                        .semantic
                        .generic
                        .iter()
                        .flat_map(|generic| {
                            generic
                                .parameters
                                .iter()
                                .map(|parameter| parameter.as_type())
                        }),
                )
                .collect::<Vec<_>>();
            let mut bindings = TypeSubstitution::default();
            let mut applied_interface = None;
            let mut applied_receiver = None;
            if let Some((table, _)) = inherited {
                for (parameter, argument) in table.generic_params.iter().zip(&arguments) {
                    bindings.bind(&parameter.owner, parameter.position, argument);
                }
                let Ty::Trait(interface) = &table.trait_type else {
                    return Err(invalid());
                };
                applied_interface = Some(
                    bindings
                        .apply_nominal(interface, &cancel)
                        .map_err(|_| invalid())?,
                );
                applied_receiver = Some(
                    bindings
                        .apply(&table.for_type, &cancel)
                        .map_err(|_| invalid())?,
                );
            }
            let mut application = TypeSubstitution::default();
            if let Some(interface) = &applied_interface {
                for (position, argument) in interface.arguments.iter().enumerate() {
                    application.bind(&interface.declaration, position, argument);
                }
                application.bind_receiver(
                    &interface.declaration,
                    applied_receiver.as_ref().ok_or_else(invalid)?,
                );
            }
            let offset = inherited.map_or(0, |(table, _)| table.generic_params.len());
            if function_contract.generic_params.len() + offset != arguments.len() {
                return Err(invalid());
            }
            for (parameter, argument) in function_contract
                .generic_params
                .iter()
                .zip(&arguments[offset..])
            {
                application.bind(&parameter.owner, parameter.position, argument);
            }
            let interface = match inherited {
                Some((table, _)) => Some(AppliedInterface {
                    table,
                    interface: applied_interface.as_ref().ok_or_else(invalid)?,
                    receiver: applied_receiver.as_ref().ok_or_else(invalid)?,
                }),
                None => None,
            };
            let trace = Trace::read(module, function, program)?;
            let mut expression = Expression {
                program,
                canonical,
                function: &function_contract,
                application: &application,
                interface,
                trace: &trace,
                next: 0,
            };
            let expected_result = expression.apply_type(&function_contract.return_type)?;
            let result = expression.value(&body, Some(&expected_result))?;
            if result != trace.result || expression.next != trace.calls.len() {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Parameter(usize),
    Definition(DefinitionId),
    Result(usize),
    Unit,
}

struct AppliedCall {
    callee: Value,
    arguments: Vec<Value>,
    types: Vec<Ty>,
    result: Value,
    signature: Signature,
}

struct Trace {
    calls: Vec<AppliedCall>,
    result: Value,
}

fn canonical(identity: &ConcreteFunctionIdentity, program: &VerifiedProgram) -> DefinitionId {
    program
        .modules()
        .iter()
        .flat_map(|module| &module.function_origins)
        .find(|origin| origin.placement == identity.declaration)
        .map_or(identity.declaration, |origin| origin.authored)
}

impl Trace {
    // The supplied grammar contains only values and left-to-right calls. Moves
    // preserve symbolic values; every other executable effect needs a matching
    // expression, so an extra or substituted effect cannot hide in discarded code.
    fn read(
        module: &BytecodeModule,
        function: &BytecodeFunction,
        program: &VerifiedProgram,
    ) -> NativeResult<Self> {
        let mut slots = HashMap::new();
        for (index, slot) in function.metadata.parameter_slots.iter().enumerate() {
            slots.insert(*slot, Value::Parameter(index));
        }
        let mut calls = Vec::new();
        let get = |slots: &HashMap<Slot, Value>, slot: &Slot| {
            slots.get(slot).cloned().ok_or_else(invalid)
        };
        let mut result = None;
        for instruction in &function.instructions {
            if result.is_some() {
                return Err(invalid());
            }
            match instruction {
                I::Move { dst, src } => {
                    slots.insert(*dst, get(&slots, src)?);
                }
                I::MakeClosure {
                    dst,
                    function,
                    captures,
                } if captures.is_empty() => {
                    let identity = module
                        .functions
                        .get(function.index())
                        .and_then(|function| function.identity.as_ref())
                        .ok_or_else(invalid)?;
                    if !identity.arguments.is_empty() {
                        return Err(invalid());
                    }
                    slots.insert(*dst, Value::Definition(canonical(identity, program)));
                }
                I::Call { dst, callee, args } => {
                    let (callee, types, signature) = match callee {
                        CallTarget::Native(id) => {
                            let import =
                                module.native_imports.get(id.index()).ok_or_else(invalid)?;
                            (
                                Value::Definition(import.instance.declaration),
                                import.instance.arguments.clone(),
                                import.signature.clone(),
                            )
                        }
                        CallTarget::Shared { contract, .. } => (
                            Value::Definition(canonical(&contract.instance, program)),
                            contract.arguments.clone(),
                            contract.signature.clone(),
                        ),
                        CallTarget::Function(id) => {
                            let target = module.functions.get(id.index()).ok_or_else(invalid)?;
                            let identity = target.identity.as_ref().ok_or_else(invalid)?;
                            (
                                Value::Definition(canonical(identity, program)),
                                identity.arguments.clone(),
                                function_signature(target)?,
                            )
                        }
                        CallTarget::ModuleFunction { module, function } => {
                            let target = program
                                .modules()
                                .get(module.index())
                                .and_then(|owner| owner.functions.get(function.index()))
                                .ok_or_else(invalid)?;
                            let identity = target.identity.as_ref().ok_or_else(invalid)?;
                            (
                                Value::Definition(canonical(identity, program)),
                                identity.arguments.clone(),
                                function_signature(target)?,
                            )
                        }
                        CallTarget::Slot(slot)
                        | CallTarget::ClosureRegister { register: slot, .. } => {
                            let Ty::Function { params, result } = function
                                .metadata
                                .semantic
                                .slots
                                .get(&slot.index())
                                .ok_or_else(invalid)?
                            else {
                                return Err(invalid());
                            };
                            (
                                get(&slots, slot)?,
                                vec![],
                                Signature {
                                    params: params.clone(),
                                    result: *result.clone(),
                                },
                            )
                        }
                        _ => return Err(invalid()),
                    };
                    let arguments = args
                        .iter()
                        .map(|slot| get(&slots, slot))
                        .collect::<NativeResult<_>>()?;
                    let result = if let Some(dst) = dst {
                        let result = Value::Result(calls.len());
                        slots.insert(*dst, result.clone());
                        result
                    } else {
                        Value::Unit
                    };
                    calls.push(AppliedCall {
                        callee,
                        types,
                        arguments,
                        result,
                        signature,
                    });
                }
                I::Return(value) => {
                    result = Some(
                        value
                            .as_ref()
                            .map(|slot| get(&slots, slot))
                            .transpose()?
                            .unwrap_or(Value::Unit),
                    );
                }
                _ => return Err(invalid()),
            }
        }
        Ok(Self {
            calls,
            result: result.ok_or_else(invalid)?,
        })
    }
}

/// A selected, verified impl application, not a catalog for Runtime trait search.
struct AppliedInterface<'a> {
    table: &'a InterfaceTable,
    interface: &'a NominalTy,
    receiver: &'a Ty,
}

struct Expression<'a, 'b> {
    program: &'a VerifiedProgram,
    canonical: DefinitionId,
    function: &'a FnDecl,
    application: &'a TypeSubstitution<'b>,
    interface: Option<AppliedInterface<'a>>,
    trace: &'a Trace,
    next: usize,
}

fn function_signature(function: &BytecodeFunction) -> NativeResult<Signature> {
    let semantic = &function.metadata.semantic;
    Ok(Signature {
        params: (0..usize::from(function.parameter_count))
            .map(|index| semantic.params.get(&index).cloned().ok_or_else(invalid))
            .collect::<NativeResult<_>>()?,
        result: semantic.result.clone().ok_or_else(invalid)?,
    })
}

fn call_count(expression: &ScriptExpr) -> usize {
    match expression {
        ScriptExpr::Call {
            callee, arguments, ..
        } => 1 + call_count(callee) + arguments.iter().map(call_count).sum::<usize>(),
        _ => 0,
    }
}

impl Expression<'_, '_> {
    fn normalize_type(&self, ty: &Ty) -> NativeResult<Ty> {
        let Some(applied) = &self.interface else {
            return Ok(ty.clone());
        };
        let Ty::Trait(template) = &applied.table.trait_type else {
            return Err(invalid());
        };
        let cancel = CancellationToken::default();
        let resolved = resolve_associated_outputs(ty, applied.interface, applied.receiver, &cancel)
            .map_err(|_| invalid())?;
        // Reuse only this checked impl's outputs/families. The bounded transform
        // leaves unrelated projections symbolic and rejects cyclic expansions.
        normalize_projections(
            &resolved,
            &|interface, receiver, member, arguments| {
                projection_output(
                    ImplementationPattern {
                        parameters: &applied.table.generic_params,
                        receiver: &applied.table.for_type,
                        interface: template,
                    },
                    &applied.table.associated_type_families,
                    interface,
                    receiver,
                    member,
                    arguments,
                    &cancel,
                )
            },
            &cancel,
        )
        .map_err(|_| invalid())
    }

    fn apply_type(&self, ty: &Ty) -> NativeResult<Ty> {
        self.normalize_type(
            &self
                .application
                .apply(ty, &CancellationToken::default())
                .map_err(|_| invalid())?,
        )
    }

    fn callable(&self, id: DefinitionId) -> NativeResult<&FnDecl> {
        self.program
            .modules()
            .iter()
            .find_map(|owner| {
                owner
                    .native_declarations
                    .iter()
                    .find(|declaration| declaration.declaration == id)
                    .map(|declaration| &declaration.function)
                    .or_else(|| {
                        owner.named_declarations.iter().find_map(|declaration| {
                            if declaration.declaration != id {
                                return None;
                            }
                            match &declaration.contract {
                                NamedContract::Function(function) => Some(function),
                                _ => None,
                            }
                        })
                    })
            })
            .ok_or_else(invalid)
    }

    fn value_type(&self, value: &Value) -> NativeResult<Ty> {
        Ok(match value {
            Value::Parameter(index) => {
                self.apply_type(&self.function.params.get(*index).ok_or_else(invalid)?.ty)?
            }
            Value::Definition(id) => {
                let function = self.callable(*id)?;
                self.normalize_type(&Ty::Function {
                    params: function
                        .params
                        .iter()
                        .map(|parameter| parameter.ty.clone())
                        .collect(),
                    result: Box::new(function.return_type.clone()),
                })?
            }
            Value::Result(index) => self.normalize_type(
                &self
                    .trace
                    .calls
                    .get(*index)
                    .ok_or_else(invalid)?
                    .signature
                    .result,
            )?,
            Value::Unit => Ty::Builtin(BuiltinType::Unit),
        })
    }

    fn inferred_arguments(
        &self,
        call: &AppliedCall,
        values: &[Value],
        expected: Option<&Ty>,
    ) -> NativeResult<()> {
        let Value::Definition(id) = call.callee else {
            return Err(invalid());
        };
        let function = self.callable(id)?;
        let mut template = function
            .params
            .iter()
            .map(|parameter| self.normalize_type(&parameter.ty))
            .collect::<NativeResult<Vec<_>>>()?;
        let mut actual = values
            .iter()
            .map(|value| self.value_type(value))
            .collect::<NativeResult<Vec<_>>>()?;
        if let Some(expected) = expected {
            template.push(self.normalize_type(&function.return_type)?);
            actual.push(self.normalize_type(expected)?);
        }
        let template = Ty::Tuple(template);
        let actual = Ty::Tuple(actual);
        // Authenticate an already checked application by structural signature
        // matching, not trait search or Runtime type inference. Every omitted
        // argument must be determined by ordinary inputs or the result context.
        let bindings = match_receiver(
            &function.generic_params,
            &template,
            &actual,
            &CancellationToken::default(),
        )
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
        if function.generic_params.len() != call.types.len() {
            return Err(invalid());
        }
        for (parameter, argument) in function.generic_params.iter().zip(&call.types) {
            let argument = self.normalize_type(argument)?;
            if bindings.parameter(&parameter.owner, parameter.position) != Some(&argument) {
                return Err(invalid());
            }
        }
        Ok(())
    }

    fn value(&mut self, expression: &ScriptExpr, expected: Option<&Ty>) -> NativeResult<Value> {
        let value = match expression {
            ScriptExpr::Name(name) => {
                if let Some(index) = self
                    .function
                    .params
                    .iter()
                    .position(|parameter| &parameter.name == name)
                {
                    Value::Parameter(index)
                } else {
                    let definitions = self.program.definitions();
                    let owner = definitions.resolve(self.canonical).map_err(|_| invalid())?;
                    let root = definitions
                        .lookup_root(owner.module())
                        .ok_or_else(invalid)?;
                    Value::Definition(
                        definitions
                            .lookup_child(root, DefinitionKind::Function, name, 0)
                            .ok_or_else(invalid)?,
                    )
                }
            }
            ScriptExpr::Definition(id) => Value::Definition(*id),
            ScriptExpr::Call {
                callee,
                type_arguments,
                arguments,
            } => {
                let outer = self.next + call_count(expression) - 1;
                let signature = self
                    .trace
                    .calls
                    .get(outer)
                    .ok_or_else(invalid)?
                    .signature
                    .clone();
                if signature.params.len() != arguments.len() {
                    return Err(invalid());
                }
                let callee = self.value(callee, None)?;
                let arguments = arguments
                    .iter()
                    .zip(&signature.params)
                    .map(|(argument, parameter)| self.value(argument, Some(parameter)))
                    .collect::<NativeResult<Vec<_>>>()?;
                let types = type_arguments
                    .iter()
                    .map(|ty| self.apply_type(ty))
                    .collect::<NativeResult<Vec<_>>>()?;
                let actual = self.trace.calls.get(self.next).ok_or_else(invalid)?;
                if actual.callee != callee || actual.arguments != arguments {
                    return Err(invalid());
                }
                if types.is_empty() && !actual.types.is_empty() {
                    self.inferred_arguments(actual, &arguments, expected)?;
                } else if actual.types != types {
                    return Err(invalid());
                }
                let value = actual.result.clone();
                self.next += 1;
                value
            }
        };

        Ok(value)
    }
}
