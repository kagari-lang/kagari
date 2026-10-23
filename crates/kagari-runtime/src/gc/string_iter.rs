use crate::{
    error::RuntimeError,
    gc::{GcHeap, iter::PendingItem},
    value::Value,
};
use kagari_contract::operations::StringIterKind;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Cursor {
    position: usize,
    started: bool,
    done: bool,
    yielded: u64,
}

#[derive(Debug)]
pub(super) struct StringTraversal {
    kind: StringIterKind,
    separator: Option<Value>,
    limit: u64,
    pub(super) cursor: Cursor,
}

impl StringTraversal {
    pub(super) fn new(kind: StringIterKind, fields: &[Value]) -> Result<Self, RuntimeError> {
        let (limit, separator) = match (kind, fields) {
            (StringIterKind::Split, [Value::Str(_), Value::Str(separator)]) => {
                (u64::MAX, Some(Value::Str(*separator)))
            }
            (StringIterKind::SplitN, [Value::Str(_), Value::U64(count), Value::Str(separator)]) => {
                (*count, Some(Value::Str(*separator)))
            }
            (
                StringIterKind::Whitespace
                | StringIterKind::Lines
                | StringIterKind::Chars
                | StringIterKind::Bytes
                | StringIterKind::CharIndices,
                [Value::Str(_)],
            ) => (u64::MAX, None),
            _ => {
                return Err(RuntimeError::module_validation(
                    "invalid string traversal arguments",
                ));
            }
        };
        Ok(Self {
            kind,
            separator,
            limit,
            cursor: Cursor::default(),
        })
    }

    pub(super) fn trace<'a>(&'a self, visit: &mut dyn FnMut(&'a Value)) {
        if let Some(separator) = &self.separator {
            visit(separator);
        }
    }

