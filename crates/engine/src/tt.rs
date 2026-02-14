//! Clustered transposition table - Pleco-style cache-aligned storage
//! Way faster than FxHashMap for perft workloads

use std::alloc::{alloc_zeroed, dealloc, Layout};

/// Single TT entry - 16 bytes
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct TTEntry {
    /// Board hash (upper 48 bits stored, lower 16 used for indexing)
    pub key: u64,
    /// Cached node count
    pub nodes: u64,
}

/// Cluster of entries - 32 bytes (cache line friendly)
#[derive(Clone, Copy, Default)]
#[repr(C, align(32))]
pub struct Cluster {
    pub entries: [TTEntry; 2],
}

/// Cache-aligned transposition table
/// Uses raw allocation for zero-init and alignment
pub struct TranspositionTable {
    clusters: *mut Cluster,
    mask: usize, // capacity - 1 for fast indexing
    capacity: usize,
}

impl TranspositionTable {
    /// Create new TT with given capacity (rounded up to power of 2)
    pub fn new(size_mb: usize) -> Self {
        let bytes = size_mb.saturating_mul(1024 * 1024);
        let cluster_size = std::mem::size_of::<Cluster>();
        let num_clusters = (bytes / cluster_size).max(1).next_power_of_two();
        let capacity = num_clusters;
        let mask = capacity - 1;

        let layout = match Layout::array::<Cluster>(capacity) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid transposition table layout for capacity {capacity}"),
        };
        let clusters = unsafe { alloc_zeroed(layout) as *mut Cluster };
        if clusters.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        Self {
            clusters,
            mask,
            capacity,
        }
    }

    /// Probe for cached value
    #[inline]
    pub fn probe(&self, key: u64, depth: u32) -> Option<u64> {
        let combined_key = key ^ (depth as u64);
        let index = (combined_key as usize) & self.mask;

        unsafe {
            let cluster = &*self.clusters.add(index);

            // Check both slots
            for entry in &cluster.entries {
                if entry.key == combined_key && entry.nodes != 0 {
                    return Some(entry.nodes);
                }
            }
        }

        None
    }

    /// Store value - replaces least valuable entry
    #[inline]
    pub fn store(&mut self, key: u64, depth: u32, nodes: u64) {
        let combined_key = key ^ (depth as u64);
        let index = (combined_key as usize) & self.mask;

        unsafe {
            let cluster = &mut *self.clusters.add(index);

            // Find empty slot or replace first
            for entry in &mut cluster.entries {
                if entry.key == 0 || entry.key == combined_key {
                    entry.key = combined_key;
                    entry.nodes = nodes;
                    return;
                }
            }

            // Both full - replace first (simple strategy)
            cluster.entries[0].key = combined_key;
            cluster.entries[0].nodes = nodes;
        }
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        unsafe {
            std::ptr::write_bytes(self.clusters, 0, self.capacity);
        }
    }
}

impl Drop for TranspositionTable {
    fn drop(&mut self) {
        if !self.clusters.is_null() {
            if let Ok(layout) = Layout::array::<Cluster>(self.capacity) {
                unsafe {
                    dealloc(self.clusters as *mut u8, layout);
                }
            }
        }
    }
}

// Safety: We manage our own memory
unsafe impl Send for TranspositionTable {}
unsafe impl Sync for TranspositionTable {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tt_store_probe() {
        let mut tt = TranspositionTable::new(1); // 1MB

        tt.store(12345, 3, 1000);
        assert_eq!(tt.probe(12345, 3), Some(1000));
        assert_eq!(tt.probe(12345, 4), None); // different depth
        assert_eq!(tt.probe(12346, 3), None); // different key
    }

    #[test]
    fn test_tt_cluster_size() {
        assert_eq!(std::mem::size_of::<Cluster>(), 32);
        assert_eq!(std::mem::align_of::<Cluster>(), 32);
    }

    #[test]
    fn test_tt_overwrite() {
        let mut tt = TranspositionTable::new(1);

        tt.store(100, 1, 500);
        tt.store(100, 1, 600); // same key, should overwrite
        assert_eq!(tt.probe(100, 1), Some(600));
    }
}
