//! Ordered, bounded parallel map over independent items (desktop only; the browser build runs
//! serially). Each item runs under [`crate::guard`]; a worker that cannot be spawned runs its
//! chunk on the calling thread, and a worker that dies reports an internal error for its items.

use crate::{KernelError, Result, guard};

/// Most worker threads.
const MAX_WORKERS: usize = 8;

pub(crate) fn workers() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map_or(1, |n| n.get().min(MAX_WORKERS))
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// `f` over `items`, results in item order. Runs serially below `min_items` items.
pub(crate) fn map<T: Sync, R: Send>(what: &'static str, items: &[T], min_items: usize, f: impl Fn(&T) -> Result<R> + Sync) -> Vec<Result<R>> {
    let one = |item: &T| guard(what, || f(item));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let workers = workers();
        if items.len() >= min_items.max(2) && workers > 1 {
            return std::thread::scope(|scope| {
                let size = items.len().div_ceil(workers);
                let one = &one;
                let pending: Vec<_> = items
                    .chunks(size)
                    .map(|chunk| {
                        let handle =
                            std::thread::Builder::new().name(what.into()).spawn_scoped(scope, move || chunk.iter().map(one).collect::<Vec<_>>()).ok();
                        (chunk, handle)
                    })
                    .collect();
                let mut out = Vec::with_capacity(items.len());
                for (chunk, handle) in pending {
                    match handle {
                        Some(h) => match h.join() {
                            Ok(results) => out.extend(results),
                            Err(_) => out.extend(chunk.iter().map(|_| Err(KernelError::Internal(format!("{what}: worker stopped"))))),
                        },
                        None => out.extend(chunk.iter().map(one)),
                    }
                }
                out
            });
        }
    }
    items.iter().map(one).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_and_errors() {
        let items: Vec<u32> = (0..1000).collect();
        let out = map("test", &items, 1, |x| if *x == 500 { Err(KernelError::Invalid("five hundred".into())) } else { Ok(x * 2) });
        assert_eq!(out.len(), 1000);
        for (i, r) in out.iter().enumerate() {
            match r {
                Ok(v) => assert_eq!(*v, 2 * i as u32),
                Err(_) => assert_eq!(i, 500),
            }
        }
    }
}
