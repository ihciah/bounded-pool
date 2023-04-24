//! A generic but simple pool implemention.

use std::sync::{Arc, Weak};

use parking_lot::Mutex;

/// A Vec based buffer pool.
#[derive(Default)]
pub struct Pool<T, F = fn() -> T> {
    cached: Vec<T>,
    limit: usize,

    default: F,
}
pub type DynPool<T> = Pool<T, Box<dyn Fn() -> T + Send + Sync + 'static>>;

impl<T, F> std::fmt::Debug for Pool<T, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pool with limit {} and size {}",
            self.limit,
            self.cached.len()
        )
    }
}

impl<T, F> Pool<T, F>
where
    F: Fn() -> T,
{
    #[inline]
    pub fn new(limit: usize, pre_allocate: usize, initialize: bool, default: F) -> Self {
        let mut cached = Vec::with_capacity(pre_allocate);
        if initialize {
            for _ in 0..pre_allocate {
                cached.push(default());
            }
        }

        Self {
            cached,
            limit,
            default,
        }
    }

    pub fn pop(&mut self) -> T {
        if let Some(val) = self.cached.pop() {
            return val;
        }
        (self.default)()
    }
}

impl<T, F> Pool<T, F> {
    #[inline]
    pub fn try_pop(&mut self) -> Option<T> {
        self.cached.pop()
    }

    #[inline]
    pub fn push(&mut self, val: T) {
        if self.cached.len() < self.limit {
            self.cached.push(val);
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.cached.clear();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.cached.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cached.is_empty()
    }

    #[inline]
    pub fn limit(&self) -> usize {
        self.limit
    }
}

impl<T> Pool<T, fn() -> T>
where
    T: Default,
{
    #[inline]
    pub fn new_with_default(limit: usize) -> Self {
        Self::new(limit, 0, false, T::default)
    }
}

/// Shared Pool.
#[derive(Default)]
pub struct SharedPool<T, F = fn() -> T>(Arc<Mutex<Pool<T, F>>>);
pub type DynSharedPool<T> = SharedPool<T, Box<dyn Fn() -> T + Send + Sync + 'static>>;

impl<T, F> std::fmt::Debug for SharedPool<T, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SharedPool with limit {} and size {}",
            self.limit(),
            self.len()
        )
    }
}

impl<T, F> Clone for SharedPool<T, F> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T, F> SharedPool<T, F>
where
    F: Fn() -> T,
{
    #[inline]
    pub fn new(limit: usize, pre_allocate: usize, initialize: bool, default: F) -> Self {
        Self(Arc::new(Mutex::new(Pool::new(
            limit,
            pre_allocate,
            initialize,
            default,
        ))))
    }

    #[inline]
    pub fn pop(&self) -> T {
        self.0.lock().pop()
    }

    #[inline]
    pub fn pop_guarded(&self) -> Guard<T, F> {
        Guard {
            pool: Arc::downgrade(&self.0),
            val: Some(self.pop()),
        }
    }
}

impl<T> SharedPool<T, fn() -> T>
where
    T: Default,
{
    #[inline]
    pub fn new_with_default(limit: usize) -> Self {
        Self(Arc::new(Mutex::new(Pool::new_with_default(limit))))
    }
}

impl<T, F> SharedPool<T, F> {
    #[inline]
    pub fn try_pop(&self) -> Option<T> {
        self.0.lock().try_pop()
    }

    #[inline]
    pub fn try_pop_guarded(&self) -> Option<Guard<T, F>> {
        self.0.lock().try_pop().map(|inner| Guard {
            pool: Arc::downgrade(&self.0),
            val: Some(inner),
        })
    }

    #[inline]
    pub fn push(&self, val: T) {
        self.0.lock().push(val)
    }

    #[inline]
    pub fn clear(&self) {
        self.0.lock().clear();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.lock().len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.lock().is_empty()
    }

    #[inline]
    pub fn limit(&self) -> usize {
        self.0.lock().limit()
    }
}

#[derive(Clone, Debug)]
pub struct Guard<T, F = fn() -> T> {
    pool: Weak<Mutex<Pool<T, F>>>,
    val: Option<T>,
}

impl<T, F> Guard<T, F> {
    #[inline]
    pub fn into_inner(mut self) -> T {
        self.val.take().unwrap()
    }
}

impl<T, F> std::ops::Deref for Guard<T, F> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.val.as_ref().unwrap()
    }
}

impl<T, F> std::ops::DerefMut for Guard<T, F> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.val.as_mut().unwrap()
    }
}

impl<T, F> Drop for Guard<T, F> {
    #[inline]
    fn drop(&mut self) {
        if let Some(pool) = self.pool.upgrade() {
            if let Some(val) = self.val.take() {
                SharedPool(pool).push(val);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pool() {
        type BufferPool = SharedPool<()>;
        let pool = BufferPool::new_with_default(10);
        assert!(pool.is_empty());
        let buf = pool.pop_guarded();
        drop(buf);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn dynamic_pool() {
        type DynBufferPool = SharedPool<(), Box<dyn Fn()>>;
        let pool = DynBufferPool::new(10, 0, false, Box::new(|| ()));
        assert!(pool.is_empty());
        pool.pop();
        assert!(pool.is_empty());
        pool.push(());
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn sized() {
        type BufferPool = Pool<u8>;
        let mut pool = BufferPool::new_with_default(3);
        assert!(pool.is_empty());
        for _ in 0..10 {
            let element = pool.pop();
            pool.push(element);
        }
        assert_eq!(pool.len(), 1);

        for _ in 0..10 {
            pool.push(0);
        }
        assert_eq!(pool.len(), 3);
    }

    #[test]
    fn new_pool() {
        type BufferPool = SharedPool<()>;
        let _pool = BufferPool::new_with_default(10);
        let _pool = BufferPool::new(10, 0, false, || ());

        type DynBufferPool = Pool<u8, Box<dyn Fn() -> u8>>;
        let number = 100;
        let _pool = DynBufferPool::new(10, 0, false, Box::new(move || number));
    }
}
