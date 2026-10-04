//! Bounded semantic metadata collections and unique associated-output maps.
use kagari_common::decode_limits::bounded_vec;
use serde::{
    Deserialize, Deserializer, de,
    de::{Error, Visitor},
};
use std::{collections::BTreeMap, fmt, marker::PhantomData};

pub const MAX_NESTED_RECORDS: usize = 4_096;
pub const MAX_TABLE_RECORDS: usize = 1_000_000;

pub fn map<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    struct BoundedMap<K, V>(PhantomData<(K, V)>);

    impl<'de, K: Deserialize<'de> + Ord, V: Deserialize<'de>> Visitor<'de> for BoundedMap<K, V> {
        type Value = BTreeMap<K, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded unique associated type map")
        }

        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            if map
                .size_hint()
                .is_some_and(|count| count > MAX_NESTED_RECORDS)
            {
                return Err(Error::custom("associated type count limit exceeded"));
            }
            let mut result = BTreeMap::new();
            while let Some((key, value)) = map.next_entry()? {
                if result.len() >= MAX_NESTED_RECORDS || result.insert(key, value).is_some() {
                    return Err(Error::custom("invalid associated type map"));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(BoundedMap(PhantomData))
}

pub fn nested<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded_vec(deserializer, MAX_NESTED_RECORDS, "nested declaration")
}
pub fn table<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded_vec(deserializer, MAX_TABLE_RECORDS, "artifact table")
}
