use anyhow::Result;

pub(crate) fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
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
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(size)
            .map(|chunk| {
                let operation = &operation;
                let context = context.clone();
                scope.spawn(move || {
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
