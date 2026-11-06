//! The fast paths: a cheap approximation with a proven error bound, returned only when every
//! value within that bound rounds to the same float (Ziv's rounding test); the double-double
//! path in `lib.rs` decides the rest. The two agree bit for bit wherever the fast path answers,
//! since both are then the correctly rounded value, and every input the double-double path
//! could misround (a true value within 2⁻⁹⁶ of a rounding boundary) is far inside the fast
//! path's bound and so always reaches it. `polycore`'s `pmath_tests` compare the two over the
//! reference cases, the kernel's angles and large random sets, and check the bounds.
//!
//! The arithmetic is plain IEEE 754, no fused multiply-add (Rust does not contract), so the
//! bits are the same on every platform, as in `lib.rs`.
//!
//! Each path produces an [`Approx`]: `hi + lo` within `err` (absolute) of the true value. The
//! bounds below use `u = 2⁻⁵³` (half an ULP of 1) and the rule that a rounded operation is
//! within `u` times its result of the exact one.
//!
//! # Sine and cosine
//!
//! `x` is reduced about the nearest multiple `q` of π/256 with the pieces of π/2 of `lib.rs`
//! scaled by 2⁻⁷ (exact): `q` has at most 21 bits (|x| ≤ 20000) and each piece at most 32, so
//! every product is exact, and `x − q·P₁` is exact by Sterbenz's lemma; the rest is summed as
//! `rh + rl` with `|r − (rh + rl)| ≤ u·(|t| + |rl|) + |q|·2⁻¹⁴⁷` (`t` the first partial sum,
//! 2⁻¹⁴⁷ what π/256 is beyond its four pieces). The multiple is `128·quadrant + k` with
//! |k| ≤ 64, so `x = quadrant·π/2 + a + r` with `a = k·π/256`, |a| ≤ π/4 and |r| ≤ π/512. Then
//! with `S = sin a`, `C = cos a` from the table (double-doubles, within 2⁻¹⁰⁵) and the series
//! `cos r = 1 + pc`, `sin r = rh + sr` to r⁶ and r⁷ (the next terms are below 2⁻⁷⁴ and
//! 2⁻⁷⁷·|rh|):
//!
//! ```text
//! sin(a + r) = S·cos r + C·sin r + rl·cos(a + rh)
//!            = Sh + [Ch·rh] + Sl + Sh·pc + Ch·sr + Cl·rh + rl·(Ch − Sh·rh) + …
//! cos(a + r) = Ch − [Sh·rh] + Cl + Ch·pc − Sh·sr − Sl·rh − rl·(Sh + Ch·rh) + …
//! ```
//!
//! The bracketed product is exact (Dekker) and its leading part is added to `Sh` (`Ch`) with
//! an exact two-sum (the fast one: `|Sh| ≥ |Ch·rh|` or `Sh = 0`, and `Ch ≥ 0.7`); the rest is
//! summed in plain arithmetic, in a tree so that the additions overlap. With z = rh² ≤ 3.8·10⁻⁵:
//! |pc| ≤ 1.9·10⁻⁵ = 2⁻¹⁵·⁷ with error 3u·|pc|; |sr| ≤ 6.3·10⁻⁶·|rh| with error 6u·|sr|; the
//! six additions of the rest each round within u of a partial sum below 2⁻¹⁵·⁷·|Sh| (sine) or
//! 2⁻¹⁵·⁷·|Ch| (cosine); the dropped `Sl·pc`, `Cl·pc`, `Sl·sr`, `Cl·sr`, `rl²` terms are below
//! 2⁻⁶⁸·⁷·|Sh| and 2⁻⁶⁸·⁷·|Ch|; the neglected part of the `rl` term is below 2⁻¹⁴·|rl|
//! (added to the reduction's error). Altogether the sine is within 2⁻⁶⁵·³·|Sh| + 2⁻⁶⁷·⁷·|rh|
//! and the cosine within 2⁻⁶⁵·³·|Ch| of the true value, plus the reduction's error (the
//! derivative is at most 1). Since |Sh| ≤ 2·|sin(a + r)| (the nearest table angle is within
//! half a step), |rh| ≤ 1.0001·|sin(a + r)| and |Ch| ≤ 1.43·|cos(a + r)|, both are within
//! 2⁻⁶⁴·²·|result|; the bound used is 2⁻⁶³·|hi| (`E_SIN`, a margin above 2), and the fast
//! path then fails to decide about one argument in a thousand.
//!
//! # Tangent
//!
//! The quotient of the two approximations with one Dekker correction (`q = sh/ch`, the
//! remainder `sh − q·ch + sl − q·cl` over `ch`): within 2⁻¹⁰⁰·|q| of `(sh + sl)/(ch + cl)`,
//! and the true tangent within `(err_s + |q|·err_c)/|ch|` of that.
//!
//! # Arc tangent
//!
//! `t = y/x` in `[0, 1]` (the larger magnitude below) as a quotient with one Dekker
//! correction, within 2⁻¹⁰⁰·|t|. With `c = k/32` the nearest multiple of 1/32 (`A = atan c`
//! from the table, within 2⁻¹⁰⁵), `atan t = A + atan v`, `v = (t − c)/(1 + t·c)`, |v| ≤ 1/64:
//! `t − c` is exact (Sterbenz), `1 + t·c` a two-sum of a Dekker product, the quotient as
//! above (within 2⁻¹⁰⁰·|vh|), and the series `atan v = vh + vl·(1 − vh²) + vh·pa` to v¹¹ (the
//! next term is below 2⁻⁷⁵·|vh|). |pa| ≤ 8.1·10⁻⁵ = 2⁻¹³·⁶ with error 5u·|pa|; the four
//! additions of the rest round within u of partial sums below 2⁻¹³·⁶·|vh|: within
//! 2⁻⁶⁴·|vh| + 2⁻¹⁰⁰·|hi| of `atan t`, and |vh| ≤ |hi|. The bound used is 2⁻⁶²·|hi|
//! (`E_ATAN`). The quadrant adds `π/2 ± a` or `π − a` (two-sum with the double-double
//! constant, within 2⁻⁶⁵·|hi| of exact, added to the bound). `asin` and `acos` take
//! `√(1 − x²)` as a double-double within 2⁻¹⁰³ (the difference and the square root's Dekker
//! correction), which moves the angle by less than 2⁻¹⁰²·|hi|; `E_ATAN` has room for that.
//!
//! # Rounding test
//!
//! `hi + (lo − err)` and `hi + (lo + err)` are rounded sums; the test passes only when they
//! are the same float, which then is the rounding of everything in between, the true value
//! included. The roundings of `lo ± err` themselves narrow the interval by at most
//! `u·(|lo| + err)`, which [`Approx::new`] adds to `err`.

