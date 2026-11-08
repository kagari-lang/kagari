//! Library-owned interpretation of normal outputs; Runtime owns branch lifetimes.
use crate::{
    catalog::documentation::example,
    foundation::{enumeration, tasks::documentation as docs},
    identity,
};
use kagari_common::identity::DefinitionKind;
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        assembly::draft::ModuleDraft,
        binding::{Codec, NativeBinding, NativeResult},
        composition::{PairContext, PairDecision, PairPolicy},
        declarations::FunctionDecl,
        types::{Type, VariantRef},
    },
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};
use std::sync::Arc;

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    for fallible in [false, true] {
        let future = module.reference_type(
            identity::type_id("Future", DefinitionKind::AssociatedType),
            TypeDefKind::NativeStorage(NativeStorageLayout::Future),
            1,
        )?;
        let result = enumeration(module, "Result", 2)?;
        let name = if fallible { "try_join" } else { "join" };
        let docs = if fallible {
            example(
                "Combines two cold Result-producing Futures.\n\nAwaiting drives both cooperatively. Two Ok values produce an input-ordered\npair; the first observed Err closes the sibling before returning that error.\nExecution traps are not converted into Err values. Completed effects are not\nrolled back, and the error keeps its original diagnostic origin.",
                docs::TRY_JOIN_EXAMPLE,
            )
        } else {
            example(
                "Combines two cold Futures without starting them.\n\nAwaiting drives both cooperatively and returns the input-ordered pair after\nbranch cleanup. The order of outputs does not prescribe the order of effects.\nResult values are ordinary outputs; an execution trap closes the sibling and\nterminates the parent. The host drives the example's scheduler.",
                docs::JOIN_EXAMPLE,
            )
        };
        module.define_function(
            FunctionDecl::new(name)
                .documentation(docs)
                .configure(move |function| {
                    let left = function.type_parameter("A")?.ty();
                    let right = function.type_parameter("B")?.ty();
                    let pair = Type::tuple([left.clone(), right.clone()])?;
                    let (left, right, output) = if fallible {
                        let error = function.type_parameter("E")?.ty();
                        (
                            result.apply([left, error.clone()])?,
                            result.apply([right, error.clone()])?,
                            result.apply([pair, error])?,
                        )
                    } else {
                        (left, right, pair)
                    };
                    function.parameter("left", future.apply([left])?)?;
                    function.parameter("right", future.apply([right])?)?;
                    function.returns(future.apply([output])?)?;
                    Ok(())
                })
                .implementation_with_send(move |view| {
                    let policy: Arc<dyn PairPolicy> = if fallible {
                        let result = view
                            .catalog()
                            .type_reference(&identity::type_id("Result", DefinitionKind::Enum))?;
                        Arc::new(TryJoin {
                            ok: result.variant("Ok")?,
                            error: result.variant("Err")?,
                        })
                    } else {
                        Arc::new(Join)
                    };
                    Ok(NativeBinding::new(
                        vec![Codec::Value; 2],
                        Codec::Value,
                        move |cx| cx.compose_pair(policy.clone()),
                    ))
                }),
        )?;
    }
    Ok(())
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("join library type relationship")
}

fn check_pair(
    cx: &PairContext<'_>,
    output: &TypeArgument,
    inputs: [&TypeArgument; 2],
) -> NativeResult<()> {
    if !matches!(output.ty(), Ty::Tuple(elements) if elements.len() == 2) {
        return Err(invalid());
    }
    for (index, input) in inputs.into_iter().enumerate() {
        if !cx.compatible(input, &cx.parameter(output, index)?) {
            return Err(invalid());
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Join;

impl PairPolicy for Join {
    fn validate(&self, cx: &PairContext<'_>) -> NativeResult<()> {
        check_pair(cx, cx.output_type(), [cx.input_type(0)?, cx.input_type(1)?])
    }

    fn on_output(&self, cx: &PairContext<'_>, _: usize) -> NativeResult<PairDecision> {
        let (Some(left), Some(right)) = (cx.output(0)?, cx.output(1)?) else {
            return Ok(PairDecision::Continue);
        };
        cx.tuple(cx.output_type(), vec![left, right])
            .map(PairDecision::Finish)
    }
}

#[derive(Debug)]
struct TryJoin {
    ok: VariantRef,
    error: VariantRef,
}

impl PairPolicy for TryJoin {
    fn validate(&self, cx: &PairContext<'_>) -> NativeResult<()> {
        let output = cx.output_type();
        cx.check_variant(output, &self.ok)?;
        cx.check_variant(output, &self.error)?;
        let error = cx.parameter(output, 1)?;
        for index in 0..2 {
            let input = cx.input_type(index)?;
            cx.check_variant(input, &self.ok)?;
            cx.check_variant(input, &self.error)?;
            if !cx.compatible(&error, &cx.parameter(input, 1)?) {
                return Err(invalid());
            }
        }
        check_pair(
            cx,
            &cx.parameter(output, 0)?,
            [
                &cx.parameter(cx.input_type(0)?, 0)?,
                &cx.parameter(cx.input_type(1)?, 0)?,
            ],
        )
    }

    fn on_output(&self, cx: &PairContext<'_>, index: usize) -> NativeResult<PairDecision> {
        if cx.is_variant(index, &self.error)? {
            return cx
                .forward_variant(index, cx.output_type(), &self.error)
                .map(PairDecision::Finish);
        }
        if !cx.is_variant(index, &self.ok)? {
            return Err(invalid());
        }
        if cx.output(0)?.is_none() || cx.output(1)?.is_none() {
            return Ok(PairDecision::Continue);
        }
        let pair = cx.tuple(
            &cx.parameter(cx.output_type(), 0)?,
            vec![cx.field(0, &self.ok, 0)?, cx.field(1, &self.ok, 0)?],
        )?;
        cx.allocate_enum(cx.output_type(), &self.ok, vec![cx.value(&pair)?])
            .map(PairDecision::Finish)
    }
}
