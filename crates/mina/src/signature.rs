use mina_hasher::{Hashable, ROInput};
use mina_signer::{BaseField, Keypair, NetworkId, PubKey, ScalarField, Signature, Signer};
use o1_utils::field_helpers::FieldHelpers;
use sha2::Digest;

#[derive(Clone)]
struct FieldsInput {
    fields: Vec<BaseField>,
}

impl Hashable for FieldsInput {
    type D = NetworkId;

    fn to_roinput(&self) -> ROInput {
        let mut roi = ROInput::new();
        for field in &self.fields {
            roi = roi.append_field(*field);
        }
        roi
    }

    fn domain_string(network_id: NetworkId) -> Option<String> {
        match network_id {
            NetworkId::MAINNET => "MinaSignatureMainnet",
            NetworkId::TESTNET => "CodaSignature*******",
        }
        .to_string()
        .into()
    }
}

/// Create a signature over an array of BaseField elements using the kimchi signer.
///
/// - Signs with kimchi domain and OCaml/TS-compatible nonce (packed = true)
/// - Uses TESTNET domain by default
pub fn create_signature(kp: &Keypair, fields: &[BaseField]) -> mina_signer::Signature {
    let input = FieldsInput {
        fields: fields.to_vec(),
    };

    let mut ctx = mina_signer::create_kimchi::<FieldsInput>(NetworkId::TESTNET);
    let sig = ctx.sign(kp, &input, true);

    sig
}

/// Convert a signature to a base58 string.
/// Format: version(2 bytes) || rx(32 bytes, big-endian) || s(32 bytes, big-endian) || checksum(4 bytes)
pub fn signature_to_base58(sig: &mina_signer::Signature) -> String {
    // Payload: versionNumber (1) || r (LE 32) || s (LE 32)
    let r_bytes = sig.rx.to_bytes();
    let s_bytes = sig.s.to_bytes();

    let mut payload = Vec::with_capacity(1 + r_bytes.len() + s_bytes.len());
    payload.push(1u8); // versionNumbers.signature = 1
    payload.extend_from_slice(&r_bytes);
    payload.extend_from_slice(&s_bytes);

    // Base58Check: prepend versionByte (versionBytes.signature = 154), double sha256 checksum
    let mut with_version = Vec::with_capacity(1 + payload.len() + 4);
    with_version.push(154u8);
    with_version.extend_from_slice(&payload);
    let checksum = sha2::Sha256::digest(&sha2::Sha256::digest(&with_version[..])[..]);
    with_version.extend_from_slice(&checksum[..4]);
    bs58::encode(with_version).into_string()
}

/// Decode a base58-encoded Mina signature (current format) into a `Signature`.
/// Expects Base58Check with version byte 154 and inner version number 1.
pub fn signature_from_base58(signature_b58: &str) -> Option<Signature> {
    let bytes = bs58::decode(signature_b58).into_vec().ok()?;
    if bytes.len() < 1 + 1 + 32 + 32 + 4 {
        return None;
    }
    let (original, checksum) = (&bytes[..bytes.len() - 4], &bytes[bytes.len() - 4..]);
    let expected = sha2::Sha256::digest(&sha2::Sha256::digest(original)[..]);
    if &expected[..4] != checksum {
        return None;
    }
    if original[0] != 154u8 {
        return None;
    }
    let payload = &original[1..];
    if payload.len() != 1 + 32 + 32 || payload[0] != 1u8 {
        return None;
    }
    let r_bytes = &payload[1..33];
    let s_bytes = &payload[33..65];
    let rx = BaseField::from_bytes(r_bytes).ok()?;
    let s = ScalarField::from_bytes(s_bytes).ok()?;
    Some(Signature::new(rx, s))
}

/// A simple representation of signature components as decimal strings.
pub struct SignatureStrings {
    pub r: String,
    pub s: String,
}

/// Convert a signature to decimal strings for r (rx) and s.
pub fn signature_to_strings(sig: &mina_signer::Signature) -> SignatureStrings {
    SignatureStrings {
        r: sig.rx.to_string(),
        s: sig.s.to_string(),
    }
}

/// Verify a base58 signature for an arbitrary array of fields on TESTNET.
/// Uses kimchi signer and packed=true nonce derivation.
pub fn verify_signature(signature_b58: &str, address: &str, fields: &[BaseField]) -> bool {
    let Some(sig) = signature_from_base58(signature_b58) else {
        return false;
    };

    let public = match PubKey::from_address(address) {
        Ok(pk) => pk,
        Err(_) => return false,
    };

    let input = FieldsInput {
        fields: fields.to_vec(),
    };
    let mut ctx = mina_signer::create_kimchi::<FieldsInput>(NetworkId::TESTNET);
    ctx.verify(&sig, &public, &input)
}