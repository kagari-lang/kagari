//! Reject impossible executable collection lengths before decoding their elements.
use serde::{
    Deserialize, Deserializer,
    de::{self, SeqAccess, Visitor},
};
use std::{fmt, marker::PhantomData};

pub(crate) const MAX_MODULES: usize = 1_024;
pub(crate) const MAX_FUNCTIONS: usize = 65_536;
pub(crate) const MAX_INSTRUCTIONS: usize = 1_000_000;
pub(crate) const MAX_TABLE_RECORDS: usize = 1_000_000;
pub(crate) const MAX_NESTED_RECORDS: usize = 4_096;

pub(crate) fn map<'de, D, K, V>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    struct BoundedMap<K, V>(PhantomData<(K, V)>);
    impl<'de, K: Deserialize<'de> + Ord, V: Deserialize<'de>> Visitor<'de> for BoundedMap<K, V> {
        type Value = std::collections::BTreeMap<K, V>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded unique associated type map")
        }
        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            if map
                .size_hint()
                .is_some_and(|count| count > MAX_NESTED_RECORDS)
            {
                return Err(de::Error::custom("associated type count limit exceeded"));
            }
            let mut result = std::collections::BTreeMap::new();
            while let Some((key, value)) = map.next_entry()? {
                if result.len() >= MAX_NESTED_RECORDS || result.insert(key, value).is_some() {
                    return Err(de::Error::custom("invalid associated type map"));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(BoundedMap(PhantomData))
}

fn bounded<'de, D, T>(
    deserializer: D,
    limit: usize,
    label: &'static str,
) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T> {
        limit: usize,
        label: &'static str,
        marker: PhantomData<T>,
    }
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "at most {} {}", self.limit, self.label)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            if sequence.size_hint().is_some_and(|count| count > self.limit) {
                return Err(de::Error::custom(format!(
                    "{} count limit exceeded",
                    self.label
                )));
            }
            let mut elements = Vec::new();
            while let Some(element) = sequence.next_element()? {
                if elements.len() >= self.limit {
                    return Err(de::Error::custom(format!(
                        "{} count limit exceeded",
                        self.label
                    )));
                }
                elements.push(element);
            }
            Ok(elements)
        }
    }
    deserializer.deserialize_seq(Bounded {
        limit,
        label,
        marker: PhantomData,
    })
}

pub(crate) fn modules<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded(deserializer, MAX_MODULES, "module")
}

pub(crate) fn functions<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded(deserializer, MAX_FUNCTIONS, "function")
}

pub(crate) fn instructions<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded(deserializer, MAX_INSTRUCTIONS, "instruction")
}

pub(crate) fn table<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded(deserializer, MAX_TABLE_RECORDS, "artifact table")
}

pub(crate) fn nested<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded(deserializer, MAX_NESTED_RECORDS, "nested declaration")
}

pub(crate) fn operands<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded(deserializer, MAX_NESTED_RECORDS, "instruction operand")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::Options;

    #[derive(Debug, Deserialize)]
    struct Counts {
        #[serde(deserialize_with = "modules")]
        _modules: Vec<u8>,
    }

    #[derive(Debug, Deserialize)]
    struct FunctionCounts {
        #[serde(deserialize_with = "functions")]
        _functions: Vec<u8>,
    }

    #[derive(Debug, Deserialize)]
    struct InstructionCounts {
        #[serde(deserialize_with = "instructions")]
        _instructions: Vec<u8>,
    }

    #[derive(Debug, Deserialize)]
    struct TableCounts {
        #[serde(deserialize_with = "table")]
        _table: Vec<u8>,
    }

    #[derive(Debug, Deserialize)]
    struct NestedCounts {
        #[serde(deserialize_with = "nested")]
        _nested: Vec<u8>,
    }

    #[test]
    fn declared_count_is_rejected_before_reading_any_element() {
        let bytes = u64::MAX.to_le_bytes();
        let error = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize::<Counts>(&bytes)
            .unwrap_err();
        assert!(error.to_string().contains("module count limit exceeded"));
        let error = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize::<FunctionCounts>(&bytes)
            .unwrap_err();
        assert!(error.to_string().contains("function count limit exceeded"));
        let error = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize::<InstructionCounts>(&bytes)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("instruction count limit exceeded")
        );
        let error = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize::<TableCounts>(&bytes)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("artifact table count limit exceeded")
        );
        let error = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize::<NestedCounts>(&bytes)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("nested declaration count limit exceeded")
        );
    }
}
