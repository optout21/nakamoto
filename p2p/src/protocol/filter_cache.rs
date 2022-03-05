//! Compact filter cache.
use std::collections::BTreeMap;

use nakamoto_common::block::filter::BlockFilter;
use nakamoto_common::block::Height;

/// An in-memory compact filter cache with a fixed capacity.
#[derive(Debug, Default)]
pub struct FilterCache {
    /// Cache.
    cache: BTreeMap<Height, BlockFilter>,
    /// Cache size in bytes.
    size: usize,
    /// Cache capacity in bytes.
    capacity: usize,
}

impl FilterCache {
    /// Create a new filter cache.
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: BTreeMap::new(),
            size: 0,
            capacity,
        }
    }

    /// Return the size of the cache filters in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Return the cache capacity in bytes.
    ///
    /// ```
    /// use nakamoto_p2p::protocol::filter_cache::FilterCache;
    /// use nakamoto_common::block::filter::BlockFilter;
    ///
    /// let mut cache = FilterCache::new(32);
    /// assert_eq!(cache.capacity(), 32);
    /// ```
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Return the number of filters in the cache.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.len() == 0
    }

    /// Push a filter into the cache.
    ///
    /// Returns `false` if the filter was not added to the cache because it wasn't
    /// subsequent to the last filter height.
    ///
    /// ```
    /// use nakamoto_p2p::protocol::filter_cache::FilterCache;
    /// use nakamoto_common::block::filter::BlockFilter;
    ///
    /// let mut cache = FilterCache::new(8);
    ///
    /// assert!(cache.push(3, BlockFilter::new(&[1, 2, 3])));
    /// assert!(cache.push(4, BlockFilter::new(&[4, 5])));
    /// assert!(cache.push(5, BlockFilter::new(&[6])));
    ///
    /// assert_eq!(cache.len(), 3);
    /// assert_eq!(cache.size(), 6);
    ///
    /// assert!(cache.push(7, BlockFilter::new(&[7]))); // Non-contiguous height.
    /// assert_eq!(cache.len(), 4);
    ///
    /// assert!(cache.push(6, BlockFilter::new(&[8]))); // Hit max capacity.
    /// assert_eq!(cache.len(), 5);
    /// assert_eq!(cache.size(), 8);
    /// assert_eq!(cache.start(), Some(3));
    ///
    /// assert!(cache.push(8, BlockFilter::new(&[9]))); // Evict the first element.
    /// assert_eq!(cache.len(), 5);
    /// assert_eq!(cache.size(), 6);
    /// assert_eq!(cache.start(), Some(4));
    /// assert_eq!(cache.end(), Some(8));
    ///
    /// ```
    pub fn push(&mut self, height: Height, filter: BlockFilter) -> bool {
        assert!(self.size <= self.capacity);

        let size = filter.content.len();
        if size > self.capacity {
            return false;
        }

        self.cache.insert(height, filter);
        self.size += size;

        while self.size > self.capacity {
            if let Some(height) = self.cache.keys().cloned().next() {
                if let Some(filter) = self.cache.remove(&height) {
                    self.size -= filter.content.len();
                }
            }
        }
        true
    }

    /// Get the start height of the cache.
    ///
    /// ```
    /// use nakamoto_p2p::protocol::filter_cache::FilterCache;
    /// use nakamoto_common::block::filter::BlockFilter;
    ///
    /// let mut cache = FilterCache::new(32);
    ///
    /// cache.push(3, BlockFilter::new(&[1]));
    /// cache.push(4, BlockFilter::new(&[2]));
    /// cache.push(5, BlockFilter::new(&[3]));
    ///
    /// assert_eq!(cache.start(), Some(3));
    /// assert_eq!(cache.end(), Some(5));
    /// ```
    pub fn start(&self) -> Option<Height> {
        self.cache.keys().next().copied()
    }

    /// Get the end height of the cache.
    pub fn end(&self) -> Option<Height> {
        self.cache.keys().next_back().copied()
    }

    /// Iterate over cached filters.
    pub fn iter(&self) -> impl Iterator<Item = (&Height, &BlockFilter)> {
        self.cache.iter().map(|(h, b)| (h, b))
    }

    /// Iterate over cached heights.
    pub fn heights(&self) -> impl Iterator<Item = Height> + '_ {
        self.cache.keys().copied()
    }

    /// Get a filter in the cache by height.
    ///
    /// ```
    /// use nakamoto_p2p::protocol::filter_cache::FilterCache;
    /// use nakamoto_common::block::filter::BlockFilter;
    ///
    /// let mut cache = FilterCache::new(32);
    ///
    /// cache.push(3, BlockFilter::new(&[1]));
    /// cache.push(4, BlockFilter::new(&[2]));
    /// cache.push(5, BlockFilter::new(&[3]));
    ///
    /// assert_eq!(cache.get(&4).unwrap().content, vec![2]);
    /// assert_eq!(cache.get(&5).unwrap().content, vec![3]);
    /// assert_eq!(cache.get(&1), None);
    ///
    /// ```
    pub fn get(&self, height: &Height) -> Option<&BlockFilter> {
        self.cache.get(height)
    }

    /// Rollback the cache to a certain height. Drops all filters with a height greater
    /// than the given height.
    ///
    /// ```
    /// use nakamoto_p2p::protocol::filter_cache::FilterCache;
    /// use nakamoto_common::block::filter::BlockFilter;
    ///
    /// let mut cache = FilterCache::new(0);
    ///
    /// cache.push(3, BlockFilter::new(&[]));
    /// cache.push(4, BlockFilter::new(&[]));
    /// cache.push(5, BlockFilter::new(&[]));
    ///
    /// cache.rollback(4);
    /// assert_eq!(cache.end(), Some(4));
    ///
    /// cache.rollback(5);
    /// assert_eq!(cache.end(), Some(4));
    ///
    /// cache.rollback(1);
    /// assert_eq!(cache.end(), None);
    /// ```
    pub fn rollback(&mut self, height: Height) {
        while let Some(h) = self.end() {
            if h > height {
                if let Some(k) = self.cache.keys().cloned().next_back() {
                    if let Some(filter) = self.cache.remove(&k) {
                        self.size -= filter.content.len();
                    }
                }
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;

    #[derive(Clone, Debug)]
    enum Op {
        Push(BlockFilter),
        Rollback,
    }

    impl Op {
        fn apply(self, cache: &mut FilterCache, rng: &mut fastrand::Rng) {
            match self {
                Self::Push(filter) => {
                    if let Some(end) = cache.end() {
                        cache.push(end + 1, filter);
                    } else {
                        cache.push(rng.u64(..), filter);
                    }
                }
                Self::Rollback => {
                    if let (Some(start), Some(end)) = (cache.start(), cache.end()) {
                        cache.rollback(rng.u64(start - 1..=end + 1));
                    }
                }
            }
        }
    }

    impl Arbitrary for Op {
        fn arbitrary(g: &mut Gen) -> Self {
            let n = u8::arbitrary(g);

            match n % 4 {
                0..=2 => {
                    let content: Vec<_> = Arbitrary::arbitrary(g);
                    let filter = BlockFilter::new(&content);

                    Op::Push(filter)
                }
                3 => Op::Rollback,

                _ => unreachable! {},
            }
        }
    }

    #[quickcheck]
    fn prop_capacity(capacity: usize, operations: Vec<Op>, seed: u64) {
        let mut cache = FilterCache::new(capacity);
        let mut rng = fastrand::Rng::with_seed(seed);

        for op in operations.into_iter() {
            op.apply(&mut cache, &mut rng);

            let size = cache
                .cache
                .iter()
                .map(|(_, f)| f.content.len())
                .sum::<usize>();

            assert!(cache.size <= cache.capacity);
            assert!(size == cache.size);
        }
    }
}
