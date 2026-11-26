//! Portable monotonic clock wrapper (`std::time::Instant` on native, no-op stub on `wasm32`).

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct Instant;

#[cfg(target_arch = "wasm32")]
impl Instant {
    #[inline]
    pub fn now() -> Self {
        Self
    }

    #[inline]
    pub fn elapsed(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
