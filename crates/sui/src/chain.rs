use anyhow::{Result, anyhow};
use prost_types;
use std::env;
use std::str::FromStr;
use sui_rpc::client::v2::Client as GrpcClient;
use sui_rpc::proto::sui::rpc::v2 as proto;
use sui_sdk_types as sui;
use tracing::debug;

/// Resolve the RPC URL based on the following priority:
/// 1. If rpc_url is provided explicitly, use it
/// 2. If SUI_RPC_URL env var is set, use it (allows custom endpoints)
/// 3. Otherwise, determine chain and use chain-specific URL:
///    - Use provided chain parameter if Some
///    - Otherwise check SUI_CHAIN env var
///    - Default to "devnet"
///    - Check SUI_RPC_URL_<CHAIN> env var
///    - Fall back to default https://fullnode.<chain>.sui.io:443
pub fn resolve_rpc_url(rpc_url: Option<String>, chain_override: Option<String>) -> Result<String> {
    // 1. Use explicit rpc_url if provided
    if let Some(url) = rpc_url {
        return Ok(url);
    }

    // 2. Check for custom SUI_RPC_URL
    if let Ok(custom_url) = env::var("SUI_RPC_URL") {
        return Ok(custom_url);
    }

    // 3. Determine chain
    let chain = if let Some(chain) = chain_override {
        // Use the provided chain override
        chain.to_lowercase()
    } else {
        // Fall back to SUI_CHAIN env var or default to devnet
        env::var("SUI_CHAIN")
            .unwrap_or_else(|_| "devnet".to_string())
            .to_lowercase()
    };

    // Validate chain
    match chain.as_str() {
        "devnet" | "testnet" | "mainnet" => {
            // Valid chain, continue
        }
        _ => {
            return Err(anyhow!(
                "Invalid chain '{}'. Must be one of: devnet, testnet, mainnet",
                chain
            ));
        }
    }

    // 4. Try chain-specific env var first
    let chain_specific_var = format!("SUI_RPC_URL_{}", chain.to_uppercase());
    if let Ok(chain_url) = env::var(&chain_specific_var) {
        return Ok(chain_url);
    }

    // 5. Fall back to default public endpoints
    let url = match chain.as_str() {
        "devnet" => "https://fullnode.devnet.sui.io:443".to_string(),
        "testnet" => "https://fullnode.testnet.sui.io:443".to_string(),
        "mainnet" => "https://fullnode.mainnet.sui.io:443".to_string(),
        _ => unreachable!(), // We already validated the chain above
    };

    Ok(url)
}

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

/// Load sender address and private key from environment variables or provided key
pub fn load_sender_from_env() -> Result<(sui::Address, sui_crypto::ed25519::Ed25519PrivateKey)> {
    load_sender_from_env_or_key(None)
}

