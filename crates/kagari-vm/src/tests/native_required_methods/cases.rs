pub(super) fn cases() -> Vec<(&'static str, String)> {
    let mut cases = Vec::new();
    for (name, interface, source, expected) in [
        ("list_len", "List<i32>", "[20,22]", "2usize"),
        (
            "map_len",
            "Map<String,i32>",
            "LinkedHashMap::from([(\"a\",20),(\"b\",22)])",
            "2usize",
        ),
        (
            "set_len",
            "Set<i32>",
            "LinkedHashSet::from([20,22])",
            "2usize",
        ),
    ] {
        cases.push((name, format!(r#"fn query<C:{interface}>(source:C)->usize{{print("read");source.len()}}fn main()->i32{{val value=query(input({source}));std::debug::assert(value=={expected},"length");print("done");42}}"#)));
    }
    for (name, interface, source) in [
        (
            "list_empty",
            "List<i32>",
            "{val items:ArrayList<i32> = [];items}",
        ),
        (
            "map_empty",
            "Map<String,i32>",
            "{val items:LinkedHashMap<String,i32> = LinkedHashMap::new();items}",
        ),
        (
            "set_empty",
            "Set<i32>",
            "{val items:LinkedHashSet<i32> = LinkedHashSet::new();items}",
        ),
    ] {
        cases.push((name,format!(r#"fn query<C:{interface}>(source:C)->bool{{print("read");source.is_empty()}}fn main()->i32{{std::debug::assert(query(input({source})),"empty");print("done");42}}"#)));
    }
    for (name, body) in [
        (
            "list_get",
            "std::debug::assert(source.get(index(1usize))==Some(22),\"get\");",
        ),
        (
            "list_pop",
            "std::debug::assert(source.pop()==Some(22),\"pop\");",
        ),
        (
            "list_remove",
            "std::debug::assert(source.remove(index(1usize))==Some(22),\"remove\");",
        ),
        (
            "list_push",
            "source.push(item(42));std::debug::assert(source.len()==3usize,\"push\");",
        ),
        (
            "list_insert",
            "source.insert(index(1usize),item(42));std::debug::assert(source.get(1usize)==Some(42),\"insert\");",
        ),
        (
            "list_clear",
            "source.clear();std::debug::assert(source.is_empty(),\"clear\");",
        ),
        (
            "list_set",
            "source.set(index(1usize),item(42));std::debug::assert(source.get(1usize)==Some(42),\"set\");",
        ),
        (
            "list_swap",
            "source.swap(index(0usize),index(1usize));std::debug::assert(source.get(0usize)==Some(22),\"swap\");",
        ),
        (
            "list_reverse",
            "source.reverse();std::debug::assert(source.get(0usize)==Some(22),\"reverse\");",
        ),
        (
            "list_truncate",
            "source.truncate(index(1usize));std::debug::assert(source.len()==1usize,\"truncate\");",
        ),
    ] {
        cases.push((name,format!(r#"fn change<C:MutableList<i32>>(source:C){{print("change");{body}print("changed");}}fn main()->i32{{change(input([20,22]));print("done");42}}"#)));
    }
    for (name, interface, source) in [
        (
            "map_clear",
            "MutableMap<String,i32>",
            "LinkedHashMap::from([(\"a\",20),(\"b\",22)])",
        ),
        (
            "set_clear",
            "MutableSet<i32>",
            "LinkedHashSet::from([20,22])",
        ),
    ] {
        cases.push((name,format!(r#"fn change<C:{interface}>(source:C){{print("change");source.clear();print("changed");std::debug::assert(source.is_empty(),"clear");}}fn main()->i32{{change(input({source}));print("done");42}}"#)));
    }
    for (name, source, item, expected) in [
        ("iter_array", "[20,22]", "i32", "Some(20)"),
        (
            "iter_map",
            "LinkedHashMap::from([(\"a\",20)])",
            "(String,i32)",
            "Some((\"a\",20))",
        ),
        (
            "iter_set",
            "LinkedHashSet::from([20,22])",
            "i32",
            "Some(20)",
        ),
        ("iter_string", "\"ab\"", "String", "Some(\"a\")"),
        ("iter_range", "20..22", "i32", "Some(20)"),
        ("iter_inclusive", "20..=22", "i32", "Some(20)"),
        ("iter_from", "20..", "i32", "Some(20)"),
    ] {
        cases.push((name,format!(r#"fn query<I:Iterable<Item={item}>>(source:I){{print("iter");val cursor=source.iter();print("next");std::debug::assert(cursor.next()=={expected},"first");}}fn main()->i32{{query(input({source}));print("done");42}}"#)));
    }
    cases.push(("next_lazy", r#"fn step<I:Iterator<Item=i32>>(cursor:I)->Option<i32>{print("next");cursor.next()}fn main()->i32{val cursor=input([20,22].iter().map(|x|{print("visit");x}));std::debug::assert(step(cursor)==Some(20),"first");std::debug::assert(step(cursor)==Some(22),"second");std::debug::assert(step(cursor)==None,"end");print("done");42}"#.into()));
    for (name, range, lower, upper) in [
        (
            "bounds_range",
            "20usize..22usize",
            "Bound::Included(20usize)",
            "Bound::Excluded(22usize)",
        ),
        (
            "bounds_inclusive",
            "20usize..=22usize",
            "Bound::Included(20usize)",
            "Bound::Included(22usize)",
        ),
        (
            "bounds_from",
            "20usize..",
            "Bound::Included(20usize)",
            "Bound::Unbounded",
        ),
        (
            "bounds_to",
            "..22usize",
            "Bound::Unbounded",
            "Bound::Excluded(22usize)",
        ),
        (
            "bounds_to_inclusive",
            "..=22usize",
            "Bound::Unbounded",
            "Bound::Included(22usize)",
        ),
        ("bounds_full", "..", "Bound::Unbounded", "Bound::Unbounded"),
    ] {
        cases.push((name,format!(r#"fn query<R:RangeBounds<usize>>(source:R){{print("start bound");std::debug::assert(source.start_bound()=={lower},"start");print("end bound");std::debug::assert(source.end_bound()=={upper},"end");}}fn main()->i32{{query(input({range}));print("done");42}}"#)));
    }
    for (name, ty, input, expected) in [
        ("parse_i32", "i32", "42", "Ok(42)"),
        ("parse_bool", "bool", "true", "Ok(true)"),
        ("parse_bad", "i32", "bad", "Err(ParseError::InvalidDigit)"),
    ] {
        cases.push((name,format!(r#"fn main()->i32{{val value=<{ty} as FromStr>::from_str(input("{input}"));print("parsed");std::debug::assert(value=={expected},"parse");print("done");42}}"#)));
    }
    for (_, source) in &mut cases {
        source.push_str(r#"fn input<T>(value:T)->T{print("input");value}fn index(value:usize)->usize{print("index");value}fn item(value:i32)->i32{print("item");value}"#);
    }
    cases
}
