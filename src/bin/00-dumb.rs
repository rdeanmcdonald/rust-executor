use std::future::Future;
use std::{pin::Pin, sync::Arc, task::Context};

use std::time::Duration;

use async_std::task;
use futures::task::{waker_ref, ArcWake};

struct Id {
    id: u64,
}

impl ArcWake for Id {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        println!("WAKING TASK {}", arc_self.id);
    }
}

async fn sleep(ms: u64) {
    task::sleep(Duration::from_millis(ms)).await;
}

fn main() {
    println!("Hello, world!");

    let mut sleep_fut = sleep(1);
    let mut sleep_pin = unsafe { Pin::new_unchecked(&mut sleep_fut) };

    let id = Arc::new(Id { id: 1 });
    // now with the pin we can call as_mut to get at the impl Future<> so we can
    // safely call poll
    let waker = waker_ref(&id);
    let mut ctx = Context::from_waker(&waker);

    while sleep_pin.as_mut().poll(&mut ctx).is_pending() {
        println!("NOT READY");
    }

    println!("READY");
}