use crate::{Dd, PI, PIO2, PIO2_1, PIO2_2, PIO2_3, PIO2_4, quick_two_sum, split, two_prod, two_sum};

/// `hi + lo` within `err` of a true value (absolute, `err ≥ 0`).
#[derive(Clone, Copy, Debug)]
pub struct Approx {
    pub hi: f64,
    pub lo: f64,
    pub err: f64,
}

impl Approx {
    /// `hi + lo` within `rel · |hi| + abs` of the value (`|lo|` well below `|hi|`), `err`
    /// widened by the rounding of `lo ± err`.
    #[inline]
    fn new(hi: f64, lo: f64, rel: f64, abs: f64) -> Approx {
        let err = rel * hi.abs() + abs;
        Approx { hi, lo, err: err + (lo.abs() + err) * 2.220_446_049_250_313e-16 }
    }

    /// [`Approx::new`] with `hi` the rounding of the sum (a quotient's correction needs that).
    #[inline]
    fn normalized(hi: f64, lo: f64, rel: f64, abs: f64) -> Approx {
        let n = two_sum(hi, lo);
        Approx::new(n.hi, n.lo, rel, abs)
    }

    #[inline]
    fn neg(self) -> Approx {
        Approx { hi: -self.hi, lo: -self.lo, err: self.err }
    }

