//! Flush-to-zero for the threads that run DSP. Feedback filters (reverb combs, biquads, envelopes) fade towards
//! zero after the input goes silent and end up in subnormal numbers, which x86 processes many times slower than
//! normal ones: the audio callback then spikes exactly when a sound dies away. With FTZ and DAZ on, results and
//! inputs below the smallest normal f32 (about 1e-38, far below anything audible) become zero.
//!
//! The setting lives in the MXCSR register of each thread, so it must be turned on by the thread that renders
//! (the audio callback, the offline bounce) and turned back off when a borrowed thread is given back. Only x86_64
//! is handled; on other architectures these functions do nothing.

use std::marker::PhantomData;

/// MXCSR bit 15 (FTZ: subnormal results become zero) and bit 6 (DAZ: subnormal inputs are read as zero).
#[cfg(target_arch = "x86_64")]
const FTZ_DAZ: u32 = 0x8040;

/// The floating-point control state of one thread, saved to put it back later on the same thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpControl(u32);

impl FpControl {
    /// True when this state flushes subnormal results to zero.
    pub fn flushes_to_zero(self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            self.0 & FTZ_DAZ == FTZ_DAZ
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }
}

/// The current thread's floating-point control state.
pub fn fp_control() -> FpControl {
    #[cfg(target_arch = "x86_64")]
    {
        FpControl(read_mxcsr())
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        FpControl(0)
    }
}

/// Turn flush-to-zero and denormals-are-zero on for the current thread and return the state it had before.
pub fn enable_flush_to_zero() -> FpControl {
    #[cfg(target_arch = "x86_64")]
    {
        let old = read_mxcsr();
        if old & FTZ_DAZ != FTZ_DAZ {
            write_mxcsr(old | FTZ_DAZ);
        }
        FpControl(old)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        FpControl(0)
    }
}

/// Put back a state saved on this same thread by `fp_control` or `enable_flush_to_zero`.
#[cfg(target_arch = "x86_64")]
pub fn restore_fp_control(saved: FpControl) {
    if read_mxcsr() != saved.0 {
        write_mxcsr(saved.0);
    }
}

/// Put back a state saved on this same thread by `fp_control` or `enable_flush_to_zero`.
#[cfg(not(target_arch = "x86_64"))]
pub fn restore_fp_control(_saved: FpControl) {}

/// Flush-to-zero on the current thread for as long as the guard lives; dropping it (also while unwinding from a
/// panic) gives the thread back the state it had. Bind it to a named variable such as `_ftz`: bound to `_` alone it drops at once.
#[must_use = "flush-to-zero ends when the guard is dropped"]
pub struct FlushToZero {
    saved: FpControl,
    /// MXCSR belongs to one thread: the guard must be dropped where it was created.
    _same_thread: PhantomData<*const ()>,
}

impl FlushToZero {
    pub fn enable() -> FlushToZero {
        FlushToZero { saved: enable_flush_to_zero(), _same_thread: PhantomData }
    }
}

impl Drop for FlushToZero {
    fn drop(&mut self) {
        restore_fp_control(self.saved);
    }
}

#[cfg(target_arch = "x86_64")]
fn read_mxcsr() -> u32 {
    let mut csr: u32 = 0;
    // SAFETY: stmxcsr only stores the 32-bit SSE control register into `csr`, a live, aligned u32 on our stack.
    // The deprecated SSE getcsr intrinsic does the same thing; inline asm avoids the deprecation.
    unsafe {
        core::arch::asm!("stmxcsr dword ptr [{p}]", p = in(reg) &raw mut csr, options(nostack, preserves_flags));
    }
    csr
}

#[cfg(target_arch = "x86_64")]
fn write_mxcsr(csr: u32) {
    // SAFETY: ldmxcsr loads the SSE control register from `csr`. Every value written here was read by stmxcsr,
    // possibly with the FTZ and DAZ bits set, so no reserved bit is ever set (which would fault). The asm is not
    // marked pure/nomem, so the compiler keeps it in program order with the memory accesses around it.
    unsafe {
        core::arch::asm!("ldmxcsr dword ptr [{p}]", p = in(reg) &raw const csr, options(nostack, preserves_flags));
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;

    fn tiny_product() -> f32 {
        std::hint::black_box(f32::MIN_POSITIVE) * std::hint::black_box(0.25)
    }

    #[test]
    fn flush_to_zero_is_per_thread() {
        let a = std::thread::spawn(|| {
            let before = enable_flush_to_zero();
            (before, tiny_product(), fp_control())
        })
        .join()
        .unwrap();
        let b = std::thread::spawn(tiny_product).join().unwrap();
        assert_eq!(a.1, 0.0, "with flush-to-zero the product must be zero, got {:e}", a.1);
        assert!(b != 0.0 && b.is_subnormal(), "without flush-to-zero the product must be subnormal, got {b:e}");
        assert!(!a.0.flushes_to_zero(), "a new thread starts with flush-to-zero off");
        assert!(a.2.flushes_to_zero());
    }

    #[test]
    fn guard_restores_the_previous_state_also_on_panic() {
        std::thread::spawn(|| {
            let before = fp_control();
            {
                let _g = FlushToZero::enable();
                assert!(fp_control().flushes_to_zero());
                assert_eq!(tiny_product(), 0.0);
            }
            assert_eq!(fp_control(), before);
            assert!(tiny_product().is_subnormal());
            let r = std::panic::catch_unwind(|| {
                let _g = FlushToZero::enable();
                panic!("boom")
            });
            assert!(r.is_err());
            assert_eq!(fp_control(), before);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn restore_puts_back_an_already_enabled_state() {
        std::thread::spawn(|| {
            enable_flush_to_zero();
            let s = fp_control();
            {
                let _g = FlushToZero::enable();
            }
            assert_eq!(fp_control(), s);
            assert!(s.flushes_to_zero());
        })
        .join()
        .unwrap();
    }
}
