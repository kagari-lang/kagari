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

    /// Preview one step. The caller allocates the Option before committing the cursor.
    pub(super) fn trace<'a>(&'a self, visit: &mut dyn FnMut(&'a Value)) {
        if let Some(separator) = &self.separator {
            visit(separator);
        }
    }

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
        let piece = match self.kind {
            StringIterKind::Bytes => unreachable!(),
            StringIterKind::CharIndices => {
                if let Some(ch) = tail.chars().next() {
                    cursor.position += ch.len_utf8();
                    Some(&tail[..ch.len_utf8()])
                } else {
                    cursor.done = true;
                    None
                }
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
                Ok(if self.kind == StringIterKind::CharIndices {
                    PendingItem::CharIndex(start as u64, result)
                } else {
                    PendingItem::Text(result)
                })
            })
            .transpose()?;
        if value.is_some() {
            cursor.yielded += 1;
        }
        Ok((value, cursor))
    }
}
