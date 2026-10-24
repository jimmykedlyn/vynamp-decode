use super::*;
use crate::common::Rng;

const C: [f64; 8] = [
    -0.6, -0.535, -0.33, -0.185, -0.095, -0.041, -0.0142, -0.0037,
];

/// The butterflies of the standard, literally, for boundaries `1..=last`.
pub(in super::super) fn reference(x: &[f32; 576], last: usize) -> Vec<f64> {
    let mut x: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    for sb in 1..=last {
        for (i, c) in C.iter().enumerate() {
            let (cs, ca) = (1.0 / (1.0 + c * c).sqrt(), c / (1.0 + c * c).sqrt());
            let (a, b) = (x[18 * sb - 1 - i], x[18 * sb + i]);
            x[18 * sb - 1 - i] = a * cs - b * ca;
            x[18 * sb + i] = b * cs + a * ca;
        }
    }
    x
}

fn random(rng: &mut Rng, n: usize) -> [f32; 576] {
    let mut a = [0.0f32; 576];
    for v in a.iter_mut().take(n) {
        *v = rng.unit() as f32;
    }
    a
}

fn assert_close(got: &[f32; 576], want: &[f64], what: &str) {
    for i in 0..576 {
        assert!(
            (f64::from(got[i]) - want[i]).abs() < 1e-6,
            "{what}: line {i}: {} vs {}",
            got[i],
            want[i]
        );
    }
}

#[test]
fn all_thirty_one_boundaries_follow_the_definition() {
    let mut rng = Rng(41);
    let x0 = random(&mut rng, 576);
    let mut x = x0;
    reduce(&mut x, 576, false);
    assert_close(&x, &reference(&x0, 31), "full");
    // The first and last lines are not part of any butterfly.
    assert_eq!((x[0], x[575]), (x0[0], x0[575]));
}

#[test]
fn only_the_first_boundary_of_a_mixed_block() {
    let mut rng = Rng(42);
    let x0 = random(&mut rng, 576);
    let mut x = x0;
    reduce(&mut x, 576, true);
    assert_close(&x, &reference(&x0, 1), "mixed");
    assert_eq!(x[36..], x0[36..]);
}

#[test]
fn the_zero_tail_is_skipped_without_changing_the_result() {
    let mut rng = Rng(43);
    for used in [0usize, 1, 10, 18, 25, 26, 27, 100, 431, 432, 570, 576] {
        let x0 = random(&mut rng, used);
        let mut x = x0;
        reduce(&mut x, used, false);
        assert_close(&x, &reference(&x0, 31), &format!("used {used}"));
    }
}

#[test]
fn the_butterflies_preserve_energy() {
    let mut rng = Rng(44);
    let x0 = random(&mut rng, 576);
    let mut x = x0;
    reduce(&mut x, 576, false);
    let e = |a: &[f32; 576]| a.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>();
    assert!((e(&x) - e(&x0)).abs() < 1e-3 * e(&x0));
}
