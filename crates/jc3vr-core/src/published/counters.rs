//! Event counts for a readout: [`Counter`], and [`counters!`](crate::counters), which declares a
//! group of them once as a live struct and a snapshot struct with the same fields.

use std::sync::atomic::{AtomicU64, Ordering};

/// A count of events, bumped from any thread and read for a readout. Relaxed: a count orders
/// nothing, and a reader may see one count of a group before another's matching bump.
#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Count one event.
    #[inline]
    pub fn bump(&self) {
        self.add(1);
    }

    /// Count `n` events.
    #[inline]
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// The count, reset to zero.
    pub fn take(&self) -> u64 {
        self.0.swap(0, Ordering::Relaxed)
    }
}

/// Declare a group of [`Counter`]s as a live struct with a `const fn new`, and a snapshot struct
/// of the same fields as `u64`s, which `snapshot` fills for a readout. The fields' doc comments
/// carry over to both. The snapshot takes the live struct's visibility, or its own when written
/// `=> pub struct Snapshot`, for a readout read beyond the crate that counts.
///
/// ```
/// jc3vr_core::counters! {
///     /// The empty-gun checks' counts.
///     pub struct EmptyCounters => EmptyCounts {
///         /// Shots held back.
///         suppressed,
///         dry_fires,
///     }
/// }
///
/// static COUNTS: EmptyCounters = EmptyCounters::new();
/// COUNTS.dry_fires.bump();
/// assert_eq!(COUNTS.snapshot(), EmptyCounts { suppressed: 0, dry_fires: 1 });
/// ```
#[macro_export]
macro_rules! counters {
    (
        $(#[$meta:meta])*
        $vis:vis struct $live:ident => $snapshot_vis:vis struct $snapshot:ident {
            $($(#[$field_meta:meta])* $field:ident),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Default)]
        $vis struct $live {
            $($(#[$field_meta])* pub $field: $crate::published::Counter,)*
        }

        impl $live {
            pub const fn new() -> Self {
                Self {
                    $($field: $crate::published::Counter::new(),)*
                }
            }

            /// Every count, read one after another.
            #[allow(dead_code)]
            pub fn snapshot(&self) -> $snapshot {
                $snapshot {
                    $($field: self.$field.get(),)*
                }
            }

            /// Reset every count to zero.
            #[allow(dead_code)]
            pub fn reset(&self) {
                $(self.$field.take();)*
            }
        }

        $(#[$meta])*
        #[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
        $snapshot_vis struct $snapshot {
            $($(#[$field_meta])* pub $field: u64,)*
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis struct $live:ident => $snapshot:ident {
            $($(#[$field_meta:meta])* $field:ident),* $(,)?
        }
    ) => {
        $crate::counters! {
            $(#[$meta])*
            $vis struct $live => $vis struct $snapshot {
                $($(#[$field_meta])* $field),*
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::counters! {
        struct Live => Snapshot {
            first,
            /// A documented field.
            second,
        }
    }

    #[test]
    fn the_snapshot_reads_each_count_and_reset_zeroes_them() {
        let live = Live::new();
        live.first.bump();
        live.second.add(3);
        assert_eq!(
            live.snapshot(),
            Snapshot {
                first: 1,
                second: 3
            }
        );
        live.reset();
        assert_eq!(live.snapshot(), Snapshot::default());
    }

    #[test]
    fn take_returns_the_count_and_zeroes_it() {
        let counter = Counter::new();
        counter.add(4);
        assert_eq!(counter.take(), 4);
        assert_eq!(counter.get(), 0);
    }
}
