use crate::passes::scalar::Scalar;
use crate::passes::{PassStatistics, Work};
use crate::{Instruction, MirModule, MirVerificationError, Terminator};

pub(super) fn simplify(
    module: &mut MirModule,
    statistics: &mut PassStatistics,
    work: &mut Work<'_>,
) -> Result<(), MirVerificationError> {
    for function in &mut module.functions {
        for block in &mut function.blocks {
            work.charge(function.temps.len())?;
            let mut values = vec![None; function.temps.len()];
            for instruction in &mut block.instructions {
                work.charge(1)?;
                let value = evaluate(instruction, &values);
                if let Some(dst) = instruction.output() {
                    let value = value.filter(|value| {
                        value.fits(function.semantic.registers.get(&dst.temp.index()))
                    });
                    values[dst.temp.index()] = value;
                    if let Some(value) = value
                        && !matches!(instruction, Instruction::LoadConst { .. })
                    {
                        *instruction = Instruction::LoadConst {
                            dst,
                            constant: value.constant(),
                        };
                        statistics.constants_folded += 1;
                    }
                }
                if instruction.effects().calls {
                    // Calls may synchronously reenter host/runtime execution. No
                    // local propagation relies on facts carried across that boundary.
                    work.charge(values.len())?;
                    values.fill(None);
                }
            }
            work.charge(1)?;
            if let Some(Terminator::Branch {
                cond,
                then_block,
                else_block,
            }) = &block.terminator
                && let Some(Scalar::Bool(condition)) = values[cond.temp.index()]
            {
                block.terminator = Some(Terminator::Jump(if condition {
                    *then_block
                } else {
                    *else_block
                }));
                statistics.branches_simplified += 1;
            }
        }
    }
    Ok(())
}

fn evaluate(instruction: &Instruction, values: &[Option<Scalar>]) -> Option<Scalar> {
    match instruction {
        Instruction::LoadConst { constant, .. } => Scalar::from_constant(constant),
        Instruction::Move { src, .. } => values[src.temp.index()],
        Instruction::Unary { op, operand, .. } => values[operand.temp.index()]?.unary(*op),
        Instruction::Binary { op, lhs, rhs, .. } => {
            values[lhs.temp.index()]?.binary(*op, values[rhs.temp.index()]?)
        }
        Instruction::Numeric {
            operation,
            lhs,
            rhs,
            ..
        } => values[lhs.temp.index()]?.numeric(
            *operation,
            match rhs {
                Some(rhs) => Some(values[rhs.temp.index()]?),
                None => None,
            },
        ),
        _ => None,
    }
}