    /// The nearest float to the value when every value within `err` rounds to it; `None` when
    /// the bound straddles a rounding boundary (the caller takes the double-double path).
    #[inline]
    pub fn round(self) -> Option<f64> {
        let below = self.hi + (self.lo - self.err);
        let above = self.hi + (self.lo + self.err);
        if below == above { Some(below) } else { None }
    }
}

/// u = 2⁻⁵³, and the relative bounds of the sine, cosine and arc tangent paths.
const U: f64 = 1.110_223_024_625_156_5e-16;
const E_SIN: f64 = 1.084_202_172_485_504_4e-19; // 2⁻⁶³
const E_ATAN: f64 = 2.168_404_344_971_009e-19; // 2⁻⁶²
/// 2⁻¹⁰⁰: the second-order terms of a quotient's Dekker correction, relative to it.
const E_QUOTIENT: f64 = 7.888_609_052_210_118e-31;
/// 2⁻⁶⁵: the composition with π/2 or π, relative to the result.
const E_TURN: f64 = 2.710_505_431_213_761e-20;
/// 2⁻¹⁴⁷: what π/256 is beyond its four pieces, per unit of `q`.
const PIO256_REST: f64 = 5.605_193_857_299_268e-45;
/// 2⁻⁵⁰⁰ and 2⁵⁰⁰: the fast paths stay far from underflow, overflow and Dekker's split limit.
const TINY: f64 = 3.054_936_363_499_605e-151;
const HUGE: f64 = 3.273_390_607_896_142e150;

/// 256/π (to the float) and π/256 in the four pieces of π/2 scaled by 2⁻⁷ (exact).
const FRAC_256_PI: f64 = 81.487_330_863_050_42;
const P1: f64 = PIO2_1 / 128.0;
const P2: f64 = PIO2_2 / 128.0;
const P3: f64 = PIO2_3 / 128.0;
const P4: f64 = PIO2_4 / 128.0;

/// The series of `cos r − 1` (−1/2, 1/4!, −1/6!) and of `sin r / r − 1` (−1/3!, 1/5!, −1/7!).
const C1: f64 = -0.5;
const C2: f64 = 0.041_666_666_666_666_664;
const C3: f64 = -0.001_388_888_888_888_889;
const S1: f64 = -0.166_666_666_666_666_66;
const S2: f64 = 0.008_333_333_333_333_333;
const S3: f64 = -0.000_198_412_698_412_698_4;
/// The series of `atan v / v − 1` (−1/3, 1/5, −1/7, 1/9, −1/11).
const A1: f64 = -0.333_333_333_333_333_3;
const A2: f64 = 0.2;
const A3: f64 = -0.142_857_142_857_142_85;
const A4: f64 = 0.111_111_111_111_111_1;
const A5: f64 = -0.090_909_090_909_090_91;

