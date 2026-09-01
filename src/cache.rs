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

/// The result of a refresh: how the cache should be updated.
pub enum RefreshOutcome<T> {
    /// A new value was fetched.
    Updated(T),
    /// Upstream revalidated the current value as unchanged (e.g. via a 304).
    Unchanged,
    /// The refresh failed; the current value is left untouched.
    Failed,
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

impl<T: Clone + Send + Sync + 'static> Cache<T> {
    /// Run `refresh` with the current cache entry and apply its outcome:
    /// [`RefreshOutcome::Updated`] replaces the cached value,
    /// [`RefreshOutcome::Unchanged`] re-timestamps the current entry, and
    /// [`RefreshOutcome::Failed`] leaves the cache untouched.
    pub async fn refresh<Fut>(&self, refresh: impl FnOnce(Option<Arc<Cached<T>>>) -> Fut)
    where
        Fut: Future<Output = RefreshOutcome<T>>,
    {
        match refresh(self.get_cloned()).await {
            RefreshOutcome::Updated(value) => self.update(value),
            RefreshOutcome::Unchanged => self.mark_as_fresh(),
            RefreshOutcome::Failed => {}
        }
    }

    /// Get the cached value, refreshing it with a stale-while-revalidate
    /// policy: an entry younger than `fresh_for` is returned directly; an
    /// entry younger than `stale_for` is returned while `refresh` runs in
    /// the background; otherwise (including when the cache is empty)
    /// `refresh` is awaited before returning the latest entry.
    ///
    /// `refresh` is passed the current cache entry (if any) and returns a
    /// [`RefreshOutcome`], which the cache applies (see [`Cache::refresh`]).
    pub async fn get_or_refresh<Fut>(
        &'static self,
        fresh_for: Duration,
        stale_for: Duration,
        refresh: impl FnOnce(Option<Arc<Cached<T>>>) -> Fut + Send + 'static,
    ) -> Option<Arc<Cached<T>>>
    where
        Fut: Future<Output = RefreshOutcome<T>> + Send + 'static,
    {
        self.get_usable_or_refresh(fresh_for, stale_for, |_| true, refresh)
            .await
    }

    /// Like [`Cache::get_or_refresh`], but a cached entry only counts as usable if
    /// `is_usable` returns true (e.g. it contains the data the caller needs).
    pub async fn get_usable_or_refresh<Fut>(
        &'static self,
        fresh_for: Duration,
        stale_for: Duration,
        is_usable: impl Fn(&T) -> bool,
        refresh: impl FnOnce(Option<Arc<Cached<T>>>) -> Fut + Send + 'static,
    ) -> Option<Arc<Cached<T>>>
    where
        Fut: Future<Output = RefreshOutcome<T>> + Send + 'static,
    {
        let existing = self.get_cloned();
        let mut await_refresh = true;
        if let Some(entry) = &existing {
            if is_usable(&entry.value) {
                let age = entry.cached_at.elapsed();
                if age <= fresh_for {
                    return existing;
                }
                if age <= stale_for {
                    await_refresh = false;
                }
            }
        }

        let handle = tokio::spawn(self.refresh(refresh));
        if await_refresh {
            let _ = handle.await;
        }
        self.get_cloned()
    }
}
