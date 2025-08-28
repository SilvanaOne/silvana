use anyhow::Result;
use dotenvy::dotenv;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use sui::state::SharedSuiState;
use sui::fetch::app_instance::fetch_app_instance;
use sui::fetch::app_instance_bcs::fetch_app_instance_bcs;

#[tokio::test]
async fn compare_app_instance_bcs_and_json() -> Result<()> {
    dotenv().ok();

    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let rpc_url = std::env::var("SUI_RPC_URL")
        .expect("SUI_RPC_URL must be set (e.g., https://fullnode.devnet.sui.io:443)");
    SharedSuiState::initialize(&rpc_url).await?;

    let id = "0x322483d8620f5ce9ded4aa3d72b4dac890e585ba6afe6dd9475b8c1cef99b6b5";

    let bcs_res = fetch_app_instance_bcs(id).await;
    let json_res = fetch_app_instance(id).await;

    if let Ok(ref app_bcs) = bcs_res {
        info!("BCS AppInstance: {:#?}", app_bcs);
    } else if let Err(e) = &bcs_res {
        error!("BCS fetch failed: {e}");
    }

    if let Ok(ref app_json) = json_res {
        info!("JSON AppInstance: {:#?}", app_json);
    } else if let Err(e) = &json_res {
        error!("JSON fetch failed: {e}");
    }

    // If both succeeded, compare field-by-field and report differences
    if let (Ok(app_bcs), Ok(app_json)) = (bcs_res, json_res) {
        // Compare core fields directly
        let mut mismatches = Vec::new();
        macro_rules! cmp {
            ($field:ident) => {
                if app_bcs.$field != app_json.$field { mismatches.push(stringify!($field)); }
            };
        }
        cmp!(id);
        cmp!(silvana_app_name);
        cmp!(description);
        cmp!(metadata);
        cmp!(kv);
        cmp!(sequence);
        cmp!(admin);
        cmp!(block_number);
        cmp!(previous_block_timestamp);
        cmp!(previous_block_last_sequence);
        cmp!(last_proved_block_number);
        cmp!(last_settled_block_number);
        cmp!(settlement_chain);
        cmp!(settlement_address);
        cmp!(is_paused);
        cmp!(created_at);
        cmp!(updated_at);
        cmp!(blocks_table_id);
        cmp!(proof_calculations_table_id);

        if mismatches.is_empty() {
            info!("✅ Core fields match between BCS and JSON results");
        } else {
            error!("❌ Core field mismatches: {:?}", mismatches);
        }

        // Show known-different complex fields equality status
        let jobs_presence_equal = app_bcs.jobs.is_some() == app_json.jobs.is_some();
        info!(
            "Complex fields equality: methods={} state={} sequence_state_manager={} previous_block_actions_state={} jobs_presence={} (skipped deep compare)",
            (app_bcs.methods == app_json.methods),
            (app_bcs.state == app_json.state),
            (app_bcs.sequence_state_manager == app_json.sequence_state_manager),
            (app_bcs.previous_block_actions_state == app_json.previous_block_actions_state),
            jobs_presence_equal
        );
    }

    Ok(())
}