/// sin(k·π/256), cos(k·π/256) for k = 0..=64 as (hi, lo) double-doubles (pmath_tables.py).
// The last entry is sin(π/4) = cos(π/4): its leading part is `FRAC_1_SQRT_2` to the bit, kept as
// the literal the script prints (`pmath_tests::fast_path_tables_match_the_double_double_series`).
#[allow(clippy::approx_constant)]
#[rustfmt::skip]
pub(crate) const SIN_COS_TABLE: [((f64, f64), (f64, f64)); 65] = [
    ((0.0, 0.0), (1.0, 0.0)),
    ((0.012271538285719925, 6.919790764028317e-19), (0.9999247018391445, 3.793108251266801e-17)),
    ((0.024541228522912288, -9.186849012577878e-20), (0.9996988186962042, -2.985148640379975e-17)),
    ((0.03680722294135883, 6.106008880352984e-19), (0.9993223845883495, -4.285853844084568e-17)),
    ((0.049067674327418015, -6.79610372051828e-19), (0.9987954562051724, -1.2291693337075465e-17)),
    ((0.06132073630220858, -5.118113406463811e-19), (0.9981181129001492, 2.793548755811383e-17)),
    ((0.07356456359966743, -2.7784941506273593e-18), (0.9972904566786902, 9.164769537110173e-18)),
    ((0.0857973123444399, -3.388189383068403e-18), (0.996312612182778, 1.1336497891624735e-17)),
    ((0.0980171403295606, -1.634582362244256e-18), (0.9951847266721969, -4.248691367830441e-17)),
    ((0.11022220729388306, -5.678950353782323e-19), (0.9939069700023561, -1.8964849471123746e-17)),
    ((0.1224106751992162, 2.8354501489965335e-18), (0.99247953459871, 3.1093055095428906e-17)),
    ((0.1345807085071262, -9.16703591714807e-18), (0.99090263542778, 1.5394565094566704e-17)),
    ((0.14673047445536175, 3.726947147046568e-18), (0.989176509964781, -4.098730993704711e-17)),
    ((0.15885814333386145, -4.016320057385908e-18), (0.9873014181578584, -5.233226125571565e-17)),
    ((0.17096188876030122, 9.19199801817591e-18), (0.9852776423889412, 2.3155637027900207e-17)),
    ((0.18303988795514095, 7.734991868863738e-18), (0.9831054874312163, 4.217000752288863e-17)),
    ((0.19509032201612828, -7.991079068461731e-18), (0.9807852804032304, 1.8546939997825006e-17)),
    ((0.20711137619221856, -1.0613362528971356e-17), (0.9783173707196277, -2.1623082233344895e-17)),
    ((0.2191012401568698, -3.6513812299150776e-19), (0.9757021300385286, -2.5572556081259686e-17)),
    ((0.2310581082806711, 1.0129787149761869e-17), (0.9729399522055602, -3.13112111222818e-17)),
    ((0.2429801799032639, -8.751431529719663e-18), (0.970031253194544, 1.8365300348428844e-17)),
    ((0.25486565960451457, -1.3602299806901461e-19), (0.9669764710448521, 3.8496228837337864e-17)),
    ((0.26671275747489837, 2.0941222578826688e-17), (0.9637760657954398, 2.646395056122003e-17)),
    ((0.2785196893850531, -1.0030273719543544e-17), (0.9604305194155658, 2.4653904815317185e-17)),
    ((0.2902846772544624, -1.892797870777425e-17), (0.9569403357322088, 4.05538698618757e-17)),
    ((0.3020059493192281, -1.7167666235262474e-17), (0.9533060403541939, -2.5190738779919934e-17)),
    ((0.31368174039889146, 1.4560447299968912e-17), (0.9495281805930367, -7.55441519280433e-18)),
    ((0.3253102921622629, 7.91712494637659e-18), (0.9456073253805213, 4.601910247852374e-17)),
    ((0.33688985339222005, -4.200094003347509e-19), (0.9415440651830208, -2.789637954769834e-17)),
    ((0.34841868024943456, 3.697442051420492e-18), (0.937339011912575, -3.6570926284362776e-17)),
    ((0.35989503653498817, -1.7601687123839282e-17), (0.9329927988347388, 4.2041415555384355e-17)),
    ((0.37131719395183754, 3.4749239648238266e-19), (0.9285060804732156, -2.3306639848485943e-17)),
    ((0.3826834323650898, -1.0050772696461588e-17), (0.9238795325112867, 1.7645047084336677e-17)),
    ((0.3939920400610481, 9.764924164123934e-18), (0.9191138516900578, -2.6496484622344718e-17)),
    ((0.40524131400498986, 9.911140194289988e-18), (0.9142097557035307, -3.631618252781442e-17)),
    ((0.4164295600976372, -2.5475580413131732e-17), (0.9091679830905224, -3.6878564091359894e-18)),
    ((0.4275550934302821, 9.411189816295473e-18), (0.9039892931234433, -6.609754468748431e-18)),
    ((0.43861623853852766, -2.088331583107509e-17), (0.8986744656939538, 2.6316906461033013e-17)),
    ((0.4496113296546066, 4.883192423203524e-18), (0.8932243011955153, -4.116123915190891e-18)),
    ((0.46053871095824, 1.8488777492177872e-17), (0.8876396204028539, 1.280509191858796e-17)),
    ((0.47139673682599764, 6.516678136069013e-18), (0.881921264348355, -1.9843248405890562e-17)),
    ((0.4821837720791228, -2.5861500925520442e-17), (0.8760700941954066, 5.872902423514768e-18)),
    ((0.49289819222978404, -1.0257831676562186e-18), (0.8700869911087115, -4.188851086854997e-17)),
    ((0.5035383837257176, -1.6731308204967497e-17), (0.8639728561215867, 4.148635595736161e-17)),
    ((0.5141027441932218, -4.5712707523615624e-17), (0.8577286100002721, -4.818344793633662e-17)),
    ((0.524589682678469, -4.3068869040082345e-17), (0.8513551931052652, -5.327987444601621e-17)),
    ((0.5349976198870973, -5.3683132708358134e-17), (0.8448535652497071, -4.363136029687964e-17)),
    ((0.5453249884220465, -4.151781753838426e-17), (0.8382247055548381, -3.5560085052855026e-17)),
    ((0.5555702330196022, 4.709410940561677e-17), (0.8314696123025452, 1.4073856984728024e-18)),
    ((0.5657318107836132, -3.4096079596590466e-17), (0.8245893027850253, -2.6512360488868275e-17)),
    ((0.5758081914178453, -3.7909495458942734e-17), (0.8175848131515837, -1.4883149812426772e-17)),
    ((0.5857978574564389, -3.748550196431129e-18), (0.8104571982525948, 2.35203673498405e-17)),
    ((0.5956993044924334, -1.3438641936579467e-17), (0.8032075314806449, -3.306060980481491e-17)),
    ((0.6055110414043255, -3.120267249330568e-17), (0.7958369046088836, -3.006272485191072e-17)),
    ((0.6152315905806268, 2.623141776726695e-17), (0.7883464276266062, 3.439699315405971e-17)),
    ((0.6248594881423863, 3.36718460372439e-17), (0.7807372285720945, -9.91987820666788e-18)),
    ((0.6343932841636455, 1.0420901929280035e-17), (0.773010453362737, -3.256590703364977e-17)),
    ((0.6438315428897915, -3.2084798795046886e-17), (0.765167265622459, -3.27072256125346e-17)),
    ((0.6531728429537768, 8.569564206002624e-18), (0.7572088465064846, -1.9909098777335502e-17)),
    ((0.6624157775901718, -2.261550888576459e-17), (0.7491363945234594, -4.472907844701189e-17)),
    ((0.6715589548470184, -4.048903774929669e-17), (0.7409511253549591, -1.4708616952297345e-17)),
    ((0.680600997795453, 2.8473293354522047e-17), (0.7326542716724128, 1.891867348157352e-17)),
    ((0.6895405447370669, -1.588932329480679e-17), (0.7242470829514669, 2.9198471334403004e-17)),
    ((0.6983762494089728, 4.898828243566777e-17), (0.7157308252838187, -5.158101847641026e-17)),
    ((0.7071067811865476, -4.833646656726457e-17), (0.7071067811865476, -4.833646656726457e-17)),
];
/// atan(k/32) for k = 0..=32 as (hi, lo) double-doubles (pmath_tables.py).
// The last entry is atan 1 = π/4: its leading part is `FRAC_PI_4` to the bit, kept as the
// literal the script prints (`pmath_tests::fast_path_tables_match_the_double_double_series`).
#[allow(clippy::approx_constant)]
#[rustfmt::skip]
pub(crate) const ATAN_TABLE: [(f64, f64); 33] = [
    (0.0, 0.0),
    (0.031239833430268277, -1.188442711587748e-18),
    (0.06241880999595735, -1.5490756308295046e-18),
    (0.09347678115858947, -6.2844725995420954e-18),
    (0.12435499454676144, -3.1253241424539383e-18),
    (0.15499674192394097, 9.585415594114324e-18),
    (0.18534794999569476, 4.180692268843079e-18),
    (0.21535769969773805, 4.738160130078733e-19),
    (0.24497866312686414, 1.0698755618734451e-17),
    (0.2741674511196588, 8.261353575163773e-18),
    (0.3028848683749714, -1.1010827903001369e-17),
    (0.3310960767041321, -7.952610375793799e-18),
    (0.35877067027057225, -2.4623815582638635e-17),
    (0.38588266939807375, 2.378822732491941e-17),
    (0.4124104415973873, -1.587652227770689e-17),
    (0.43833655985795783, -2.494277030626541e-17),
    (0.4636476090008061, 2.2698777452961687e-17),
    (0.48833395105640554, -1.1373236189329585e-17),
    (0.5123894603107377, -2.5462781472855804e-17),
    (0.5358112379604637, -4.0637956834825575e-18),
    (0.5585993153435624, -5.4556305485916264e-18),
    (0.5807563535676704, -1.441464378193067e-17),
    (0.6022873461349642, 2.950430737228402e-17),
    (0.6231993299340659, 2.672403885140095e-17),
    (0.6435011087932844, 1.5834785051444286e-17),
    (0.6632029927060933, -3.076054864429649e-17),
    (0.6823165548747481, 6.943223671560008e-18),
    (0.7008544078844502, -1.987626234335816e-17),
    (0.7188299996216245, -2.1478388444456983e-17),
    (0.7362574289814281, 3.473937648299457e-17),
    (0.7531512809621944, -2.4256934659182068e-17),
    (0.7695264804056583, -3.704991905602721e-17),
    (0.7853981633974483, 3.061616997868383e-17),
];

