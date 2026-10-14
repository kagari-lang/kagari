mod prototype;
use prototype::{Player, Runtime};

fn main() {
    let mut runtime = Runtime;
    let mut player = Player { hp: 60 };
    runtime.with_host_scope(|scope| {
        let view = scope.borrow_mut(&mut player);
        scope.execute_mut(view);
        player.hp = 70;
    });
}
