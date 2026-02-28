use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, OnceLock};

/// Shared helper for small process-local caches keyed by deterministic values.
///
/// Best-effort semantics: if cache lock acquisition fails, this function still
/// returns a freshly built value instead of failing the caller.
pub fn get_or_insert_cached<K, V, E, F>(
    cache_cell: &'static OnceLock<Mutex<HashMap<K, V>>>,
    key: K,
    max_entries: usize,
    build: F,
) -> Result<V, E>
where
    K: Eq + Hash + Clone,
    V: Clone,
    F: FnOnce() -> Result<V, E>,
{
    if max_entries == 0 {
        return build();
    }

    let cache = cache_cell.get_or_init(|| Mutex::new(HashMap::new()));

    if let Ok(guard) = cache.lock()
        && let Some(value) = guard.get(&key)
    {
        return Ok(value.clone());
    }

    let value = build()?;

    if let Ok(mut guard) = cache.lock() {
        if !guard.contains_key(&key)
            && guard.len() >= max_entries
            && let Some(evict_key) = guard.keys().next().cloned()
        {
            guard.remove(&evict_key);
        }
        guard.insert(key, value.clone());
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn returns_cached_value_on_subsequent_reads() {
        let cache_cell: &'static OnceLock<Mutex<HashMap<u64, String>>> =
            Box::leak(Box::new(OnceLock::new()));
        static BUILD_COUNT: AtomicUsize = AtomicUsize::new(0);
        BUILD_COUNT.store(0, Ordering::SeqCst);

        let first = get_or_insert_cached(cache_cell, 100, 4, || {
            BUILD_COUNT.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>("client-a".to_string())
        })
        .unwrap();
        let second = get_or_insert_cached(cache_cell, 100, 4, || {
            BUILD_COUNT.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>("client-b".to_string())
        })
        .unwrap();

        assert_eq!(first, "client-a");
        assert_eq!(second, "client-a");
        assert_eq!(BUILD_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn evicts_when_capacity_is_reached() {
        let cache_cell: &'static OnceLock<Mutex<HashMap<u64, String>>> =
            Box::leak(Box::new(OnceLock::new()));

        get_or_insert_cached(cache_cell, 1, 1, || Ok::<_, ()>("first".to_string())).unwrap();
        get_or_insert_cached(cache_cell, 2, 1, || Ok::<_, ()>("second".to_string())).unwrap();

        let cache = cache_cell.get().unwrap().lock().unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&2).is_some());
    }
}
