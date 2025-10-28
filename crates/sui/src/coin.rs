use anyhow::Result;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sui_rpc::field::{FieldMask, FieldMaskUtil};
use sui_rpc::proto::sui::rpc::v2 as proto;
use sui_rpc::Client as GrpcClient;
use sui_sdk_types as sui;
use tracing::debug;

const MAX_RETRIES: u32 = 6;
const RETRY_DELAY_MS: u64 = 500;

/// RAII-style guard returned by `try_lock_coin`.
/// When this guard is dropped, the coin lock is automatically released.
pub struct CoinLockGuard {
    manager: CoinLockManager,
    coin_id: sui::Address,
}

impl CoinLockGuard {
    /// Get the coin ID that this guard is locking
    pub fn coin_id(&self) -> sui::Address {
        self.coin_id
    }
}

impl Drop for CoinLockGuard {
    fn drop(&mut self) {
        self.manager.release_coin(self.coin_id);
    }
}

/// Coin lock manager to prevent concurrent usage of the same coin
#[derive(Clone)]
pub struct CoinLockManager {
    locks: Arc<Mutex<HashMap<sui::Address, Instant>>>,
    lock_timeout: Duration,
}

impl CoinLockManager {
    pub fn new(lock_timeout_seconds: u64) -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
            lock_timeout: Duration::from_secs(lock_timeout_seconds),
        }
    }

    /// Attempts to lock a coin for exclusive use.
    /// Returns `Some(CoinLockGuard)` if the coin was successfully locked; `None` otherwise.
    pub fn try_lock_coin(&self, coin_id: sui::Address) -> Option<CoinLockGuard> {
        let mut locks = self.locks.lock();

        // Clean up expired locks first
        let now = Instant::now();
        locks.retain(|_, lock_time| now.duration_since(*lock_time) < self.lock_timeout);

        use std::collections::hash_map::Entry;
        match locks.entry(coin_id) {
            Entry::Occupied(_) => None, // already locked
            Entry::Vacant(entry) => {
                entry.insert(now);
                Some(CoinLockGuard {
                    manager: self.clone(),
                    coin_id,
                })
            }
        }
    }

    /// Releases a coin lock
    fn release_coin(&self, coin_id: sui::Address) {
        let mut locks = self.locks.lock();
        locks.remove(&coin_id);
    }

    /// Checks if a coin is currently locked
    #[allow(dead_code)]
    pub fn is_locked(&self, coin_id: sui::Address) -> bool {
        let mut locks = self.locks.lock();

        // Clean up expired locks first
        let now = Instant::now();
        locks.retain(|_, lock_time| now.duration_since(*lock_time) < self.lock_timeout);

        locks.contains_key(&coin_id)
    }
}

/// Global coin lock manager instance
static COIN_LOCK_MANAGER: std::sync::OnceLock<CoinLockManager> = std::sync::OnceLock::new();

pub fn get_coin_lock_manager() -> &'static CoinLockManager {
    COIN_LOCK_MANAGER.get_or_init(|| CoinLockManager::new(60)) // 60 seconds timeout (increased for safety)
}

#[derive(Debug, Clone)]
pub struct CoinInfo {
    pub object_ref: sui::ObjectReference,
    pub balance: u64,
}

impl CoinInfo {
    pub fn object_id(&self) -> sui::Address {
        *self.object_ref.object_id()
    }
}

