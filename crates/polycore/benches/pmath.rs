//! The cost of every `pmath` function against the platform's libm, on the arguments the
//! kernel produces (PC-12.perf): angles in [−2π, 2π], cosines in [−1, 1] for `acos` and
//! `asin`, points across the quadrants for `atan2` and `hypot`, square-root-like ranges for
//! `cbrt`, `ln` and `powf`, and modest arguments for the exponentials and hyperbolics. For the
//! functions with a fast path it also times the double-double path alone and counts the
//! arguments the fast path leaves to it. The first two rows time `portable_trig::sin_cos_memo`
//! (the revolved surface's sine and cosine) on the presearch grid it serves and on angles that
//! all miss its memo.
//!
//!     cargo bench -p polycore --bench pmath
//!
//! Prints one line per function: nanoseconds per call for pmath, the platform, their ratio,
//! the double-double path and the fast path's fallback rate. Each is the best of several
//! rounds over 2¹⁶ precomputed arguments, with the results folded into a sum the optimizer
//! cannot discard. The platform's functions are the only place in the workspace that calls
//! them; `clippy.toml` disallows them everywhere else, and they are allowed here, one by one,
//! because the benchmark exists to compare against them.

use std::hint::black_box;
use std::time::Instant;

use pmath::paths;

const N: usize = 1 << 16;
const ROUNDS: usize = 7;

/// splitmix64, so the arguments are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * ((self.next() >> 11) as f64 / (1u64 << 53) as f64)
    }

    fn log_uniform(&mut self, lo: f64, hi: f64) -> f64 {
        pmath::exp(self.uniform(pmath::ln(lo), pmath::ln(hi)))
    }

    fn points(&mut self, n: usize, lo: f64, hi: f64) -> Vec<(f64, f64)> {
        (0..n).map(|_| (self.uniform(lo, hi), self.uniform(lo, hi))).collect()
    }
}

/// Nanoseconds per call of `f` over `args`: the best round.
fn time<T: Copy>(args: &[T], f: impl Fn(T) -> f64) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        let mut sum = 0.0;
        for &a in args {
            sum += f(black_box(a));
        }
        black_box(sum);
        best = best.min(start.elapsed().as_nanos() as f64 / args.len() as f64);
    }
    best
}

/// The share of `args` the fast path leaves to the double-double path.
fn fallback<T: Copy>(args: &[T], fast: impl Fn(T) -> Option<f64>) -> f64 {
    let n = args.iter().filter(|&&a| fast(a).is_none()).count();
    100.0 * n as f64 / args.len() as f64
}

#[allow(clippy::disallowed_methods)]
fn platform_sin(x: f64) -> f64 {
    x.sin()
}
#[allow(clippy::disallowed_methods)]
fn platform_cos(x: f64) -> f64 {
    x.cos()
}
#[allow(clippy::disallowed_methods)]
fn platform_sin_cos(x: f64) -> f64 {
    let (s, c) = x.sin_cos();
    s + c
}
#[allow(clippy::disallowed_methods)]
fn platform_tan(x: f64) -> f64 {
    x.tan()
}
#[allow(clippy::disallowed_methods)]
fn platform_asin(x: f64) -> f64 {
    x.asin()
}
#[allow(clippy::disallowed_methods)]
fn platform_acos(x: f64) -> f64 {
    x.acos()
}
#[allow(clippy::disallowed_methods)]
fn platform_atan(x: f64) -> f64 {
    x.atan()
}
#[allow(clippy::disallowed_methods)]
fn platform_atan2((y, x): (f64, f64)) -> f64 {
    y.atan2(x)
}
#[allow(clippy::disallowed_methods)]
fn platform_exp(x: f64) -> f64 {
    x.exp()
}
#[allow(clippy::disallowed_methods)]
fn platform_exp2(x: f64) -> f64 {
    x.exp2()
}
#[allow(clippy::disallowed_methods)]
fn platform_exp_m1(x: f64) -> f64 {
    x.exp_m1()
}
#[allow(clippy::disallowed_methods)]
fn platform_ln(x: f64) -> f64 {
    x.ln()
}
#[allow(clippy::disallowed_methods)]
fn platform_ln_1p(x: f64) -> f64 {
    x.ln_1p()
}
#[allow(clippy::disallowed_methods)]
fn platform_log2(x: f64) -> f64 {
    x.log2()
}
#[allow(clippy::disallowed_methods)]
fn platform_log10(x: f64) -> f64 {
    x.log10()
}
#[allow(clippy::disallowed_methods)]
fn platform_log((x, b): (f64, f64)) -> f64 {
    x.log(b)
}
#[allow(clippy::disallowed_methods)]
fn platform_powf((x, y): (f64, f64)) -> f64 {
    x.powf(y)
}
#[allow(clippy::disallowed_methods)]
fn platform_hypot((x, y): (f64, f64)) -> f64 {
    x.hypot(y)
}
#[allow(clippy::disallowed_methods)]
fn platform_cbrt(x: f64) -> f64 {
    x.cbrt()
}
#[allow(clippy::disallowed_methods)]
fn platform_sinh(x: f64) -> f64 {
    x.sinh()
}
#[allow(clippy::disallowed_methods)]
fn platform_cosh(x: f64) -> f64 {
    x.cosh()
}
#[allow(clippy::disallowed_methods)]
fn platform_tanh(x: f64) -> f64 {
    x.tanh()
}
#[allow(clippy::disallowed_methods)]
fn platform_asinh(x: f64) -> f64 {
    x.asinh()
}
#[allow(clippy::disallowed_methods)]
fn platform_acosh(x: f64) -> f64 {
    x.acosh()
}
#[allow(clippy::disallowed_methods)]
fn platform_atanh(x: f64) -> f64 {
    x.atanh()
}

