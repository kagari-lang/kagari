mod prototype;
use prototype::{Player, Runtime};

fn main() {
    let mut runtime = Runtime;
    let player = Player { hp: 60 };
    runtime.with_host_scope(|scope| {
        let view = scope.borrow(&player);
        scope.execute_mut(view);
    });
}
