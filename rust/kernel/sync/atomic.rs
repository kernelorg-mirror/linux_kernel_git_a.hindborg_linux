// SPDX-License-Identifier: GPL-2.0

//! Atomic primitives.
//!
//! These primitives have the same semantics as their C counterparts: and the precise definitions of
//! semantics can be found at [`LKMM`]. Note that Linux Kernel Memory (Consistency) Model is the
//! only model for Rust code in kernel, and Rust's own atomics should be avoided.
//!
//! # Data races
//!
//! [`LKMM`] atomics have different rules regarding data races:
//!
//! - A normal write from C side is treated as an atomic write if
//!   CONFIG_KCSAN_ASSUME_PLAIN_WRITES_ATOMIC=y.
//! - Mixed-size atomic accesses don't cause data races.
//!
//! [`LKMM`]: srctree/tools/memory-model/

pub mod generic;
pub mod ops;
pub mod ordering;

pub use generic::Atomic;
pub use ordering::{Acquire, Full, Relaxed, Release};

// SAFETY: `i32` has the same size and alignment with itself, and is round-trip transmutable to
// itself.
unsafe impl generic::AllowAtomic for i32 {
    type Repr = i32;
}

// SAFETY: `i32` is always sound to transmute back to itself.
unsafe impl generic::AllowAtomicArithmetic for i32 {
    type Delta = i32;

    fn delta_into_repr(d: Self::Delta) -> Self::Repr {
        d
    }
}

// SAFETY: `i64` has the same size and alignment with itself, and is round-trip transmutable to
// itself.
unsafe impl generic::AllowAtomic for i64 {
    type Repr = i64;
}

// SAFETY: `i64` is always sound to transmute back to itself.
unsafe impl generic::AllowAtomicArithmetic for i64 {
    type Delta = i64;

    fn delta_into_repr(d: Self::Delta) -> Self::Repr {
        d
    }
}
