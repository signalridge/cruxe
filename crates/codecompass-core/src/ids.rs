use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a process-local monotonic job id.
///
/// The identifier combines current wall-clock time, process id, thread id,
/// and an atomic counter to reduce collision risk across rapid calls.
pub fn new_job_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let mut thread_hasher = DefaultHasher::new();
    std::thread::current().id().hash(&mut thread_hasher);
    let thread_hash = thread_hasher.finish() as u128;
    let pid = std::process::id() as u128;
    let counter = JOB_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;

    let mixed = now_nanos ^ (pid << 32) ^ thread_hash ^ counter;
    format!("{:032x}", mixed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn job_id_generation_is_unique_for_small_batch() {
        let ids: Vec<String> = (0..128).map(|_| new_job_id()).collect();
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len());
    }
}
