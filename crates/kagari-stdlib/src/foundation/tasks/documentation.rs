//! Host setup is explicit; examples observe cold creation and retained results.
pub(super) const SCOPE_EXAMPLE: &str = r#"pub async fn main(scope: TaskScope) -> bool {
    val calls: Vec<i32> = Vec::new();
    val task = match scope.spawn(|| {
        calls.push(1);
        async { 42 }
    }) {
        Ok(task) => task,
        Err(_) => { return false; },
    };
    val cold = calls.is_empty();
    val result = task.await;
    cold && result == 42 && calls.len() == 1u64
}"#;

pub(super) const TASK_EXAMPLE: &str = r#"pub async fn main(scope: TaskScope) -> bool {
    val calls: Vec<i32> = Vec::new();
    val task = match scope.spawn(|| {
        calls.push(1);
        async { 42 }
    }) {
        Ok(task) => task,
        Err(_) => { return false; },
    };
    val alias = task;
    val first = task.await;
    val again = alias.await;
    first == 42 && again == 42 && calls.len() == 1u64
}"#;

pub(super) const CLOSED_EXAMPLE: &str = r#"pub fn main(closed_scope: TaskScope) -> bool {
    match closed_scope.spawn(|| async { 42 }) {
        Err(SpawnError::ScopeClosed) => true,
        _ => false,
    }
}"#;

pub(super) const CANCEL_EXAMPLE: &str = r#"pub async fn main(scope: TaskScope) -> bool {
    val calls: Vec<i32> = Vec::new();
    val task = match scope.spawn(|| async { calls.push(1); 42 }) {
        Ok(task) => task,
        Err(_) => { return false; },
    };
    task.cancel();
    val probe = match scope.spawn(|| async { true }) {
        Ok(task) => task,
        Err(_) => { return false; },
    };
    probe.await && calls.is_empty()
}"#;

pub(super) const JOIN_EXAMPLE: &str = r#"async fn main() -> bool {
    val calls: Vec<i32> = Vec::new();
    val pair = core::task::join(
        async { calls.push(1); 10 },
        async { calls.push(2); 20 },
    );
    val cold = calls.is_empty();
    val result = pair.await;
    cold && result == (10, 20) && calls.len() == 2u64
}"#;

pub(super) const TRY_JOIN_EXAMPLE: &str = r#"async fn main() -> bool {
    val left: Future<Result<i32, String>> = async { Ok(10) };
    val right: Future<Result<i32, String>> = async { Ok(20) };
    val success = core::task::try_join(left, right).await;
    val bad: Future<Result<i32, String>> = async { Err("stop") };
    val other: Future<Result<i32, String>> = async { Ok(20) };
    val failure = core::task::try_join(bad, other).await;
    success == Ok((10, 20)) && failure == Err("stop")
}"#;
