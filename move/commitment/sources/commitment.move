module commitment::commitment;

use commitment::exp::r_scalar_pow;
use sui::bls12381::{
    Scalar,
    scalar_add,
    scalar_sub,
    scalar_mul,
    scalar_from_bytes,
    scalar_zero
};
use sui::group_ops::Element;

/// should be random, less than 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001
fun get_s(): Element<Scalar> {
    let s_bytes =
        x"1582695da6689f26db7bb3eb32907ecd0ac3af032aefad31a069352705f0d459";
    scalar_from_bytes(&s_bytes)
}

fun get_r(): Element<Scalar> {
    let r_bytes =
        x"149fa8c209ab655fd480a3aff7d16dc72b6a3943e4b95fcf7909f42d9c17a552";
    scalar_from_bytes(&r_bytes)
}

/// inner: digest one struct
public fun digest_struct(fields: vector<Element<Scalar>>): Element<Scalar> {
    let mut d = scalar_zero();
    let mut i = 0;
    let s = get_s();
    while (i < vector::length(&fields)) {
        d = scalar_add(&scalar_mul(&d, &s), vector::borrow(&fields, i));
        i = i + 1;
    };
    d
}

/// outer: commit whole table
public fun commit(table: vector<Element<Scalar>>): Element<Scalar> {
    // table[i] already holds c_i = digest_struct(...)
    let mut acc = scalar_zero();
    let r = get_r();
    let len = vector::length(&table);
    let mut i = len;

    // Iterate in reverse order so that table[i] gets coefficient r^i
    while (i > 0) {
        i = i - 1;
        acc = scalar_add(&scalar_mul(&acc, &r), vector::borrow(&table, i));
    };
    acc
}

/// constant-time single-field update using struct digest recalculation
public fun update(
    old_table_commitment: &Element<Scalar>,
    old_struct_digest: &Element<Scalar>, // old struct digest at position i
    new_struct_digest: &Element<Scalar>, // new struct digest at position i
    index: u32, // index in table (0-based)
): Element<Scalar> {
    // The table commitment formula in commit() now produces:
    // table[0]*r^0 + table[1]*r^1 + table[2]*r^2 + ... + table[i]*r^i
    // So position i has coefficient r^i

    // Position i has coefficient r^i
    let r_pow_i = r_scalar_pow(index);

    let struct_delta = scalar_sub(new_struct_digest, old_struct_digest);
    let table_delta = scalar_mul(&struct_delta, &r_pow_i);
    scalar_add(old_table_commitment, &table_delta)
}

/// Mina (Pallas) field modulus  p = 0x4000...0001
const MINA_PRIME: u256 =
    0x40000000000000000000000000000000224698FC094CF91B992D30ED00000001;

/// Convert a u256 into a BLS scalar *iff* it is < MINA_PRIME
public fun scalar_from_u256(n: u256): Element<Scalar> {
    // abort 1 if out of range
    assert!(n < MINA_PRIME, 1);

    // 32‑byte big‑endian buffer
    let mut bytes =
        x"0000000000000000000000000000000000000000000000000000000000000000";

    let mut tmp = n;
    let mut i: u8 = 31;
    loop {
        // write least‑significant byte of tmp into bytes[i]
        *vector::borrow_mut(&mut bytes, i as u64) = (tmp & 0xff) as u8;
        if (i == 0) break;
        tmp = tmp >> 8;
        i = i - 1;
    };

    scalar_from_bytes(&bytes)
}
