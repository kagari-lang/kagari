use crate::MirFunction;
use crate::debug::{SourceOrigin, SourcePosition};
use crate::verify::{Context, MirVerificationError, MirVerificationErrorKind};
use std::mem;

pub(super) fn verify(
    function: &MirFunction,
    context: Context<'_>,
) -> Result<(), MirVerificationError> {
    if let Some(origin) = &function.debug.source {
        verify_origin(origin, context)?;
    }
    for span in function.source_spans() {
        context.check_cancel()?;
        if span.start > span.end
            || function.debug.source.as_ref().is_some_and(|origin| {
                span.end > origin.byte_len
                    || origin.position(span.start).is_none()
                    || origin.position(span.end).is_none()
            })
        {
            return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
        }
    }
    Ok(())
}

fn verify_origin(origin: &SourceOrigin, context: Context<'_>) -> Result<(), MirVerificationError> {
    context.limit(
        origin
            .positions
            .len()
            .saturating_mul(mem::size_of::<SourcePosition>())
            .saturating_add(origin.uri.len()),
        64 * 1024 * 1024,
        "source origin bytes",
    )?;
    if origin.positions.first()
        != Some(&SourcePosition {
            offset: 0,
            line: Some(1),
            column: Some(1),
        })
    {
        return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
    }
    let mut previous: Option<&SourcePosition> = None;
    for position in &origin.positions {
        context.check_cancel()?;
        if position.offset > origin.byte_len
            || position.line == Some(0)
            || position.column == Some(0)
            || position
                .line
                .is_some_and(|line| u64::from(line).saturating_sub(1) > position.offset as u64)
            || position
                .column
                .is_some_and(|column| u64::from(column).saturating_sub(1) > position.offset as u64)
            || previous.is_some_and(|previous| !ordered(previous, position))
        {
            return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
        }
        previous = Some(position);
    }
    Ok(())
}

fn ordered(previous: &SourcePosition, next: &SourcePosition) -> bool {
    let Some(distance) = next
        .offset
        .checked_sub(previous.offset)
        .filter(|distance| *distance > 0)
    else {
        return false;
    };
    let (Some(first_line), Some(last_line)) = (previous.line, next.line) else {
        return true;
    };
    if last_line < first_line || u64::from(last_line - first_line) > distance as u64 {
        return false;
    }
    match (previous.column, next.column) {
        (Some(first), Some(last)) if first_line == last_line => {
            u64::from(first).checked_add(distance as u64) == Some(u64::from(last))
        }
        (_, Some(last)) if last_line > first_line => u64::from(last) <= distance as u64,
        _ => true,
    }
}
