use anyhow::Result;
use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

thread_local! {
    static BATCH: RefCell<Option<Arc<AtomicUsize>>> = const { RefCell::new(None) };
}

pub(crate) struct Budget(Option<Arc<AtomicUsize>>);

pub(crate) fn batch_budget(active: Arc<AtomicUsize>) -> Budget {
    Budget(BATCH.replace(Some(active)))
}

impl Drop for Budget {
    fn drop(&mut self) {
        BATCH.set(self.0.take());
    }
}

pub(crate) fn threads() -> usize {
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    BATCH.with_borrow(|batch| {
        let active = batch
            .as_ref()
            .map_or(1, |active| active.load(Ordering::Relaxed).max(1));
        (available / active).max(1)
    })
}

pub(crate) fn map<T: Sync, U: Send>(
    items: &[T],
    operation: impl Fn(&T) -> Result<U> + Sync,
) -> Result<Vec<U>> {
    let threads = threads().min(items.len()).max(1);
    if threads == 1 {
        return items.iter().map(operation).collect();
    }
    let size = items.len().div_ceil(threads);
    let context = crate::packfs::context();
    let budget = BATCH.with_borrow(Clone::clone);
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(size)
            .map(|chunk| {
                let operation = &operation;
                let context = context.clone();
                let budget = budget.clone();
                scope.spawn(move || {
                    let _budget = budget.map(batch_budget);
                    let _context = crate::packfs::enter_context(context);
                    chunk.iter().map(operation).collect::<Result<Vec<_>>>()
                })
            })
            .collect();
        let mut values = Vec::with_capacity(items.len());
        for handle in handles {
            values.extend(
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("并行资源任务意外终止"))??,
            );
        }
        Ok(values)
    })
}
