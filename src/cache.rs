//! Generic in-memory cache with a stale-while-revalidate refresh policy.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A cached value with the time it was cached (or last revalidated) at.
pub struct Cached<T> {
    pub cached_at: Instant,
    pub value: T,
}

impl<T> std::ops::Deref for Cached<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

/// A process-global cache slot holding a single (atomically replaced) value.
pub struct Cache<T>(Mutex<Option<Arc<Cached<T>>>>);

impl<T> Cache<T> {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn get_cloned(&self) -> Option<Arc<Cached<T>>> {
        self.0.lock().unwrap().clone()
    }

    /// Replace the cached value, timestamping it with the current time.
    pub fn update(&self, value: T) {
        *self.0.lock().unwrap() = Some(Arc::new(Cached {
            cached_at: Instant::now(),
            value,
        }));
    }

    /// Re-timestamp the current entry without replacing its value
    /// (e.g. after upstream revalidated as unchanged via a 304).
    pub fn mark_as_fresh(&self)
    where
        T: Clone,
    {
        let mut inner = self.0.lock().unwrap();
        if let Some(entry) = inner.take() {
            *inner = Some(Arc::new(Cached {
                cached_at: Instant::now(),
                value: entry.value.clone(),
            }));
        }
    }
}

impl<T: Send + Sync + 'static> Cache<T> {
    /// Get the cached value, refreshing it with a stale-while-revalidate
    /// policy: an entry younger than `fresh_for` is returned directly; an
    /// entry younger than `stale_for` is returned while `refresh` runs in
    /// the background; otherwise (including when the cache is empty)
    /// `refresh` is awaited before returning the latest entry.
    ///
    /// `refresh` is responsible for storing its result via [`Cache::update`]
    /// (or [`Cache::mark_as_fresh`]).
    pub async fn fresh<Fut>(
        &self,
        fresh_for: Duration,
        stale_for: Duration,
        refresh: impl FnOnce() -> Fut,
    ) -> Option<Arc<Cached<T>>>
    where
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.fresh_if(fresh_for, stale_for, |_| true, refresh).await
    }

    /// Like [`Cache::fresh`], but a cached entry only counts as usable if
    /// `is_usable` returns true (e.g. it contains the data the caller needs).
    pub async fn fresh_if<Fut>(
        &self,
        fresh_for: Duration,
        stale_for: Duration,
        is_usable: impl Fn(&T) -> bool,
        refresh: impl FnOnce() -> Fut,
    ) -> Option<Arc<Cached<T>>>
    where
        Fut: Future<Output = ()> + Send + 'static,
    {
        let mut await_refresh = true;
        if let Some(entry) = self.get_cloned() {
            if is_usable(&entry.value) {
                let age = entry.cached_at.elapsed();
                if age <= fresh_for {
                    return Some(entry);
                }
                if age <= stale_for {
                    await_refresh = false;
                }
            }
        }

        let handle = tokio::spawn(refresh());
        if await_refresh {
            let _ = handle.await;
        }
        self.get_cloned()
    }
}
