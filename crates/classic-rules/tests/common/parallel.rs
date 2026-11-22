//! Shared by the seed sweeps (`#[path = "common/parallel.rs"] mod parallel;`): the games of a sweep are
//! independent, so they run on a few threads instead of one after another. The result of item `i` is
//! `work(&items[i])`, in the order of `items`; a panic of a game fails the test with its message.
//! At most `THREADS` threads (env `AURELIA_TEST_THREADS` changes it): the test runner already runs several
//! tests at once, so one test must not take all the cores.

const THREADS: usize = 2;

pub fn parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::env::var("AURELIA_TEST_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(THREADS).clamp(1, items.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(at) else { break done };
                        done.push((at, work(item)));
                    }
                })
            })
            .collect();
        let mut all: Vec<(usize, R)> = handles.into_iter().flat_map(|h| h.join().expect("a game of the sweep ran")).collect();
        all.sort_by_key(|(at, _)| *at);
        all.into_iter().map(|(_, r)| r).collect()
    })
}
