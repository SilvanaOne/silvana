use anyhow::Result;
use std::env;
use std::str::FromStr;
use sui_rpc::proto::sui::rpc::v2beta2 as proto;
use sui_rpc::Client as GrpcClient;
use sui_sdk_types as sui;
use tracing::{debug};
use prost_types;

/// Derive Sui address from a 32-byte private key using the exact reference algorithm
pub fn derive_address_from_secret_key(secret_key_bytes: &[u8; 32]) -> sui::Address {
    // Use ed25519_dalek to get public key bytes
    let signing_key = ed25519_dalek::SigningKey::from_bytes(secret_key_bytes);
    let verifying_key = signing_key.verifying_key();
    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(verifying_key.as_bytes());
    
    // Use SDK's Ed25519PublicKey to derive address - exact reference algorithm
    let sui_public_key = sui::Ed25519PublicKey::new(pk_bytes);
    sui_public_key.derive_address()
}

/// Load sender address and private key from environment variables
pub fn load_sender_from_env() -> Result<(sui::Address, sui_crypto::ed25519::Ed25519PrivateKey)> {
    use base64ct::Encoding;
    let addr = sui::Address::from_str(&env::var("SUI_ADDRESS")?)?;
    let raw = env::var("SUI_SECRET_KEY")?;
    let key_part = raw.split_once(':').map(|(_, b)| b.to_string()).unwrap_or(raw);
    
    // Try bech32 "suiprivkey" first
    if key_part.starts_with("suiprivkey") {
        debug!("Decoding SUI_SECRET_KEY as bech32 suiprivkey");
        let (hrp, data, _variant) = bech32::decode(&key_part)?;
        if hrp != "suiprivkey" {
            return Err(anyhow::anyhow!("invalid bech32 hrp"));
        }
        let bytes: Vec<u8> = bech32::FromBase32::from_base32(&data)?;
        if bytes.len() != 33 {
            return Err(anyhow::anyhow!("bech32 payload must be 33 bytes (flag || key)"));
        }
        if bytes[0] != 0x00 {
            return Err(anyhow::anyhow!("unsupported key scheme flag; only ed25519 supported"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes[1..]);
        
        // Verify the derived address matches
        let derived_address = derive_address_from_secret_key(&arr);
        if addr != derived_address {
            return Err(anyhow::anyhow!("Address mismatch: environment address does not match derived address"));
        }
        
        let sk = sui_crypto::ed25519::Ed25519PrivateKey::new(arr);
        return Ok((addr, sk));
    }
    
    // Else try base64 then hex
    let mut bytes = match base64ct::Base64::decode_vec(&key_part) {
        Ok(v) => v,
        Err(_) => {
            debug!("SUI_SECRET_KEY not base64; trying hex");
            if let Some(hex_str) = key_part.strip_prefix("0x") {
                hex::decode(hex_str)?
            } else {
                hex::decode(&key_part)?
            }
        }
    };
    
    if !bytes.is_empty() && (bytes[0] == 0x00 || bytes.len() == 33) {
        bytes = bytes[1..].to_vec();
    }
    
    if bytes.len() < 32 {
        return Err(anyhow::anyhow!("SUI_SECRET_KEY must contain at least 32 bytes"));
    }
    
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes[..32]);
    let sk = sui_crypto::ed25519::Ed25519PrivateKey::new(arr);
    Ok((addr, sk))
}


/// Get reference gas price from the network
pub async fn get_reference_gas_price(client: &mut GrpcClient) -> Result<u64> {
    let mut ledger = client.ledger_client();
    let _resp = ledger
        .get_service_info(proto::GetServiceInfoRequest {})
        .await?
        .into_inner();
    // ServiceInfo does not expose gas price yet; default to 1000
    let price = 1_000u64;
    debug!("Using reference gas price: {}", price);
    Ok(price)
}


/// Pick a gas object owned by the sender
pub async fn pick_gas_object(client: &mut GrpcClient, sender: sui::Address) -> Result<sui::ObjectReference> {
    let mut live = client.live_data_client();
    debug!("Listing owned objects for sender: {}", sender);
    
    let resp = live
        .list_owned_objects(proto::ListOwnedObjectsRequest {
            owner: Some(sender.to_string()),
            page_size: Some(100),
            page_token: None,
            read_mask: Some(prost_types::FieldMask { 
                paths: vec![
                    "object_id".into(), 
                    "version".into(), 
                    "digest".into(), 
                    "object_type".into(),
                ]
            }),
            object_type: None,
        })
        .await?
        .into_inner();
    
    debug!("Owned objects returned: {}", resp.objects.len());
    
    let mut obj = resp
        .objects
        .into_iter()
        .find(|o| o.object_type.as_ref().map(|t| t.contains("::sui::SUI")).unwrap_or(true))
        .ok_or_else(|| anyhow::anyhow!("no owned objects to use as gas"))?;
    
    debug!(object = ?obj, "Selected object");
    
    if obj.digest.is_none() || obj.version.is_none() {
        debug!("Digest/version missing; fetching object details");
        let mut ledger = client.ledger_client();
        let object_id_str = obj.object_id.clone()
            .ok_or_else(|| anyhow::anyhow!("missing object id"))?;
        let got = ledger
            .get_object(proto::GetObjectRequest {
                object_id: Some(object_id_str.clone()),
                version: None,
                read_mask: None,
            })
            .await?
            .into_inner();
        
        if let Some(full) = got.object {
            debug!(object = ?full, "GetObject response");
            obj.object_id = full.object_id;
            obj.version = full.version;
            obj.digest = full.digest;
        }
    }
    
    let id = obj.object_id.as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing object id"))?
        .parse()?;
    let version = obj.version
        .ok_or_else(|| anyhow::anyhow!("missing version"))?;
    let digest = obj.digest.as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing digest"))?
        .parse()?;
    
    debug!("Gas coin chosen: id={}, version={}, digest={}", id, version, digest);
    Ok(sui::ObjectReference::new(id, version, digest))
}