/// `x` reduced about `q·π/256`: `rh + rl` and the bound of its error.
#[inline]
fn reduce(x: f64, q: f64) -> (f64, f64, f64) {
    if q == 0.0 {
        return (x, 0.0, 0.0);
    }
    let a = x - q * P1; // exact: q·P1 is, and it is within a factor of two of x
    let b = q * P2; // exact
    let s = two_sum(a, -b);
    let c = q * P3; // exact
    let d = q * P4; // exact
    let t = s.lo - c;
    let rl = t - d;
    let err = (t.abs() + rl.abs()) * (2.0 * U) + q.abs() * PIO256_REST;
    (s.hi, rl, err)
}

/// `a · b` exactly as `hi + lo` (Dekker's product) with `a` already split into 26-bit halves.
#[inline]
fn two_prod_split(a: f64, (ah, al): (f64, f64), b: f64) -> Dd {
    let p = a * b;
    let (bh, bl) = split(b);
    Dd { hi: p, lo: ((ah * bh - p) + ah * bl + al * bh) + al * bl }
}

/// `x = quadrant·π/2 + a + r` with the table's sine and cosine of `a`, the reduced `r` and the
/// series of its sine and cosine (see the module documentation): what the sine and the cosine
/// share.
struct Parts {
    sh: f64,
    sl: f64,
    ch: f64,
    cl: f64,
    rh: f64,
    rl: f64,
    /// `rh` split for Dekker's product.
    rs: (f64, f64),
    /// `cos rh − 1` and `sin rh − rh`.
    pc: f64,
    sr: f64,
    /// The bound of the reduction's error, with the neglected part of the `rl` terms.
    er: f64,
    quadrant: i64,
}

