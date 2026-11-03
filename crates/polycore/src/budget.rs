//! A deterministic work budget for long kernel operations (fillet, chamfer, the boolean retry
//! ladder). Work is counted in units, one per edge-order attempt, per-edge blend attempt,
//! boolean retry stage and boolean intersection (SSI march), never in wall time, so the same
//! input stops at the same stage on every machine and thread count.
//!
//! The budget is opt-in and installed like a [`crate::watch::Watch`]: [`with_work_budget`]
//! scopes it to the calling thread. With none installed every check is one thread-local read
//! and results are unchanged. When the units are spent the operation returns
//! [`KernelError::BudgetSpent`], naming the stage that found the budget empty; that is neither
//! an ordinary refusal nor a cancellation.
//!
//! Worker threads: the pool helpers in [`crate::par`] run each item under a [`Fork`], which
//! gives the item its own count (starting from what is left) and the caller's watch. After the
//! items are done the caller is charged their total. An item's count does not depend on which
//! thread ran it or when, so the outcome is the same on one thread and on many.

use crate::{KernelError, Result};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
struct State {
    limit: u64,
    spent: u64,
    /// The stage that found the budget empty (the first one; later checks repeat it).
    out: Option<&'static str>,
}

#[cfg(test)]
thread_local! {
    /// The stages that reached a checkpoint on this thread, in order (tests read them).
    pub(crate) static TRAIL: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

thread_local! {
    static CURRENT: Cell<Option<State>> = const { Cell::new(None) };
}

/// Run `f` with a budget of `units` for the kernel work it does on this thread (and on the
/// worker threads that work starts). Inside another budget the smaller of `units` and what
/// is left applies, and the work done is charged to the outer one too.
pub fn with_work_budget<R>(units: u64, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<State>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let inner = CURRENT.with(Cell::get);
            let merged = self.0.map(|mut outer| {
                if let Some(inner) = inner {
                    outer.spent = outer.spent.saturating_add(inner.spent);
                    if outer.out.is_none() && outer.spent >= outer.limit {
                        outer.out = inner.out;
                    }
                }
                outer
            });
            CURRENT.with(|c| c.set(merged));
        }
    }
    let outer = CURRENT.with(Cell::get);
    let limit = outer.map_or(units, |o| units.min(o.limit.saturating_sub(o.spent)));
    CURRENT.with(|c| c.set(Some(State { limit, spent: 0, out: None })));
    let _restore = Restore(outer);
    f()
}

/// The units used so far under the innermost [`with_work_budget`] on this thread, if any.
pub fn work_spent() -> Option<u64> {
    CURRENT.with(Cell::get).map(|s| s.spent)
}

/// Take one unit for `stage`. `Err(BudgetSpent)` when none is left (and on every later call,
/// naming the same first stage). No budget installed: free.
pub(crate) fn charge(stage: &'static str) -> Result<()> {
    CURRENT.with(|c| match c.get() {
        None => Ok(()),
        Some(mut s) => {
            if let Some(out) = s.out {
                return Err(KernelError::BudgetSpent { stage: out, limit: s.limit });
            }
            if s.spent >= s.limit {
                s.out = Some(stage);
                c.set(Some(s));
                return Err(KernelError::BudgetSpent { stage, limit: s.limit });
            }
            s.spent += 1;
            c.set(Some(s));
            Ok(())
        }
    })
}

/// Stop if the installed watch cancelled the work: the error is [`crate::watch::cancelled_error`].
pub(crate) fn cancel_point(stage: &str) -> Result<()> {
    if crate::watch::is_cancelled() { Err(crate::watch::cancelled_error(stage)) } else { Ok(()) }
}

/// A place the work may stop: cancelled first, then one unit of budget.
pub(crate) fn checkpoint(stage: &'static str) -> Result<()> {
    #[cfg(test)]
    TRAIL.with(|t| t.borrow_mut().push(stage));
    cancel_point(stage)?;
    charge(stage)
}

/// `Err` when the work is to stop: its watch cancelled it or its budget is already spent. Takes
/// no unit. For the start of a fallback that would otherwise run on after a stop.
pub(crate) fn halted() -> Result<()> {
    cancel_point("operation")?;
    match CURRENT.with(Cell::get) {
        Some(State { out: Some(stage), limit, .. }) => Err(KernelError::BudgetSpent { stage, limit }),
        _ => Ok(()),
    }
}

/// Whether `e` is the budget running out or (with a watch cancelled) the cancellation: an
/// error the retry ladders must pass on, not try around.
pub(crate) fn is_stop(e: &KernelError) -> bool {
    matches!(e, KernelError::BudgetSpent { .. }) || (matches!(e, KernelError::Failed(_)) && crate::watch::is_cancelled())
}

/// `r`, except that an error from work that ran out of budget (or was cancelled) is that
/// stop, whatever a stage along the way made of it.
pub(crate) fn stop_or<T>(r: Result<T>) -> Result<T> {
    match r {
        Err(e) if !matches!(e, KernelError::BudgetSpent { .. }) => {
            if crate::watch::is_cancelled() {
                return Err(crate::watch::cancelled_error("operation"));
            }
            match CURRENT.with(Cell::get) {
                Some(State { out: Some(stage), limit, .. }) => Err(KernelError::BudgetSpent { stage, limit }),
                _ => Err(e),
            }
        }
        r => r,
    }
}

/// What a worker thread needs of the thread that started it: the watch and the budget left.
pub(crate) struct Fork {
    watch: Option<Arc<dyn crate::watch::Watch>>,
    left: Option<u64>,
    spent: AtomicU64,
}

impl Fork {
    pub(crate) fn capture() -> Self {
        let left = CURRENT.with(Cell::get).map(|s| s.limit.saturating_sub(s.spent));
        Fork { watch: crate::watch::current(), left, spent: AtomicU64::new(0) }
    }

    /// Whether there is anything to hand on.
    fn idle(&self) -> bool {
        self.watch.is_none() && self.left.is_none()
    }

    /// Run one item under the caller's watch and its own count of the budget.
    pub(crate) fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        if self.idle() {
            return f();
        }
        let saved = CURRENT.with(Cell::get);
        CURRENT.with(|c| c.set(self.left.map(|limit| State { limit, spent: 0, out: None })));
        struct Back<'a>(Option<State>, &'a AtomicU64);
        impl Drop for Back<'_> {
            fn drop(&mut self) {
                let used = CURRENT.with(Cell::get).map_or(0, |s| s.spent);
                self.1.fetch_add(used, Ordering::Relaxed);
                CURRENT.with(|c| c.set(self.0));
            }
        }
        let _back = Back(saved, &self.spent);
        match &self.watch {
            Some(w) => crate::watch::watching(w.clone(), f),
            None => f(),
        }
    }

    /// Charge the calling thread for what the items used (their sum, so independent of order).
    pub(crate) fn settle(&self) {
        let used = self.spent.swap(0, Ordering::Relaxed);
        if used > 0 {
            CURRENT.with(|c| {
                if let Some(mut s) = c.get() {
                    s.spent = s.spent.saturating_add(used);
                    c.set(Some(s));
                }
            });
        }
    }
}