/// Load sender address and private key from environment variables or provided key
pub fn load_sender_from_env_or_key(
    private_key_opt: Option<String>,
) -> Result<(sui::Address, sui_crypto::ed25519::Ed25519PrivateKey)> {
    use base64ct::Encoding;

    // Track if we're using a provided key
    let using_provided_key = private_key_opt.is_some();

    // Get the private key string from parameter or environment
    let key_part = if let Some(key) = private_key_opt {
        key
    } else {
        let raw = env::var("SUI_SECRET_KEY")?;
        raw.split_once(':')
            .map(|(_, b)| b.to_string())
            .unwrap_or(raw)
    };

    // Try bech32 "suiprivkey" first
    if key_part.starts_with("suiprivkey") {
        debug!("Decoding SUI_SECRET_KEY as bech32 suiprivkey");
        let (hrp, data, _variant) = bech32::decode(&key_part)?;
        if hrp != "suiprivkey" {
            return Err(anyhow::anyhow!("invalid bech32 hrp"));
        }
        let bytes: Vec<u8> = bech32::FromBase32::from_base32(&data)?;
        if bytes.len() != 33 {
            return Err(anyhow::anyhow!(
                "bech32 payload must be 33 bytes (flag || key)"
            ));
        }
        if bytes[0] != 0x00 {
            return Err(anyhow::anyhow!(
                "unsupported key scheme flag; only ed25519 supported"
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes[1..]);

        // Derive address from the private key
        let derived_address = derive_address_from_secret_key(&arr);

        // If private key was provided, use derived address
        // Otherwise get and verify against environment address
        let addr = if using_provided_key {
            derived_address
        } else {
            let env_addr = sui::Address::from_str(&env::var("SUI_ADDRESS")?)?;
            if env_addr != derived_address {
                return Err(anyhow::anyhow!(
                    "Address mismatch: environment address does not match derived address"
                ));
            }
            env_addr
        };

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
        return Err(anyhow::anyhow!(
            "SUI_SECRET_KEY must contain at least 32 bytes"
        ));
    }

    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes[..32]);

    // Derive address from the private key
    let derived_address = derive_address_from_secret_key(&arr);

    // If private key was provided, use derived address
    // Otherwise get and verify against environment address
    let addr = if using_provided_key {
        derived_address
    } else {
        let env_addr = sui::Address::from_str(&env::var("SUI_ADDRESS")?)?;
        if env_addr != derived_address {
            return Err(anyhow::anyhow!(
                "Address mismatch: environment address does not match derived address"
            ));
        }
        env_addr
    };

    let sk = sui_crypto::ed25519::Ed25519PrivateKey::new(arr);
    Ok((addr, sk))
}

/// Get reference gas price from the network
pub async fn get_reference_gas_price(client: &mut GrpcClient) -> Result<u64> {
    let mut ledger = client.ledger_client();
    let _resp = ledger
        .get_service_info(proto::GetServiceInfoRequest::default())
        .await?
        .into_inner();
    // ServiceInfo does not expose gas price yet; default to 1000
    let price = 1_000u64;
    debug!("Using reference gas price: {}", price);
    Ok(price)
}

/// Pick a gas object owned by the sender
pub async fn pick_gas_object(
    client: &mut GrpcClient,
    sender: sui::Address,
) -> Result<sui::ObjectReference> {
    let mut state = client.state_client();
    debug!("Listing owned objects for sender: {}", sender);

    let mut list_req = proto::ListOwnedObjectsRequest::default();
    list_req.owner = Some(sender.to_string());
    list_req.page_size = Some(100);
    list_req.page_token = None;
    list_req.read_mask = Some(prost_types::FieldMask {
        paths: vec![
            "object_id".into(),
            "version".into(),
            "digest".into(),
            "object_type".into(),
        ],
    });
    list_req.object_type = None;

    let resp = state
        .list_owned_objects(list_req)
        .await?
        .into_inner();

    debug!("Owned objects returned: {}", resp.objects.len());

    let mut obj = resp
        .objects
        .into_iter()
        .find(|o| {
            o.object_type
                .as_ref()
                .map(|t| t.contains("::sui::SUI"))
                .unwrap_or(true)
        })
        .ok_or_else(|| anyhow::anyhow!("no owned objects to use as gas"))?;

    debug!(object = ?obj, "Selected object");

    if obj.digest.is_none() || obj.version.is_none() {
        debug!("Digest/version missing; fetching object details");
        let mut ledger = client.ledger_client();
        let object_id_str = obj
            .object_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("missing object id"))?;
        let mut get_obj_req = proto::GetObjectRequest::default();
        get_obj_req.object_id = Some(object_id_str.clone());
        get_obj_req.version = None;
        get_obj_req.read_mask = None;

        let got = ledger
            .get_object(get_obj_req)
            .await?
            .into_inner();

        if let Some(full) = got.object {
            debug!(object = ?full, "GetObject response");
            obj.object_id = full.object_id;
            obj.version = full.version;
            obj.digest = full.digest;
        }
    }

    let id = obj
        .object_id
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing object id"))?
        .parse()?;
    let version = obj
        .version
        .ok_or_else(|| anyhow::anyhow!("missing version"))?;
    let digest = obj
        .digest
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing digest"))?
        .parse()?;

    debug!(
        "Gas coin chosen: id={}, version={}, digest={}",
        id, version, digest
    );
    Ok(sui::ObjectReference::new(id, version, digest))
}