/// The parts of a finite `x` with `2⁻⁵⁰⁰ ≤ |x| ≤ 20000`; `None` outside that range.
#[inline]
fn parts(x: f64) -> Option<Parts> {
    let ax = x.abs();
    if !(ax >= TINY && ax <= 20000.0) {
        return None;
    }
    let q = (x * FRAC_256_PI).round();
    let (rh, rl, er) = reduce(x, q);
    let qi = q as i64;
    let quadrant = (qi + 64).div_euclid(128);
    let k = qi - 128 * quadrant; // −64..=63
    let (s, (ch, cl)) = SIN_COS_TABLE[k.unsigned_abs() as usize];
    let (sh, sl) = if k < 0 { (-s.0, -s.1) } else { s };
    let z = rh * rh;
    let pc = z * (C1 + z * (C2 + z * C3));
    let sr = rh * (z * (S1 + z * (S2 + z * S3)));
    // The `rl` terms take the first-order sine and cosine of `a + rh`; what they neglect is
    // below (2·|pc| + 4u)·|rl|.
    let er = er + rl.abs() * (2.0 * pc.abs() + 4.0 * U);
    Some(Parts { sh, sl, ch, cl, rh, rl, rs: split(rh), pc, sr, er, quadrant })
}

/// `sin(a + r)`.
#[inline]
fn sine(p: &Parts) -> Approx {
    let d = two_prod_split(p.rh, p.rs, p.ch);
    let h = quick_two_sum(p.sh, d.hi); // |sh| ≥ |d.hi| (or sh = 0): exact
    let lo = ((h.lo + d.lo) + (p.sl + p.sh * p.pc)) + ((p.ch * p.sr + p.rl * (p.ch - p.sh * p.rh)) + p.cl * p.rh);
    Approx::new(h.hi, lo, E_SIN, p.er)
}

