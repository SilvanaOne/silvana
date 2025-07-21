module commitment::exp;

use constants::constants::{
    get_r,
    get_table0_entry,
    get_table1_entry,
    get_table2_entry
};
use sui::bls12381::{Scalar, scalar_mul, scalar_from_u64};
use sui::group_ops::Element;

const MAX_EXP: u32 = 1024 * 1024 * 1024;

/// Optimized exponentiation using three 1024-element lookup tables
/// Computes R^exp using base-1024 decomposition:
///   exp = i0 + 1024*i1 + 1024^2*i2
///   R^exp = TABLE2[i2] * TABLE1[i1] * TABLE0[i0]
///
/// Time complexity: O(1) - constant time with 3 table lookups + 2 multiplications
/// Space complexity: 3 * 1024 * 32 bytes = 96 KiB for all tables
/// Range: supports exponents up to 1024^3 - 1 = 1,073,741,823
public fun r_scalar_pow(exp: u32): Element<Scalar> {
    assert!(exp < MAX_EXP, 0);
    // Decompose exponent in base-1024 (10 bits per component)
    let i0 = (exp & 0x3FF); // exp mod 1024 (lowest 10 bits)
    let i1 = ((exp >> 10) & 0x3FF); // next 10 bits
    let i2 = ((exp >> 20) & 0x3FF); // highest 10 bits

    // Constant-time table lookups using getter functions
    let t0 = get_table0_entry(i0);
    let t1 = get_table1_entry(i1);
    let t2 = get_table2_entry(i2);

    // Combine results with 2 field multiplications
    let mut result = scalar_mul(&t2, &t1); // R^(1024^2*i2 + 1024*i1)
    result = scalar_mul(&result, &t0); // + i0
    result
}

/// Legacy function (inefficient, use r_scalar_pow instead)
public fun r_scalar_pow_legacy(exp: u32): Element<Scalar> {
    let mut acc = scalar_from_u64(1); // Start with 1
    let base = get_r();
    let mut i = 0;
    while (i < exp) {
        acc = scalar_mul(&acc, &base);
        i = i + 1;
    };
    acc
}
