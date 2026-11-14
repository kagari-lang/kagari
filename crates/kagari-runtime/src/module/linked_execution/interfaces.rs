//! Closed interface conversion facts, cached at their exact function/environment.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::environments::EnvironmentId,
    frame::types::{TypeEnvironment, arguments::TypeArgument},
    module::{ExecutableModule, execution::interfaces::PreparedUpcast},
};
use kagari_types::ty::{NominalTy, Ty};
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
pub(crate) struct ClosedUpcast {
    source: TypeArgument,
    target: TypeArgument,
}

impl ClosedUpcast {
    pub(crate) fn source(&self) -> &NominalTy {
        let Ty::Trait(nominal) = self.source.ty() else {
            unreachable!("checked upcast source")
        };
        nominal
    }

    pub(crate) fn target(&self) -> &NominalTy {
        let Ty::Trait(nominal) = self.target.ty() else {
            unreachable!("checked upcast target")
        };
        nominal
    }
}

#[derive(Debug)]
enum UpcastBinding {
    Closed(ClosedUpcast),
    Scoped(usize),
}

#[derive(Debug)]
pub(crate) struct FunctionUpcasts {
    entries: Box<[UpcastBinding]>,
    scoped: usize,
}

#[derive(Debug)]
pub(crate) struct AppliedUpcasts {
    entries: Box<[OnceLock<ClosedUpcast>]>,
    #[cfg(debug_assertions)]
    environment: EnvironmentId,
}

impl FunctionUpcasts {
    pub(super) fn link(
        runtime: &Runtime,
        owner: &ExecutableModule,
        operands: &[PreparedUpcast],
    ) -> Result<Option<Arc<Self>>, RuntimeError> {
        if operands.is_empty() {
            return Ok(None);
        }
        let mut scoped = 0;
        let entries = operands
            .iter()
            .map(|operand| {
                if operand.is_closed() {
                    prepare(runtime, owner, operand, None).map(UpcastBinding::Closed)
                } else {
                    let index = scoped;
                    scoped += 1;
                    Ok(UpcastBinding::Scoped(index))
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(Some(Arc::new(Self { entries, scoped })))
    }

    pub(super) fn is_scoped(&self) -> bool {
        self.scoped != 0
    }

    pub(super) fn application(&self, environment: EnvironmentId) -> Arc<AppliedUpcasts> {
        #[cfg(not(debug_assertions))]
        let _ = environment;
        Arc::new(AppliedUpcasts {
            entries: (0..self.scoped).map(|_| OnceLock::new()).collect(),
            #[cfg(debug_assertions)]
            environment,
        })
    }

    pub(crate) fn resolve<'a>(
        &'a self,
        runtime: &Runtime,
        owner: &ExecutableModule,
        environment: Option<&TypeEnvironment>,
        active: Option<&'a AppliedUpcasts>,
        index: usize,
        operand: &PreparedUpcast,
    ) -> Result<&'a ClosedUpcast, RuntimeError> {
        match self
            .entries
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("missing upcast binding"))?
        {
            UpcastBinding::Closed(closed) => Ok(closed),
            UpcastBinding::Scoped(index) => {
                let environment = environment
                    .ok_or_else(|| RuntimeError::module_validation("missing upcast environment"))?;
                let active = active
                    .ok_or_else(|| RuntimeError::module_validation("missing upcast application"))?;
                #[cfg(debug_assertions)]
                debug_assert_eq!(active.environment, environment.id);
                let cell = &active.entries[*index];
                if cell.get().is_none() {
                    let closed = prepare(runtime, owner, operand, Some(environment))?;
                    let _ = cell.set(closed);
                }
                Ok(cell.get().expect("published upcast facts"))
            }
        }
    }
}

fn prepare(
    runtime: &Runtime,
    owner: &ExecutableModule,
    operand: &PreparedUpcast,
    environment: Option<&TypeEnvironment>,
) -> Result<ClosedUpcast, RuntimeError> {
    let mut types = runtime.type_arguments(
        owner,
        environment.map(|environment| environment.types.clone()),
        &[operand.source.clone(), operand.target.clone()],
    )?;
    let target = types.pop().expect("two checked upcast types");
    let source = types.pop().expect("two checked upcast types");
    if !matches!((source.ty(), target.ty()), (Ty::Trait(_), Ty::Trait(_))) {
        return Err(RuntimeError::module_validation("invalid upcast type facts"));
    }
    Ok(ClosedUpcast { source, target })
}
