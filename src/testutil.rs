//! Helpers for tests.

use std::time::Duration;

/// CPU time this thread has used. Timing tests measure it instead of the
/// wall clock, so they don't fail when the other tests of a parallel run
/// take the cores.
pub fn thread_cpu_time() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid timespec the call fills in.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(result, 0, "no thread CPU clock");
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_time_counts_work_not_waiting() {
        let before = thread_cpu_time();
        std::thread::sleep(Duration::from_millis(50));
        let slept = thread_cpu_time() - before;
        assert!(slept < Duration::from_millis(20), "{slept:?}");
        let before = thread_cpu_time();
        let mut x = 1u64;
        for i in 0..20_000_000u64 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(i);
        }
        assert!(x != 0);
        assert!(thread_cpu_time() - before > Duration::from_millis(5));
    }
}