/// `cos(a + r)`.
#[inline]
fn cosine(p: &Parts) -> Approx {
    let d = two_prod_split(p.rh, p.rs, p.sh);
    let h = quick_two_sum(p.ch, -d.hi); // ch ≥ 0.7 > |d.hi|: exact
    let lo = ((h.lo - d.lo) + (p.cl + p.ch * p.pc)) - ((p.sh * p.sr + p.rl * (p.sh + p.ch * p.rh)) + p.sl * p.rh);
    Approx::new(h.hi, lo, E_SIN, p.er)
}

/// The sine of a finite `x` with `2⁻⁵⁰⁰ ≤ |x| ≤ 20000` (see the module documentation);
/// `None` outside that range.
#[inline]
pub fn sin(x: f64) -> Option<Approx> {
    let p = parts(x)?;
    Some(match p.quadrant.rem_euclid(4) {
        1 => cosine(&p),
        2 => sine(&p).neg(),
        3 => cosine(&p).neg(),
        _ => sine(&p),
    })
}

/// The cosine of a finite `x` with `2⁻⁵⁰⁰ ≤ |x| ≤ 20000`; `None` outside that range.
#[inline]
pub fn cos(x: f64) -> Option<Approx> {
    let p = parts(x)?;
    Some(match p.quadrant.rem_euclid(4) {
        1 => sine(&p).neg(),
        2 => cosine(&p).neg(),
        3 => sine(&p),
        _ => cosine(&p),
    })
}

/// The sine and cosine of a finite `x` with `2⁻⁵⁰⁰ ≤ |x| ≤ 20000`; `None` outside that range.
#[inline]
pub fn sin_cos(x: f64) -> Option<(Approx, Approx)> {
    let p = parts(x)?;
    let (sin, cos) = (sine(&p), cosine(&p));
    Some(match p.quadrant.rem_euclid(4) {
        1 => (cos, sin.neg()),
        2 => (sin.neg(), cos.neg()),
        3 => (cos.neg(), sin),
        _ => (sin, cos),
    })
}

/// The tangent of `x` where [`sin_cos`] answers.
#[inline]
pub fn tan(x: f64) -> Option<Approx> {
    let (s, c) = sin_cos(x)?;
    let sn = two_sum(s.hi, s.lo);
    let cn = two_sum(c.hi, c.lo);
    let (q, ql) = quotient(sn.hi, sn.lo, cn.hi, cn.lo);
    let err = (s.err + q.abs() * c.err) / cn.hi.abs() * (1.0 + 4.0 * U);
    Some(Approx::normalized(q, ql, E_QUOTIENT, err))
}

/// `(nh + nl) / (dh + dl)` as a quotient and its Dekker correction, within 2⁻¹⁰⁰ of the
/// quotient of the sums for `|dl| ≤ u·|dh|` and `|nl| ≤ u·|nh|` or `|nl| ≤ u·|dh|` (the
/// neglected second-order terms are `(dl/dh)²` relative and `nl·dl/dh²` absolute), `nh` and
/// `dh` far from underflow and overflow.
#[inline]
fn quotient(nh: f64, nl: f64, dh: f64, dl: f64) -> (f64, f64) {
    let q = nh / dh;
    let p = two_prod(q, dh);
    let rem = ((nh - p.hi) - p.lo) + (nl - q * dl); // nh − p.hi is exact (Sterbenz)
    (q, rem / dh)
}

