//! Stable bottom-up merging advances through explicit charged control steps.
use crate::{
    error::RuntimeError,
    gc::RootedValue,
    native::{NativeAction, NativeCallback, NativeContext, NativeInvocationState},
    native_value::{
        NativeResult, NativeValue,
        array::NativeArray,
        continuation::{NativeContinuation, NativeFn},
        reorder::NativeReorder,
        selected::NativeSelected,
    },
    value::Value,
};
use std::cmp::Ordering;

/// Both callable forms enter the same validated, rooted callback driver.
pub trait NativeComparison<T: NativeValue>: 'static {
    fn request(
        &self,
        context: &NativeContext<'_>,
        arguments: (T, T),
    ) -> NativeResult<NativeCallback>;
    fn result(&self, context: &NativeContext<'_>, value: Value) -> NativeResult<Ordering>;
}
impl<T: NativeValue> NativeComparison<T> for NativeFn<(T, T), Ordering> {
    fn request(
        &self,
        context: &NativeContext<'_>,
        arguments: (T, T),
    ) -> NativeResult<NativeCallback> {
        self.request(context, arguments)
    }
    fn result(&self, context: &NativeContext<'_>, value: Value) -> NativeResult<Ordering> {
        self.result(context, value)
    }
}
impl<T: NativeValue> NativeComparison<T> for NativeSelected<(T, T), Ordering> {
    fn request(
        &self,
        context: &NativeContext<'_>,
        arguments: (T, T),
    ) -> NativeResult<NativeCallback> {
        self.request(context, arguments)
    }
    fn result(&self, context: &NativeContext<'_>, value: Value) -> NativeResult<Ordering> {
        self.result(context, value)
    }
}
/// Prepare every comparison before changing the shared array's slots/order.
pub fn stable_sort<T: NativeValue, C: NativeComparison<T>>(
    array: &NativeArray<T>,
    compare: C,
) -> NativeResult<NativeContinuation<()>> {
    Ok(NativeContinuation::new(Sorting::start(
        array.prepare_reorder()?,
        compare,
    )))
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native sorting state mismatch")
}
#[derive(Clone, Copy)]
enum Bound {
    Middle,
    End,
    Width,
}
#[derive(Clone, Copy)]
enum BoundPhase {
    Remaining,
    Short,
    Branch,
    Add,
    Move,
    Jump,
}
#[derive(Clone, Copy)]
enum Phase {
    Values,
    Length,
    Iterator,
    Begin,
    FillIndex,
    FillJump,
    FillMore,
    FillBranch,
    FillRead,
    FillAppend,
    FillOne,
    FillAdd,
    FillMove,
    Width,
    WidthJump,
    WidthMore,
    WidthBranch,
    Output,
    Start,
    StartJump,
    StartMore,
    StartBranch,
    Bound(Bound, BoundPhase),
    LeftStart,
    RightStart,
    MergeJump,
    LeftMore,
    RightMore,
    AvailableBranch,
    AvailableTrue,
    AvailableMove,
    AvailableJump,
    MergeBranch,
    LeftAvailable,
    LeftAvailableBranch,
    RightDone,
    RightDoneBranch,
    TakeTrue,
    CompareLeft,
    CompareRight,
    Compare,
    WaitingComparison,
    Greater,
    False,
    NotGreater,
    RightChoiceMove,
    RightChoiceJump,
    TakeFalse,
    OuterChoiceMove,
    OuterChoiceJump,
    SelectedBranch,
    SelectedRead,
    SelectedOne,
    SelectedAdd,
    SelectedIndex,
    SelectedMove,
    SelectedJump,
    Append,
    StartMove,
    ValuesMove,
    WidthMove,
    Close,
    EndIteration,
    EndMutation,
    Commit,
}
struct Sorting<T: NativeValue, C: NativeComparison<T>> {
    preparation: Option<NativeReorder<T>>,
    compare: C,
    first: Option<T>,
    second: Option<T>,
    selected: Option<T>,
    ordering: Ordering,
    ordering_root: Option<RootedValue>,
    phase: Phase,
    length: u64,
    index: u64,
    next: u64,
    width: u64,
    start: u64,
    middle: u64,
    end: u64,
    left: u64,
    right: u64,
    bounded: u64,
    condition: bool,
    right_more: bool,
    take_left: bool,
}
impl<T: NativeValue, C: NativeComparison<T>> Sorting<T, C> {
    fn start(preparation: NativeReorder<T>, compare: C) -> Self {
        Self {
            preparation: Some(preparation),
            compare,
            first: None,
            second: None,
            selected: None,
            ordering: Ordering::Equal,
            ordering_root: None,
            phase: Phase::Values,
            length: 0,
            index: 0,
            next: 0,
            width: 0,
            start: 0,
            middle: 0,
            end: 0,
            left: 0,
            right: 0,
            bounded: 0,
            condition: false,
            right_more: false,
            take_left: false,
        }
    }
    fn bound_start(&self, bound: Bound) -> u64 {
        match bound {
            Bound::Middle => self.start,
            Bound::End => self.middle,
            Bound::Width => self.width,
        }
    }
    fn bound(&mut self, bound: Bound, phase: BoundPhase) -> Result<(), RuntimeError> {
        self.phase = Phase::Bound(
            bound,
            match phase {
                BoundPhase::Remaining => {
                    self.bounded = self
                        .length
                        .checked_sub(self.bound_start(bound))
                        .ok_or_else(invalid)?;
                    BoundPhase::Short
                }
                BoundPhase::Short => {
                    self.condition = self.bounded < self.width;
                    BoundPhase::Branch
                }
                BoundPhase::Branch => {
                    if self.condition {
                        self.bounded = self.length;
                        BoundPhase::Move
                    } else {
                        BoundPhase::Add
                    }
                }
                BoundPhase::Add => {
                    self.bounded = self
                        .bound_start(bound)
                        .checked_add(self.width)
                        .ok_or_else(invalid)?;
                    BoundPhase::Move
                }
                BoundPhase::Move => {
                    match bound {
                        Bound::Middle => self.middle = self.bounded,
                        Bound::End => self.end = self.bounded,
                        Bound::Width => self.next = self.bounded,
                    };
                    BoundPhase::Jump
                }
                BoundPhase::Jump => {
                    self.phase = match bound {
                        Bound::Middle => Phase::Bound(Bound::End, BoundPhase::Remaining),
                        Bound::End => Phase::LeftStart,
                        Bound::Width => Phase::WidthMove,
                    };
                    return Ok(());
                }
            },
        );
        Ok(())
    }
    fn advance_index(&mut self) -> Result<(), RuntimeError> {
        self.next = self.index.checked_add(1).ok_or_else(invalid)?;
        Ok(())
    }
    fn preparation(&self) -> NativeResult<&NativeReorder<T>> {
        self.preparation.as_ref().ok_or_else(invalid)
    }
    fn preparation_mut(&mut self) -> NativeResult<&mut NativeReorder<T>> {
        self.preparation.as_mut().ok_or_else(invalid)
    }
    fn index(value: u64) -> NativeResult<usize> {
        usize::try_from(value).map_err(|_| invalid())
    }
}
impl<T: NativeValue, C: NativeComparison<T>> NativeInvocationState for Sorting<T, C> {
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> NativeResult<NativeAction> {
        if !matches!(self.phase, Phase::WaitingComparison) {
            return Err(invalid());
        }
        self.ordering = self.compare.result(context, value.clone())?;
        self.ordering_root = Some(context.heap().root_value(value).ok_or_else(invalid)?);
        self.phase = Phase::Greater;
        Ok(NativeAction::Continue)
    }
    fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
        match self.phase {
            Phase::Values => {
                self.preparation()?.allocate_input()?;
                self.phase = Phase::Length;
            }
            Phase::Length => {
                self.length = self.preparation()?.len() as u64;
                self.phase = Phase::Iterator;
            }
            Phase::Iterator => {
                self.preparation()?.create_iterator(context)?;
                self.phase = Phase::Begin;
            }
            Phase::Begin => {
                self.preparation_mut()?.begin_iteration()?;
                self.phase = Phase::FillIndex;
            }
            Phase::FillIndex => {
                self.index = 0;
                self.phase = Phase::FillJump;
            }
            Phase::FillJump => self.phase = Phase::FillMore,
            Phase::FillMore => {
                self.condition = self.index < self.length;
                self.phase = Phase::FillBranch;
            }
            Phase::FillBranch => {
                self.phase = if self.condition {
                    Phase::FillRead
                } else {
                    Phase::Width
                }
            }
            Phase::FillRead => {
                self.first = Some(
                    self.preparation()?
                        .read_original(Self::index(self.index)?)?,
                );
                self.phase = Phase::FillAppend;
            }
            Phase::FillAppend => {
                let value = self.first.take().ok_or_else(invalid)?;
                self.preparation()?.push_input(value)?;
                self.phase = Phase::FillOne;
            }
            Phase::FillOne => self.phase = Phase::FillAdd,
            Phase::FillAdd => {
                self.advance_index()?;
                self.phase = Phase::FillMove;
            }
            Phase::FillMove => {
                self.index = self.next;
                self.phase = Phase::FillJump;
            }
            Phase::Width => {
                self.width = 1;
                self.phase = Phase::WidthJump;
            }
            Phase::WidthJump => self.phase = Phase::WidthMore,
            Phase::WidthMore => {
                self.condition = self.width < self.length;
                self.phase = Phase::WidthBranch;
            }
            Phase::WidthBranch => {
                self.phase = if self.condition {
                    Phase::Output
                } else {
                    Phase::Close
                }
            }
            Phase::Output => {
                self.preparation()?.allocate_output()?;
                self.phase = Phase::Start;
            }
            Phase::Start => {
                self.start = 0;
                self.phase = Phase::StartJump;
            }
            Phase::StartJump => self.phase = Phase::StartMore,
            Phase::StartMore => {
                self.condition = self.start < self.length;
                self.phase = Phase::StartBranch;
            }
            Phase::StartBranch => {
                self.phase = if self.condition {
                    Phase::Bound(Bound::Middle, BoundPhase::Remaining)
                } else {
                    Phase::ValuesMove
                }
            }
            Phase::Bound(bound, phase) => self.bound(bound, phase)?,
            Phase::LeftStart => {
                self.left = self.start;
                self.phase = Phase::RightStart;
            }
            Phase::RightStart => {
                self.right = self.middle;
                self.phase = Phase::MergeJump;
            }
            Phase::MergeJump => self.phase = Phase::LeftMore,
            Phase::LeftMore => {
                self.condition = self.left < self.middle;
                self.phase = Phase::RightMore;
            }
            Phase::RightMore => {
                self.right_more = self.right < self.end;
                self.phase = Phase::AvailableBranch;
            }
            Phase::AvailableBranch => {
                self.phase = if self.condition {
                    Phase::AvailableTrue
                } else {
                    self.condition = self.right_more;
                    Phase::AvailableMove
                };
            }
            Phase::AvailableTrue => {
                self.condition = true;
                self.phase = Phase::AvailableMove;
            }
            Phase::AvailableMove => self.phase = Phase::AvailableJump,
            Phase::AvailableJump => self.phase = Phase::MergeBranch,
            Phase::MergeBranch => {
                self.phase = if self.condition {
                    Phase::LeftAvailable
                } else {
                    Phase::StartMove
                }
            }
            Phase::LeftAvailable => {
                self.condition = self.left < self.middle;
                self.phase = Phase::LeftAvailableBranch;
            }
            Phase::LeftAvailableBranch => {
                self.phase = if self.condition {
                    Phase::RightDone
                } else {
                    Phase::TakeFalse
                }
            }
            Phase::RightDone => {
                self.condition = self.right == self.end;
                self.phase = Phase::RightDoneBranch;
            }
            Phase::RightDoneBranch => {
                self.phase = if self.condition {
                    Phase::TakeTrue
                } else {
                    Phase::CompareLeft
                }
            }
            Phase::TakeTrue => {
                self.take_left = true;
                self.phase = Phase::RightChoiceMove;
            }
            Phase::CompareLeft => {
                self.first = Some(self.preparation()?.read_input(Self::index(self.left)?)?);
                self.phase = Phase::CompareRight;
            }
            Phase::CompareRight => {
                self.second = Some(self.preparation()?.read_input(Self::index(self.right)?)?);
                self.phase = Phase::Compare;
            }
            Phase::Compare => {
                let request = self.compare.request(
                    context,
                    (
                        self.first.take().ok_or_else(invalid)?,
                        self.second.take().ok_or_else(invalid)?,
                    ),
                )?;
                self.phase = Phase::WaitingComparison;
                return Ok(NativeAction::Callback(request));
            }
            Phase::Greater => {
                self.condition = self.ordering == Ordering::Greater;
                self.phase = Phase::False;
            }
            Phase::False => self.phase = Phase::NotGreater,
            Phase::NotGreater => {
                self.take_left = !self.condition;
                self.phase = Phase::RightChoiceMove;
            }
            Phase::RightChoiceMove => self.phase = Phase::RightChoiceJump,
            Phase::RightChoiceJump => self.phase = Phase::OuterChoiceMove,
            Phase::TakeFalse => {
                self.take_left = false;
                self.phase = Phase::OuterChoiceMove;
            }
            Phase::OuterChoiceMove => self.phase = Phase::OuterChoiceJump,
            Phase::OuterChoiceJump => self.phase = Phase::SelectedBranch,
            Phase::SelectedBranch => self.phase = Phase::SelectedRead,
            Phase::SelectedRead => {
                self.ordering_root.take();
                self.selected = Some(self.preparation()?.read_input(Self::index(
                    if self.take_left {
                        self.left
                    } else {
                        self.right
                    },
                )?)?);
                self.phase = Phase::SelectedOne;
            }
            Phase::SelectedOne => self.phase = Phase::SelectedAdd,
            Phase::SelectedAdd => {
                self.next = if self.take_left {
                    self.left
                } else {
                    self.right
                }
                .checked_add(1)
                .ok_or_else(invalid)?;
                self.phase = Phase::SelectedIndex;
            }
            Phase::SelectedIndex => {
                if self.take_left {
                    self.left = self.next
                } else {
                    self.right = self.next
                }
                self.phase = Phase::SelectedMove;
            }
            Phase::SelectedMove => self.phase = Phase::SelectedJump,
            Phase::SelectedJump => self.phase = Phase::Append,
            Phase::Append => {
                let value = self.selected.take().ok_or_else(invalid)?;
                self.preparation()?.push_output(value)?;
                self.phase = Phase::MergeJump;
            }
            Phase::StartMove => {
                self.start = self.end;
                self.phase = Phase::StartJump;
            }
            Phase::ValuesMove => {
                self.preparation()?.publish_output()?;
                self.phase = Phase::Bound(Bound::Width, BoundPhase::Remaining);
            }
            Phase::WidthMove => {
                self.width = self.next;
                self.phase = Phase::WidthJump;
            }
            Phase::Close => {
                self.preparation()?.close_iterator(context)?;
                self.phase = Phase::EndIteration;
            }
            Phase::EndIteration => {
                self.preparation_mut()?.end_iteration();
                self.phase = Phase::EndMutation;
            }
            Phase::EndMutation => {
                self.preparation_mut()?.end_mutation();
                self.phase = Phase::Commit;
            }
            Phase::Commit => {
                self.preparation.take().ok_or_else(invalid)?.commit()?;
                return Ok(NativeAction::Complete(Value::Unit));
            }
            Phase::WaitingComparison => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
