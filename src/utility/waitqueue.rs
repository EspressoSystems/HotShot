use std::marker::PhantomData;

use async_std::sync::{Condvar, Mutex};

pub struct WaitQueue<T> {
    wait_limit: usize,
    queue: Mutex<Vec<T>>,
    condvar: Condvar,
    _phantom: PhantomData<T>,
}

impl<T> WaitQueue<T> {
    pub fn new(wait_limit: usize) -> Self {
        WaitQueue {
            wait_limit,
            queue: Mutex::new(Vec::new()),
            condvar: Condvar::new(),
            _phantom: PhantomData,
        }
    }

    pub async fn wait(&self) -> Vec<T> {
        let mut guard = self
            .condvar
            .wait_until(self.queue.lock().await, |queue| {
                queue.len() >= self.wait_limit
            })
            .await;
        let mut replacement = Vec::new();
        std::mem::swap(&mut replacement, &mut *guard);
        replacement
    }

    pub async fn push(&self, value: T) {
        let mut guard = self.queue.lock().await;
        guard.push(value);
        self.condvar.notify_one();
    }
}

pub struct WaitOnce<T> {
    item: Mutex<Option<T>>,
    condvar: Condvar,
    _phantom: PhantomData<T>,
}

impl<T> WaitOnce<T> {
    pub fn new() -> Self {
        WaitOnce {
            item: Mutex::new(None),
            condvar: Condvar::new(),
            _phantom: PhantomData,
        }
    }

    pub async fn wait_for(&self, closure: impl Fn(&T) -> bool) -> T {
        let mut x = None;
        while x.is_none() {
            let mut guard = self.condvar.wait(self.item.lock().await).await;
            if guard.is_some() && closure(guard.as_mut().unwrap()) {
                std::mem::swap(&mut x, &mut *guard);
            } else {
                let _ = std::mem::replace(&mut *guard, None);
            }
        }
        x.unwrap()
    }

    pub async fn wait(&self) -> T {
        self.wait_for(|_| true).await
    }

    pub async fn put(&self, item: T) {
        *self.item.lock().await = Some(item);
        self.condvar.notify_one();
    }
}