/// Fetches a coin with sufficient balance and locks it for exclusive use
pub async fn fetch_coin(
    client: &mut GrpcClient,
    sender: sui::Address,
    min_balance: u64,
) -> Result<Option<(CoinInfo, CoinLockGuard)>> {
    let lock_manager = get_coin_lock_manager();

    for attempt in 1..=MAX_RETRIES {
        let mut state = client.state_client();

        // List owned objects to find SUI coins
        let mut request = proto::ListOwnedObjectsRequest::default();
        request.owner = Some(sender.to_string());
        request.page_size = Some(100);
        request.page_token = None;
        request.read_mask = Some(FieldMask::from_paths([
            "object_id",
            "version",
            "digest",
            "object_type",
            "contents",
        ]));
        request.object_type = Some("0x2::coin::Coin<0x2::sui::SUI>".to_string());

        let resp = state
            .list_owned_objects(request)
            .await?
            .into_inner();

        debug!("Attempt {}/{}: Found {} SUI coins for address {}", 
               attempt, MAX_RETRIES, resp.objects.len(), sender);

        // Collect all suitable coins first, then try to lock them
        let mut suitable_coins = Vec::new();
        for obj in resp.objects {
            if let (Some(id_str), Some(version), Some(digest_str)) =
                (&obj.object_id, obj.version, &obj.digest)
            {
                let object_id = sui::Address::from_str(id_str)?;
                let digest = sui::Digest::from_base58(digest_str)?;
                let object_ref = sui::ObjectReference::new(object_id, version, digest);

                // Extract coin balance from contents if available
                let balance = if let Some(contents) = &obj.contents {
                    if let Some(value) = &contents.value {
                        extract_coin_balance_from_contents(value)?
                    } else {
                        get_coin_balance_via_get_object(client, &object_ref).await?
                    }
                } else {
                    // Fallback: fetch object details to get balance
                    get_coin_balance_via_get_object(client, &object_ref).await?
                };

                if balance >= min_balance {
                    suitable_coins.push((object_id, object_ref, balance));
                }
            }
        }

        // Sort by balance ascending to prefer smaller coins first
        suitable_coins.sort_by(|a, b| a.2.cmp(&b.2));
        
        debug!("Found {} suitable coins with balance >= {} MIST ({:.4} SUI)", 
               suitable_coins.len(), min_balance, min_balance as f64 / 1_000_000_000.0);
        
        if suitable_coins.is_empty() {
            debug!("No coins with sufficient balance found. Need at least {} MIST ({:.4} SUI)",
                   min_balance, min_balance as f64 / 1_000_000_000.0);
        }

        // Try to lock coins in order of preference
        let mut locked_count = 0;
        for (object_id, object_ref, balance) in suitable_coins {
            // Try to lock this coin atomically
            if let Some(guard) = lock_manager.try_lock_coin(object_id) {
                debug!("Successfully locked coin {} with balance {} MIST ({:.4} SUI)",
                       object_id, balance, balance as f64 / 1_000_000_000.0);
                let coin_info = CoinInfo {
                    object_ref,
                    balance,
                };
                return Ok(Some((coin_info, guard)));
            } else {
                locked_count += 1;
                debug!("Coin {} is already locked (balance: {} MIST = {:.4} SUI)",
                       object_id, balance, balance as f64 / 1_000_000_000.0);
            }
        }
        
        if locked_count > 0 {
            debug!("All {} suitable coins are currently locked, will retry", locked_count);
        }

        if attempt < MAX_RETRIES {
            // Exponential backoff with jitter
            let delay = RETRY_DELAY_MS * (attempt as u64);
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
        }
    }

    debug!(
        "No unlocked coins found with sufficient balance after {} attempts (min_balance: {} MIST = {:.4} SUI)",
        MAX_RETRIES,
        min_balance,
        min_balance as f64 / 1_000_000_000.0
    );
    Ok(None)
}

/// Extracts coin balance from BCS contents
/// Coin<T> has layout: { id: UID, balance: Balance<T> }
/// Balance<T> has layout: { value: u64 }
/// We need to skip the UID (32 bytes) and read the balance u64
fn extract_coin_balance_from_contents(contents: &[u8]) -> Result<u64> {
    if contents.len() >= 40 {
        let balance_bytes = &contents[32..40];
        let balance = u64::from_le_bytes(balance_bytes.try_into().unwrap_or([0; 8]));
        Ok(balance)
    } else {
        Ok(0)
    }
}