    /// Return owned item data; the caller publishes it before committing the cursor.
    pub(super) fn preview(
        &self,
        heap: &GcHeap,
        text: &str,
    ) -> Result<(Option<PendingItem>, Cursor), RuntimeError> {
        let separator = match self.separator {
            Some(Value::Str(id)) => Some(
                heap.string(id)
                    .ok_or_else(|| RuntimeError::module_validation("invalid string separator"))?,
            ),
            None => None,
            _ => return Err(RuntimeError::module_validation("invalid string separator")),
        };
        let separator = separator.as_deref().unwrap_or("");
        let mut cursor = self.cursor;
        if cursor.done || cursor.yielded == self.limit {
            cursor.done = true;
            return Ok((None, cursor));
        }
        let start = cursor.position;
        if self.kind == StringIterKind::Bytes {
            let value = text
                .as_bytes()
                .get(start)
                .map(|byte| PendingItem::Value(Value::I64(i64::from(*byte))));
            if value.is_some() {
                cursor.position += 1;
                cursor.yielded += 1;
            } else {
                cursor.done = true;
            }
            return Ok((value, cursor));
        }
        let tail = text
            .get(start..)
            .ok_or_else(|| RuntimeError::module_validation("invalid string cursor"))?;
        if matches!(
            self.kind,
            StringIterKind::Chars | StringIterKind::CharIndices
        ) {
            let value = tail.chars().next().map(|ch| {
                cursor.position += ch.len_utf8();
                cursor.yielded += 1;
                if self.kind == StringIterKind::Chars {
                    PendingItem::Value(Value::Char(ch))
                } else {
                    PendingItem::Pair(Value::U64(start as u64), Value::Char(ch))
                }
            });
            if value.is_none() {
                cursor.done = true;
            }
            return Ok((value, cursor));
        }
        let piece = match self.kind {
            StringIterKind::Chars | StringIterKind::Bytes | StringIterKind::CharIndices => {
                unreachable!()
            }
            StringIterKind::Split | StringIterKind::SplitN => {
                if cursor.yielded == self.limit - 1 {
                    cursor.done = true;
                    Some(tail)
                } else if separator.is_empty() {
                    if !cursor.started {
                        cursor.started = true;
                        Some("")
                    } else if let Some(ch) = tail.chars().next() {
                        cursor.position += ch.len_utf8();
                        Some(&tail[..ch.len_utf8()])
                    } else {
                        cursor.done = true;
                        Some("")
                    }
                } else if let Some(offset) = tail.find(separator) {
                    cursor.position = start + offset + separator.len();
                    Some(&tail[..offset])
                } else {
                    cursor.done = true;
                    Some(tail)
                }
            }
            StringIterKind::Whitespace => {
                let tail = tail.trim_start();
                if tail.is_empty() {
                    cursor.done = true;
                    None
                } else {
                    let length = tail.find(char::is_whitespace).unwrap_or(tail.len());
                    cursor.position = text.len() - tail.len() + length;
                    Some(&tail[..length])
                }
            }
            StringIterKind::Lines => {
                if tail.is_empty() {
                    cursor.done = true;
                    None
                } else if let Some(offset) = tail.find('\n') {
                    cursor.position = start + offset + 1;
                    let line = &tail[..offset];
                    Some(line.strip_suffix('\r').unwrap_or(line))
                } else {
                    cursor.done = true;
                    Some(tail)
                }
            }
        };
        let value = piece
            .map(|piece| {
                let mut result = String::new();
                result
                    .try_reserve_exact(piece.len())
                    .map_err(|_| RuntimeError::resource_limit("string iterator item"))?;
                result.push_str(piece);
                Ok(PendingItem::Text(result))
            })
            .transpose()?;
        if value.is_some() {
            cursor.yielded += 1;
        }
        Ok((value, cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Runtime, error::RuntimeErrorKind, test_support::layouts::allocation_owner};
    use kagari_types::{scalar::BuiltinType, ty::Ty};

    #[test]
    fn failed_string_item_publication_preserves_progress_and_releases_roots() {
        for (kind, expected) in [
            (
                StringIterKind::Chars,
                vec![
                    vec![Value::Char('a')],
                    vec![Value::Char('é')],
                    vec![Value::Char('🙂')],
                ],
            ),
            (
                StringIterKind::CharIndices,
                vec![
                    vec![Value::U64(0), Value::Char('a')],
                    vec![Value::U64(1), Value::Char('é')],
                    vec![Value::U64(3), Value::Char('🙂')],
                ],
            ),
            (
                StringIterKind::Bytes,
                [0x61, 0xc3, 0xa9, 0xf0, 0x9f, 0x99, 0x82]
                    .map(|byte| vec![Value::I64(byte)])
                    .to_vec(),
            ),
        ] {
            let mut runtime = Runtime::default();
            let owner = allocation_owner(&mut runtime);
            let heap = runtime.gc();
            let text = heap.alloc_string("aé🙂".into()).unwrap();
            let cursor = heap
                .new_string_iter(&text, &kind.source_type(), kind, &owner)
                .unwrap();
            let root = runtime.root_value(cursor).unwrap();
            let alias = cursor;
            let ty = Ty::Iter(Box::new(kind.item_type()));
            runtime.collect_garbage().unwrap();
            assert!(heap.validate_value(&text));
            let retained_units = heap.stats().current_heap_units;
            let retained_roots = heap.active_roots();

            for expected in expected {
                let error = heap
                    .advance_iter_with(&cursor, &ty, |payload| {
                        if kind != StringIterKind::CharIndices {
                            assert_eq!(heap.stats().current_heap_units, retained_units);
                        }
                        // Collection during publication must see the source and pending tuple,
                        // with no borrowed String or iterator storage surviving preview.
                        runtime.collect_garbage()?;
                        let value = payload.expect("pending item");
                        assert_item(heap, value, &expected);
                        assert!(heap.validate_value(&text));
                        Err::<(), _>(RuntimeError::resource_limit("item publication"))
                    })
                    .unwrap_err();
                assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
                assert_eq!(heap.active_roots(), retained_roots);
                runtime.collect_garbage().unwrap();
                assert_eq!(heap.stats().current_heap_units, retained_units);

                heap.advance_iter_with(&alias, &ty, |payload| {
                    assert_item(heap, payload.expect("same item after failure"), &expected);
                    Ok(())
                })
                .unwrap();
                runtime.collect_garbage().unwrap();
                assert_eq!(heap.stats().current_heap_units, retained_units);
            }
            for _ in 0..2 {
                assert_eq!(heap.advance_iter_with(&cursor, &ty, Ok).unwrap(), None);
            }
            drop(root);
            runtime.collect_garbage().unwrap();
            assert!(!heap.validate_value(&cursor));
            assert!(!heap.validate_value(&text));
            assert_eq!(heap.active_roots(), 0);
        }
    }

    fn assert_item(heap: &GcHeap, value: Value, expected: &[Value]) {
        match value {
            Value::Tuple(id) => assert_eq!(&*heap.tuple(id).unwrap(), expected),
            value => assert_eq!([value].as_slice(), expected),
        }
    }

    #[test]
    fn string_cursors_reject_wrong_constructor_and_item_domains() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let heap = runtime.gc();
        let text = heap.alloc_string("é".into()).unwrap();
        let old_source = heap.alloc_tuple(vec![text]).unwrap();
        let old_type = Ty::Tuple(vec![Ty::Builtin(BuiltinType::String)]);
        for (kind, wrong_item) in [
            (StringIterKind::Chars, Ty::Builtin(BuiltinType::U32)),
            (
                StringIterKind::CharIndices,
                Ty::Tuple(vec![
                    Ty::Builtin(BuiltinType::U64),
                    Ty::Builtin(BuiltinType::String),
                ]),
            ),
            (StringIterKind::Bytes, Ty::Builtin(BuiltinType::I64)),
        ] {
            assert!(
                heap.new_string_iter(&old_source, &kind.source_type(), kind, &owner)
                    .is_err()
            );
            assert!(
                heap.new_string_iter(&Value::Char('é'), &kind.source_type(), kind, &owner)
                    .is_err()
            );
            assert!(
                heap.new_string_iter(&old_source, &old_type, kind, &owner)
                    .is_err()
            );
            let cursor = heap
                .new_string_iter(&text, &kind.source_type(), kind, &owner)
                .unwrap();
            let error = heap
                .advance_iter_with(
                    &cursor,
                    &Ty::Iter(Box::new(wrong_item)),
                    |_| -> Result<(), RuntimeError> {
                        panic!("wrong semantic item must not reach publication")
                    },
                )
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            assert!(
                heap.advance_iter_with(&cursor, &Ty::Iter(Box::new(kind.item_type())), Ok)
                    .unwrap()
                    .is_some()
            );
        }
    }
}
