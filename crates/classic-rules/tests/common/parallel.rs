//! Shared by the seed sweeps (`#[path = "common/parallel.rs"] mod parallel;`): the games of a sweep are
//! independent, so they run on all the cores the machine has instead of one after another. The result of
//! item `i` is `work(&items[i])`, in the order of `items`; a panic of a game fails the test with its message.

pub fn parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Option<R>>> = std::sync::Mutex::new((0..items.len()).map(|_| None).collect());
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, items.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(item) = items.get(at) else { break };
                let result = work(item);
                results.lock().expect("no game panicked holding the results")[at] = Some(result);
            });
        }
    });
    results.into_inner().expect("no game panicked holding the results").into_iter().map(|r| r.expect("every item ran")).collect()
}
