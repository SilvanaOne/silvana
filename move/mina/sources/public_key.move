module mina::public_key;

public struct MinaSignature has copy, drop, store {
    r: u256,
    s: u256,
}

public(package) fun create_mina_signature(r: u256, s: u256): MinaSignature {
    MinaSignature { r, s }
}

public(package) fun to_bytes(signature: &MinaSignature): vector<u8> {
    let mut bytes = vector::empty<u8>();
    let r_bytes = std::bcs::to_bytes(&signature.r);
    let s_bytes = std::bcs::to_bytes(&signature.s);
    vector::append(&mut bytes, r_bytes);
    vector::append(&mut bytes, s_bytes);
    bytes
}

const MAX_BIT: u256 = 1u256 << 255;

public fun convert_mina_public_key(public_key: u256): (u256, bool) {
    let is_odd = (public_key & MAX_BIT) != 0;
    let x = public_key - (if (is_odd) { MAX_BIT } else { 0 });
    (x, is_odd)
}