/// Gets the balance of a specific coin object via get_object RPC
async fn get_coin_balance_via_get_object(
    client: &mut GrpcClient,
    object_ref: &sui::ObjectReference,
) -> Result<u64> {
    let mut ledger = client.ledger_client();

    let mut request = proto::GetObjectRequest::default();
    request.object_id = Some(object_ref.object_id().to_string());
    request.version = Some(object_ref.version());
    request.read_mask = Some(FieldMask::from_paths([
        "contents",
    ]));

    let resp = ledger
        .get_object(request)
        .await?
        .into_inner();

    if let Some(obj) = resp.object {
        if let Some(contents) = obj.contents {
            if let Some(value) = contents.value {
                return extract_coin_balance_from_contents(&value);
            }
        }
    }

    Ok(0)
}

/// Lists all available coins for a sender with their balances
pub async fn list_coins(client: &mut GrpcClient, sender: sui::Address) -> Result<Vec<CoinInfo>> {
    let mut state = client.state_client();

    let mut request = proto::ListOwnedObjectsRequest::default();
    request.owner = Some(sender.to_string());
    request.page_size = Some(100);
    request.page_token = None;
    request.read_mask = Some(FieldMask::from_paths([
        "object_id",
        "version",
        "digest",
        "object_type",
        "contents",
    ]));
    request.object_type = Some("0x2::coin::Coin<0x2::sui::SUI>".to_string());

    let resp = state
        .list_owned_objects(request)
        .await?
        .into_inner();

    let mut coins = Vec::new();

    for obj in resp.objects {
        if let (Some(id_str), Some(version), Some(digest_str)) =
            (&obj.object_id, obj.version, &obj.digest)
        {
            let object_id = sui::Address::from_str(id_str)?;
            let digest = sui::Digest::from_base58(digest_str)?;
            let object_ref = sui::ObjectReference::new(object_id, version, digest);

            let balance = if let Some(contents) = &obj.contents {
                if let Some(value) = &contents.value {
                    extract_coin_balance_from_contents(value)?
                } else {
                    get_coin_balance_via_get_object(client, &object_ref).await?
                }
            } else {
                get_coin_balance_via_get_object(client, &object_ref).await?
            };

            coins.push(CoinInfo {
                object_ref,
                balance,
            });
        }
    }

    Ok(coins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_coin_lock_manager() {
        let manager = CoinLockManager::new(1); // 1 second timeout
        let coin_id = sui::Address::from_str("0x123").unwrap();

        // Test locking
        let guard1 = manager.try_lock_coin(coin_id);
        assert!(guard1.is_some());

        // Test double locking fails
        let guard2 = manager.try_lock_coin(coin_id);
        assert!(guard2.is_none());

        // Test lock is released when guard is dropped
        drop(guard1);
        let guard3 = manager.try_lock_coin(coin_id);
        assert!(guard3.is_some());
    }

    #[test]
    fn test_coin_lock_timeout() {
        let manager = CoinLockManager::new(0); // 0 second timeout for immediate expiry
        let coin_id = sui::Address::from_str("0x123").unwrap();

        // Lock the coin
        {
            let _guard = manager.try_lock_coin(coin_id);
            // Guard goes out of scope but lock is still in the HashMap
        }

        // Sleep a tiny bit to ensure timeout
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Should be able to lock again due to timeout cleanup
        let guard = manager.try_lock_coin(coin_id);
        assert!(guard.is_some());
    }

    #[test]
    fn test_extract_coin_balance() {
        // Test with valid coin data (32 bytes UID + 8 bytes balance)
        let mut coin_data = vec![0u8; 40];
        // Set balance to 1000000000 (little endian)
        coin_data[32..40].copy_from_slice(&1000000000u64.to_le_bytes());

        let balance = extract_coin_balance_from_contents(&coin_data).unwrap();
        assert_eq!(balance, 1000000000);

        // Test with insufficient data
        let short_data = vec![0u8; 32];
        let balance = extract_coin_balance_from_contents(&short_data).unwrap();
        assert_eq!(balance, 0);
    }
}