struct Row {
    name: &'static str,
    pmath: f64,
    platform: f64,
    slow: Option<f64>,
    fallback: Option<f64>,
}

fn row(name: &'static str, pmath: f64, platform: f64) -> Row {
    Row { name, pmath, platform, slow: None, fallback: None }
}

fn row_with_paths(name: &'static str, pmath: f64, platform: f64, slow: f64, fallback: f64) -> Row {
    Row { name, pmath, platform, slow: Some(slow), fallback: Some(fallback) }
}

fn main() {
    let mut rng = Rng(0x005E_ED12);
    let tau = std::f64::consts::TAU;
    let angles: Vec<f64> = (0..N).map(|_| rng.uniform(-tau, tau)).collect();
    let cosines: Vec<f64> = (0..N).map(|_| rng.uniform(-1.0, 1.0)).collect();
    let ratios: Vec<f64> = (0..N).map(|_| rng.uniform(-20.0, 20.0)).collect();
    let points = rng.points(N, -5.0, 5.0);
    let small: Vec<f64> = (0..N).map(|_| rng.uniform(-5.0, 5.0)).collect();
    let tiny: Vec<f64> = (0..N).map(|_| rng.uniform(-1e-3, 1e-3)).collect();
    let positive: Vec<f64> = (0..N).map(|_| rng.log_uniform(1e-3, 1e3)).collect();
    let powers: Vec<(f64, f64)> = (0..N).map(|_| (rng.log_uniform(0.1, 10.0), rng.uniform(-3.0, 3.0))).collect();
    let bases: Vec<(f64, f64)> = (0..N).map(|_| (rng.log_uniform(1e-3, 1e3), rng.log_uniform(1.5, 100.0))).collect();
    let above_one: Vec<f64> = (0..N).map(|_| rng.log_uniform(1.0, 30.0)).collect();
    let unit: Vec<f64> = (0..N).map(|_| rng.uniform(-0.999, 0.999)).collect();

    // `presearch`'s pattern on a revolved surface: a grid of 101 angles over a range, each
    // taken 101 times in turn (`sin_cos_memo` exists for it); the ranges alternate between the
    // full turn and a hinted part of it, as the intersection curves' projections do.
    let grid: Vec<f64> = (0..N)
        .map(|i| {
            let (call, j) = (i / (101 * 101), (i % 101) as f64 / 100.0);
            let (v0, v1) = if call % 2 == 0 { (0.0, tau) } else { (0.3 + call as f64, 0.9 + call as f64) };
            v0 * (1.0 - j) + v1 * j
        })
        .collect();

    let rows = vec![
        row(
            "sin_cos_memo, presearch grid",
            time(&grid, |x| {
                let (s, c) = truck_modeling::portable_trig::sin_cos_memo(x);
                s + c
            }),
            time(&grid, platform_sin_cos),
        ),
        row(
            "sin_cos_memo [-2π, 2π] (misses)",
            time(&angles, |x| {
                let (s, c) = truck_modeling::portable_trig::sin_cos_memo(x);
                s + c
            }),
            time(&angles, platform_sin_cos),
        ),
        row_with_paths(
            "sin [-2π, 2π]",
            time(&angles, pmath::sin),
            time(&angles, platform_sin),
            time(&angles, |x| paths::sin_cos_slow(x).0.0),
            fallback(&angles, |x| paths::sin_cos_fast(x).and_then(|(s, _)| s.round())),
        ),
        row_with_paths(
            "cos [-2π, 2π]",
            time(&angles, pmath::cos),
            time(&angles, platform_cos),
            time(&angles, |x| paths::sin_cos_slow(x).1.0),
            fallback(&angles, |x| paths::sin_cos_fast(x).and_then(|(_, c)| c.round())),
        ),
        row_with_paths(
            "sin_cos [-2π, 2π]",
            time(&angles, |x| {
                let (s, c) = pmath::sin_cos(x);
                s + c
            }),
            time(&angles, platform_sin_cos),
            time(&angles, |x| {
                let (s, c) = paths::sin_cos_slow(x);
                s.0 + c.0
            }),
            fallback(&angles, |x| paths::sin_cos_fast(x).and_then(|(s, c)| s.round().and(c.round()))),
        ),
        row_with_paths(
            "tan [-2π, 2π]",
            time(&angles, pmath::tan),
            time(&angles, platform_tan),
            time(&angles, |x| paths::tan_slow(x).0),
            fallback(&angles, |x| paths::tan_fast(x).and_then(|t| t.round())),
        ),
        row_with_paths(
            "asin [-1, 1]",
            time(&cosines, pmath::asin),
            time(&cosines, platform_asin),
            time(&cosines, |x| paths::asin_slow(x).0),
            fallback(&cosines, |x| paths::asin_fast(x).and_then(|a| a.round())),
        ),
        row_with_paths(
            "acos [-1, 1]",
            time(&cosines, pmath::acos),
            time(&cosines, platform_acos),
            time(&cosines, |x| paths::acos_slow(x).0),
            fallback(&cosines, |x| paths::acos_fast(x).and_then(|a| a.round())),
        ),
        row_with_paths(
            "atan [-20, 20]",
            time(&ratios, pmath::atan),
            time(&ratios, platform_atan),
            time(&ratios, |x| paths::atan_slow(x).0),
            fallback(&ratios, |x| paths::atan_fast(x).and_then(|a| a.round())),
        ),
        row_with_paths(
            "atan2 [-5, 5]²",
            time(&points, |(y, x)| pmath::atan2(y, x)),
            time(&points, platform_atan2),
            time(&points, |(y, x)| paths::atan2_slow(y, x).0),
            fallback(&points, |(y, x)| paths::atan2_fast(y, x).and_then(|a| a.round())),
        ),
        row("hypot [-5, 5]²", time(&points, |(x, y)| pmath::hypot(x, y)), time(&points, platform_hypot)),
        row("cbrt [1e-3, 1e3]", time(&positive, pmath::cbrt), time(&positive, platform_cbrt)),
        row("exp [-5, 5]", time(&small, pmath::exp), time(&small, platform_exp)),
        row("exp2 [-5, 5]", time(&small, pmath::exp2), time(&small, platform_exp2)),
        row("exp_m1 [-1e-3, 1e-3]", time(&tiny, pmath::exp_m1), time(&tiny, platform_exp_m1)),
        row("ln [1e-3, 1e3]", time(&positive, pmath::ln), time(&positive, platform_ln)),
        row("ln_1p [-1e-3, 1e-3]", time(&tiny, pmath::ln_1p), time(&tiny, platform_ln_1p)),
        row("log2 [1e-3, 1e3]", time(&positive, pmath::log2), time(&positive, platform_log2)),
        row("log10 [1e-3, 1e3]", time(&positive, pmath::log10), time(&positive, platform_log10)),
        row("log [1e-3, 1e3] base [1.5, 100]", time(&bases, |(x, b)| pmath::log(x, b)), time(&bases, platform_log)),
        row("powf [0.1, 10]^[-3, 3]", time(&powers, |(x, y)| pmath::powf(x, y)), time(&powers, platform_powf)),
        row("sinh [-5, 5]", time(&small, pmath::sinh), time(&small, platform_sinh)),
        row("cosh [-5, 5]", time(&small, pmath::cosh), time(&small, platform_cosh)),
        row("tanh [-5, 5]", time(&small, pmath::tanh), time(&small, platform_tanh)),
        row("asinh [-5, 5]", time(&small, pmath::asinh), time(&small, platform_asinh)),
        row("acosh [1, 30]", time(&above_one, pmath::acosh), time(&above_one, platform_acosh)),
        row("atanh [-0.999, 0.999]", time(&unit, pmath::atanh), time(&unit, platform_atanh)),
    ];

    println!("{:<34} {:>10} {:>10} {:>7} {:>12} {:>10}", "function [arguments]", "pmath ns", "libm ns", "ratio", "dd path ns", "fallback");
    for r in &rows {
        let slow = r.slow.map_or(String::from("-"), |s| format!("{s:.1}"));
        let fallback = r.fallback.map_or(String::from("-"), |f| format!("{f:.3} %"));
        println!("{:<34} {:>10.1} {:>10.1} {:>7.2} {:>12} {:>10}", r.name, r.pmath, r.platform, r.pmath / r.platform, slow, fallback);
    }
}
