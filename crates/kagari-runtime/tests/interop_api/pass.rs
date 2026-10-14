mod prototype;
use prototype::{LoanContext, NativeBinding, NativeContext, Player, Runtime, bind};

fn main() {
    bind(|_: &mut NativeContext, (a, b): (i32, i32)| Ok(a.max(b)));
    bind(|_: &mut NativeContext, (mut p, n): (Player, i32)| {
        p.hp += n;
        Ok(p)
    });
    bind(|_: &mut LoanContext, (p, n): (&Player, i32)| Ok(p.hp + n));
    bind(|_: &mut LoanContext, (p, n): (&mut Player, i32)| {
        p.hp += n;
        Ok(p.hp)
    });
    bind(
        |_: &mut LoanContext, (a, b, n): (&Player, &mut Player, i32)| {
            b.hp += a.hp + n;
            Ok(b.hp)
        },
    );
    bind(|_: &mut LoanContext, (a, b): (&mut Player, &mut Player)| {
        b.hp += a.hp;
        Ok(b.hp)
    });
    bind(NativeBinding::Raw);
    bind(NativeBinding::Primitive);
    let mut runtime = Runtime;
    let mut player = Player { hp: 60 };
    let observed = runtime.with_host_scope(|scope| {
        let view = scope.borrow_mut(&mut player);
        scope.execute_mut(view)
    });
    assert_eq!(observed, 1);
    runtime.with_host_scope(|scope| {
        let view = scope.borrow(&player);
        scope.execute_shared(view)
    });
    player.hp += 10;
    assert_eq!(player.hp, 70);
}
