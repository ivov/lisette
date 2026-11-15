//! `std::time::Instant::now` panics on wasm32-unknown-unknown.

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use std::time::Instant;

#[cfg(target_arch = "wasm32")]
mod zero {
    use std::ops::Add;
    use std::time::Duration;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub(crate) struct Instant;

    impl Instant {
        pub(crate) fn now() -> Self {
            Instant
        }

        pub(crate) fn elapsed(&self) -> Duration {
            Duration::ZERO
        }

        pub(crate) fn saturating_duration_since(&self, _earlier: Instant) -> Duration {
            Duration::ZERO
        }
    }

    impl Add<Duration> for Instant {
        type Output = Instant;

        fn add(self, _duration: Duration) -> Instant {
            Instant
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use zero::Instant;