/// atan t for `t = th + tl` in `[0, 1]` (a little more does no harm) with `th ≥ 2⁻⁵⁰⁰`.
#[inline]
fn atan_unit(th: f64, tl: f64) -> Approx {
    let k = (th * 32.0).round();
    let (vh, vl, ah, al) = if k == 0.0 {
        (th, tl, 0.0, 0.0)
    } else {
        let c = k / 32.0; // exact
        let nh = th - c; // exact: c is within a factor of two of th
        let p = two_prod(th, c);
        let d = two_sum(1.0, p.hi);
        let dl = (d.lo + p.lo) + tl * c;
        let (vh, vl) = quotient(nh, tl, d.hi, dl);
        let (ah, al) = ATAN_TABLE[k as usize];
        (vh, vl, ah, al)
    };
    let z = vh * vh;
    let pa = z * (A1 + z * (A2 + z * (A3 + z * (A4 + z * A5)))); // atan vh / vh − 1
    let h = two_sum(ah, vh);
    let lo = ((h.lo + al) + (vl - vl * z)) + vh * pa;
    Approx::new(h.hi, lo, E_ATAN, 0.0)
}

/// `turn − a` or `turn + a` for a double-double `turn` (π/2 or π) and a non-negative `a` below
/// π/4, so that the sum does not cancel.
#[inline]
fn turned(turn: Dd, a: Approx, subtract: bool) -> Approx {
    let (ah, al) = if subtract { (-a.hi, -a.lo) } else { (a.hi, a.lo) };
    let h = two_sum(turn.hi, ah);
    let lo = (h.lo + turn.lo) + al;
    Approx::new(h.hi, lo, E_TURN, a.err)
}

/// The angle of `(x, y)` in `(−π, π]` for double-doubles `y` and `x` whose leading parts are
/// finite, non-zero and within `[2⁻⁵⁰⁰, 2⁵⁰⁰]` in magnitude; `None` otherwise.
#[inline]
pub fn atan2(y: Dd, x: Dd) -> Option<Approx> {
    let (ay, ax) = (y.hi.abs(), x.hi.abs());
    if !(ay >= TINY && ay <= HUGE && ax >= TINY && ax <= HUGE) {
        return None;
    }
    let y_abs = if y.hi < 0.0 { y.neg() } else { y };
    let x_abs = if x.hi < 0.0 { x.neg() } else { x };
    let (n, d, swapped) = if ay <= ax { (y_abs, x_abs, false) } else { (x_abs, y_abs, true) };
    let (th, tl) = quotient(n.hi, n.lo, d.hi, d.lo);
    if th < TINY {
        return None;
    }
    let a = atan_unit(th, tl);
    let a = match (swapped, x.hi < 0.0) {
        (false, false) => a,
        (false, true) => turned(PI, a, true),
        (true, false) => turned(PIO2, a, true),
        (true, true) => turned(PIO2, a, false),
    };
    Some(if y.hi < 0.0 { a.neg() } else { a })
}

/// `√(1 − x²)` for `|x| < 1` as a double-double within 2⁻¹⁰³ (relative): the difference is a
/// two-sum of the exact square, normalized (so that the root's correction is first order), the
/// root one Dekker correction from the float square root.
#[inline]
pub fn cos_of_asin(x: f64) -> Dd {
    let p = two_prod(x, x);
    let w = two_sum(1.0, -p.hi);
    let w = quick_two_sum(w.hi, w.lo - p.lo); // |p.lo| ≤ 2⁻⁵³ ≤ |w.hi| / 2
    let s = w.hi.sqrt();
    let ps = two_prod(s, s);
    let d = ((w.hi - ps.hi) - ps.lo) + w.lo; // w.hi − ps.hi is exact (Sterbenz)
    Dd { hi: s, lo: d / (2.0 * s) }
}
