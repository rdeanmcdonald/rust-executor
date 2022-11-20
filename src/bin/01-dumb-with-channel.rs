use std::collections::HashMap;
use std::future::Future;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Mutex;
use std::task::Context;
use std::thread;
use std::time::Duration;
use std::{pin::Pin, sync::Arc};

use async_std::task;
use futures::task::{waker_ref, ArcWake};

type FutureId = u64;
struct Task {
    id: FutureId,
    waker_tx: SyncSender<FutureId>,
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let thread_id = thread::current().id();
        println!("WAKING TASK {} FROM THREAD {:?}", arc_self.id, thread_id);
        arc_self.waker_tx.send(arc_self.id).unwrap();
    }
}

type PinnedFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
type FuturesMap<'a> = Arc<Mutex<HashMap<FutureId, (Arc<Task>, PinnedFuture<'a>)>>>;
struct Spawner<'a> {
    futures_map: FuturesMap<'a>,
    executor_tx: SyncSender<FutureId>,
}

impl<'a> Spawner<'a> {
    fn spawn(&self, id: FutureId, waker_tx: SyncSender<FutureId>, pinned_fut: PinnedFuture<'a>) {
        // first, update the futures_map with the new future
        let mut map = self.futures_map.lock().unwrap();
        let task = Arc::new(Task { id, waker_tx });
        map.insert(id, (task, pinned_fut));

        // finally, send the ID to the executor's queue
        self.executor_tx.send(id).unwrap();
    }
}

async fn sleep(ms: u64) {
    let thread_id = thread::current().id();
    println!("THREAD ID: {:?}", thread_id);
    task::sleep(Duration::from_millis(ms)).await;
}

fn main() {
    // Now that we can executute 1 future, let try to executute any number of
    // futures (from the top level of the program). That means we need a top
    // level "spawner" which can give futures to the executor, and a "queue" for
    // the executor to work on.  Futures that are pending go back in the queue.
    // We'll worry about spawing futures (i.e. giving futures to the executor)
    // from within futures later.
    //
    // The first thing we need to think about, when spawning, what exactly are
    // we giving the executor? Are we giving it like an ID of a future which the
    // executor can go look up? That could work, then we need some global
    // structure mapping IDs to Futures. Another possibility is that the Spawner
    // gives the executor the Future struct itself. That seems better, but seems
    // more difficult? Let's start with the ID approach to begin with less
    // cognative overhead. Remember, Future structs need to be wrapped in
    // Pin<fut>, and also Future is a trait, not a type, so we need to use dyn
    // trait object type. I made a PinnedFuture type alias to express this.
    //
    // So I got pretty far with that idea, then realized, the executor, as we
    // saw last, needs a "Context" when polling a future. So, when our executor
    // get's an Id to work on from the queue, it also somehow needs a way to get
    // the Context associated with a Future. That's where the impl ArcWake comes
    // in on a Task. So for now, the Map will just store a task, along with a
    // future in the map as a tuple. (at this point, it seems almost easier to
    // just send the Task, and have the task struct hold the future, but we'll
    // just keep moving with this idea for now)
    //
    // The spawner and executor will share the futures map, so it needs to be
    // wrapped in arc<mutex>. It also needs the sending end of the executors's
    // queue.
    let futures_map: Arc<Mutex<HashMap<FutureId, (Arc<Task>, PinnedFuture)>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // let's create our spawner
    let (tx, rx): (SyncSender<FutureId>, Receiver<FutureId>) = sync_channel(10000);
    let spawner = Spawner {
        futures_map: futures_map.clone(),
        executor_tx: tx.clone(),
    };

    // now, lets spawn a handfull of futures, which the executor will work
    // through
    let mut id: FutureId = 0;
    // note, we need to box the future. Without boxing, I couldn't figure out a
    // way to create the PinnedFuture type. Not that with boxing, we no longer
    // need &mut when pinning. (needed ref when working directly on the async
    // fn, since the async fn didn't implement deref. box does so it's no issue
    // now)
    let s500: Box<dyn Future<Output = ()> + Send> = Box::new(sleep(500));
    let s500_pinned = unsafe { Pin::new_unchecked(s500) };
    spawner.spawn(id, tx.clone(), s500_pinned);
    id += 1;

    let s1000: Box<dyn Future<Output = ()> + Send> = Box::new(sleep(1000));
    let s1000_pinned = unsafe { Pin::new_unchecked(s1000) };
    spawner.spawn(id, tx.clone(), s1000_pinned);
    id += 1;

    let s100: Box<dyn Future<Output = ()> + Send> = Box::new(sleep(100));
    let s100_pinned = unsafe { Pin::new_unchecked(s100) };
    spawner.spawn(id, tx.clone(), s100_pinned);

    // Now, we can execute the queue of futures, putting pending futures at the
    // back of the queue.
    while let Ok(id) = rx.recv() {
        println!("RUNNING {}", id);
        let mut map = futures_map.lock().unwrap();
        // the id will definitely exist
        let (task, fut) = map.get_mut(&id).unwrap();
        let waker = waker_ref(&task);
        let mut ctx = Context::from_waker(&waker);
        if fut.as_mut().poll(&mut ctx).is_ready() {
            println!("READY {}", id);
        }
        // note, the future itself will call the waker function that's passed
        // in! so we don't need to put the future back on the queue
    }
    // We see something like this:
    // RUNNING 0
    // RUNNING 1
    // RUNNING 2
    // WAKING TASK 2
    // RUNNING 2
    // READY 2
    // WAKING TASK 0
    // RUNNING 0
    // READY 0
    // WAKING TASK 1
    // RUNNING 1
    // READY 1
    //
}
