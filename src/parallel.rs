//! Small order-preserving parallel map built on scoped threads.

/// Map `f` over `items` on a small thread pool, preserving order. A panic in
/// `f` yields `None` for that item instead of taking the whole run down.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    let run = |item: &T| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(item))).ok();

    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(items.len());
        if workers > 1 {
            let next = AtomicUsize::new(0);
            let results: Mutex<Vec<Option<R>>> =
                Mutex::new((0..items.len()).map(|_| None).collect());
            std::thread::scope(|s| {
                for _ in 0..workers {
                    s.spawn(|| {
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            if i >= items.len() {
                                break;
                            }
                            let r = run(&items[i]);
                            if let Ok(mut g) = results.lock() {
                                g[i] = r;
                            }
                        }
                    });
                }
            });
            return results.into_inner().unwrap_or_default();
        }
    }
    items.iter().map(run).collect()
}
