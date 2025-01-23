use std::collections::VecDeque;

/// A bounded [`VecDeque`]
pub struct BoundedVecDeque<T> {
    /// The inner [`VecDeque`]
    inner: VecDeque<T>,
    /// The maximum size of the [`VecDeque`]
    max_size: usize,
}

impl<T> BoundedVecDeque<T> {
    /// Create a new bounded [`VecDeque`] with the given maximum size
    pub fn new(max_size: usize) -> Self {
        Self {
            inner: VecDeque::new(),
            max_size,
        }
    }

    /// Push an item into the bounded [`VecDeque`], removing the oldest item if the
    /// maximum size is reached
    pub fn push(&mut self, item: T) {
        if self.inner.len() >= self.max_size {
            self.inner.pop_front();
        }
        self.inner.push_back(item);
    }
}
