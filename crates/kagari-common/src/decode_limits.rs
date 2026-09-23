//! Shared sequence preflight for portable identity and host declaration data.
use serde::{
    Deserialize, Deserializer,
    de::{self, SeqAccess, Visitor},
};
use std::{fmt, marker::PhantomData};

pub(crate) fn bounded_vec<'de, D, T>(
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
