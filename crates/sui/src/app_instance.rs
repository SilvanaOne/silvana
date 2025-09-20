use anyhow::{Context, Result, anyhow};
use sui_sdk_types as sui;
use tracing::{debug, info, warn};

use crate::state::SharedSuiState;
use crate::transactions::{execute_transaction_block, get_object_id};

/// Define Move types for serialization
#[derive(serde::Serialize)]
struct MoveString {
    bytes: Vec<u8>,
}

#[derive(serde::Serialize)]
struct MoveOption<T> {
    vec: Vec<T>,
}

/// Create and submit a transaction to start a job
pub(crate) async fn start_job_tx(app_instance_str: &str, job_sequence: u64) -> Result<String> {
    debug!(
        "Creating start_app_job transaction for job_sequence: {}",
        job_sequence
    );

    execute_app_instance_function(
        app_instance_str,
        "start_app_job",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));
            vec![app_instance_arg, job_sequence_arg, clock_arg]
        },
    )
    .await
}

/// Create and submit a transaction to complete a job
pub(crate) async fn complete_job_tx(app_instance_str: &str, job_sequence: u64) -> Result<String> {
    debug!(
        "Creating complete_app_job transaction for job_sequence: {}",
        job_sequence
    );

    execute_app_instance_function(
        app_instance_str,
        "complete_app_job",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));
            vec![app_instance_arg, job_sequence_arg, clock_arg]
        },
    )
    .await
}

/// Create and submit a transaction to fail a job
pub(crate) async fn fail_job_tx(
    app_instance_str: &str,
    job_sequence: u64,
    error_message: &str,
) -> Result<String> {
    debug!(
        "Creating fail_app_job transaction for job_sequence: {} with error: {}",
        job_sequence, error_message
    );

    // Debug: Query current job state before attempting to fail it
    let app_instance_id =
        get_object_id(app_instance_str).context("Failed to parse object ID for debug query")?;
    match query_job_status(app_instance_id, job_sequence).await {
        Ok(status) => {
            debug!(
                "Current job {} status before fail attempt: {:?}",
                job_sequence, status
            );
        }
        Err(e) => {
            warn!(
                "Failed to query job {} status before fail: {}",
                job_sequence, e
            );
        }
    }

    let error_msg = error_message.to_string();
    execute_app_instance_function(
        app_instance_str,
        "fail_app_job",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));
            // Use MoveString for proper serialization
            let error_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: error_msg.clone().into_bytes(),
            }));
            vec![app_instance_arg, job_sequence_arg, error_arg, clock_arg]
        },
    )
    .await
}

/// Create and submit a transaction to terminate a job
pub(crate) async fn terminate_job_tx(
    app_instance_str: &str,
    job_sequence: u64,
    gas_budget: Option<u64>,
) -> Result<String> {
    debug!(
        "Creating terminate_app_job transaction for job_sequence: {}",
        job_sequence
    );

    execute_app_instance_function_with_gas(
        app_instance_str,
        "terminate_app_job",
        gas_budget,
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));
            vec![app_instance_arg, job_sequence_arg, clock_arg]
        },
    )
    .await
}

/// Create and submit a multicall transaction for batch job operations
/// Executes operations in order: complete, fail, terminate, start, update_state_for_sequences, submit_proofs, create_jobs and create_merge_jobs
/// update_state_for_sequences, submit_proofs, create_jobs and create_merge_jobs are executed as separate move calls (one per operation)
pub(crate) async fn multicall_job_operations_tx(
    operations: Vec<crate::types::MulticallOperations>,
    gas_budget: Option<u64>,
) -> Result<String> {
    if operations.is_empty() {
        return Err(anyhow!("No operations provided"));
    }

    // Validate all operations
    for (i, op) in operations.iter().enumerate() {
        if let Err(e) = op.validate() {
            return Err(anyhow!("Validation failed for operation {}: {}", i, e));
        }
    }

    let shared_state = SharedSuiState::get_instance();
    let package_id = shared_state.get_coordination_package_id_required();

    let app_instances: Vec<String> = operations
        .iter()
        .map(|op| op.app_instance.clone())
        .collect();
    debug!(
        "Creating multicall transaction for {} app instances: {:?}",
        operations.len(),
        app_instances
    );

    // Build operations list for all app instances - format: (object_ids, module_name, function_name, builder)
    let mut tx_operations: Vec<(
        Vec<String>,
        String,
        String,
        Box<
            dyn Fn(
                    &mut sui_transaction_builder::TransactionBuilder,
                    Vec<sui_sdk_types::Argument>,
                    sui_sdk_types::Argument,
                ) -> Vec<sui_sdk_types::Argument>
                + Send,
        >,
    )> = Vec::new();

    // Process each app instance
    for op in &operations {
        let app_instance = op.app_instance.clone();

        // Only add multicall operation if there are any job operations
        let has_job_operations = !op.complete_job_sequences.is_empty()
            || !op.fail_job_sequences.is_empty()
            || !op.terminate_job_sequences.is_empty()
            || !op.start_job_sequences.is_empty();

        if has_job_operations {
            let complete = op.complete_job_sequences.clone();
            let fail_seqs = op.fail_job_sequences.clone();
            let fail_errs = op.fail_errors.clone();
            let terminate = op.terminate_job_sequences.clone();
            let start = op.start_job_sequences.clone();
            let start_mem = op.start_job_memory_requirements.clone();
            let avail_mem = op.available_memory;

            tx_operations.push((
                vec![app_instance.clone()],
                "app_instance".to_string(),
                "multicall_app_job_operations".to_string(),
                Box::new(move |tb, object_args, clock_arg| {
                    let app_instance_arg =
                        *object_args.get(0).expect("App instance argument required");
                    // Create vector arguments for each operation type
                    let complete_arg = tb.input(sui_transaction_builder::Serialized(&complete));
                    let fail_sequences_arg =
                        tb.input(sui_transaction_builder::Serialized(&fail_seqs));
                    // Convert Vec<String> to Vec<MoveString> for proper serialization
                    let fail_errors_move: Vec<MoveString> = fail_errs
                        .iter()
                        .map(|s| MoveString {
                            bytes: s.clone().into_bytes(),
                        })
                        .collect();
                    let fail_errors_arg =
                        tb.input(sui_transaction_builder::Serialized(&fail_errors_move));
                    let terminate_arg = tb.input(sui_transaction_builder::Serialized(&terminate));
                    let start_arg = tb.input(sui_transaction_builder::Serialized(&start));
                    let start_memory_arg =
                        tb.input(sui_transaction_builder::Serialized(&start_mem));
                    let available_memory_arg =
                        tb.input(sui_transaction_builder::Serialized(&avail_mem));

                    vec![
                        app_instance_arg,
                        complete_arg,
                        fail_sequences_arg,
                        fail_errors_arg,
                        terminate_arg,
                        start_arg,
                        start_memory_arg,
                        available_memory_arg,
                        clock_arg,
                    ]
                }),
            ));
        }

        // Add one operation for each update_state_for_sequence
        for (sequence, new_state_data, new_data_availability_hash) in &op.update_state_for_sequences
        {
            let seq = *sequence;
            let state_data = new_state_data.clone();
            let da_hash = new_data_availability_hash.clone();

            tx_operations.push((
                vec![app_instance.clone()],
                "app_instance".to_string(),
                "update_state_for_sequence".to_string(),
                Box::new(move |tb, object_args, clock_arg| {
                    let app_instance_arg =
                        *object_args.get(0).expect("App instance argument required");
                    let sequence_arg = tb.input(sui_transaction_builder::Serialized(&seq));
                    let state_data_arg = tb.input(sui_transaction_builder::Serialized(&state_data));
                    // Convert Option<String> to MoveOption<MoveString> for proper serialization
                    let da_hash_move = MoveOption {
                        vec: da_hash
                            .clone()
                            .map(|s| MoveString {
                                bytes: s.into_bytes(),
                            })
                            .into_iter()
                            .collect(),
                    };
                    let da_hash_arg = tb.input(sui_transaction_builder::Serialized(&da_hash_move));

                    vec![
                        app_instance_arg,
                        sequence_arg,
                        state_data_arg,
                        da_hash_arg,
                        clock_arg,
                    ]
                }),
            ));
        }

        // Add one operation for each submit_proof
        for (
            block_number,
            sequences,
            merged_sequences_1,
            merged_sequences_2,
            job_id,
            da_hash,
            cpu_cores,
            prover_architecture,
            prover_memory,
            cpu_time,
        ) in &op.submit_proofs
        {
            let block = *block_number;
            let seqs = sequences.clone();
            let merged_seqs_1 = merged_sequences_1.clone();
            let merged_seqs_2 = merged_sequences_2.clone();
            let job = job_id.clone();
            let hash = da_hash.clone();
            let cores = *cpu_cores;
            let arch = prover_architecture.clone();
            let memory = *prover_memory;
            let time = *cpu_time;

            tx_operations.push((
                vec![app_instance.clone()],
                "app_instance".to_string(),
                "submit_proof".to_string(),
                Box::new(move |tb, object_args, clock_arg| {
                    let app_instance_arg =
                        *object_args.get(0).expect("App instance argument required");
                    let block_arg = tb.input(sui_transaction_builder::Serialized(&block));
                    let sequences_arg = tb.input(sui_transaction_builder::Serialized(&seqs));
                    // Convert Option<Vec<u64>> to MoveOption<Vec<u64>>
                    let merged_seqs_1_move = MoveOption {
                        vec: merged_seqs_1.clone().into_iter().collect(),
                    };
                    let merged_seqs_1_arg =
                        tb.input(sui_transaction_builder::Serialized(&merged_seqs_1_move));
                    let merged_seqs_2_move = MoveOption {
                        vec: merged_seqs_2.clone().into_iter().collect(),
                    };
                    let merged_seqs_2_arg =
                        tb.input(sui_transaction_builder::Serialized(&merged_seqs_2_move));
                    // Use MoveString for string parameters
                    let job_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                        bytes: job.clone().into_bytes(),
                    }));
                    let hash_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                        bytes: hash.clone().into_bytes(),
                    }));
                    let cores_arg = tb.input(sui_transaction_builder::Serialized(&cores));
                    let arch_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                        bytes: arch.clone().into_bytes(),
                    }));
                    let memory_arg = tb.input(sui_transaction_builder::Serialized(&memory));
                    let time_arg = tb.input(sui_transaction_builder::Serialized(&time));

                    vec![
                        app_instance_arg,
                        block_arg,
                        sequences_arg,
                        merged_seqs_1_arg,
                        merged_seqs_2_arg,
                        job_arg,
                        hash_arg,
                        cores_arg,
                        arch_arg,
                        memory_arg,
                        time_arg,
                        clock_arg,
                    ]
                }),
            ));
        }

        // Add one operation for each create_job (e.g., settlement jobs)
        for (
            method_name,
            job_description,
            block_number,
            sequences,
            sequences1,
            sequences2,
            data,
            interval_ms,
            next_scheduled_at,
            settlement_chain,
        ) in &op.create_jobs
        {
            let method = method_name.clone();
            let desc = job_description.clone();
            let block = *block_number;
            let seqs = sequences.clone();
            let seqs1 = sequences1.clone();
            let seqs2 = sequences2.clone();
            let job_data = data.clone();
            let interval = *interval_ms;
            let scheduled = *next_scheduled_at;
            let chain = settlement_chain.clone();

            tx_operations.push((
                vec![app_instance.clone()],
                "app_instance".to_string(),
                "create_app_job".to_string(),
                Box::new(move |tb, object_args, clock_arg| {
                    let app_instance_arg =
                        *object_args.get(0).expect("App instance argument required");
                    let method_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                        bytes: method.clone().into_bytes(),
                    }));
                    // Convert Option<String> to MoveOption<MoveString>
                    let desc_move = MoveOption {
                        vec: desc
                            .clone()
                            .map(|s| MoveString {
                                bytes: s.into_bytes(),
                            })
                            .into_iter()
                            .collect(),
                    };
                    let description_arg = tb.input(sui_transaction_builder::Serialized(&desc_move));
                    let block_arg = tb.input(sui_transaction_builder::Serialized(&block));
                    let sequences_arg = tb.input(sui_transaction_builder::Serialized(&seqs));
                    let sequences1_arg = tb.input(sui_transaction_builder::Serialized(&seqs1));
                    let sequences2_arg = tb.input(sui_transaction_builder::Serialized(&seqs2));
                    // job_data is already Vec<u8>, so use MoveString directly with it
                    let data_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                        bytes: job_data.clone(),
                    }));
                    let interval_arg = tb.input(sui_transaction_builder::Serialized(&interval));
                    let scheduled_arg = tb.input(sui_transaction_builder::Serialized(&scheduled));
                    // Convert Option<String> to MoveOption<MoveString>
                    let chain_move = MoveOption {
                        vec: chain
                            .clone()
                            .map(|s| MoveString {
                                bytes: s.into_bytes(),
                            })
                            .into_iter()
                            .collect(),
                    };
                    let chain_arg = tb.input(sui_transaction_builder::Serialized(&chain_move));

                    vec![
                        app_instance_arg,
                        method_arg,
                        description_arg,
                        block_arg,
                        sequences_arg,
                        sequences1_arg,
                        sequences2_arg,
                        data_arg,
                        interval_arg,
                        scheduled_arg,
                        chain_arg,
                        clock_arg,
                    ]
                }),
            ));
        }

        // Add one operation for each create_merge_job
        for (block_number, sequences, sequences1, sequences2, job_description) in
            &op.create_merge_jobs
        {
            let block = *block_number;
            let seqs = sequences.clone();
            let seqs1 = sequences1.clone();
            let seqs2 = sequences2.clone();
            let desc = job_description.clone();

            tx_operations.push((
                vec![app_instance.clone()],
                "app_instance".to_string(),
                "create_merge_job".to_string(),
                Box::new(move |tb, object_args, clock_arg| {
                    let app_instance_arg =
                        *object_args.get(0).expect("App instance argument required");
                    let block_arg = tb.input(sui_transaction_builder::Serialized(&block));
                    let sequences_arg = tb.input(sui_transaction_builder::Serialized(&seqs));
                    let sequences1_arg = tb.input(sui_transaction_builder::Serialized(&seqs1));
                    let sequences2_arg = tb.input(sui_transaction_builder::Serialized(&seqs2));
                    let description_arg = tb.input(sui_transaction_builder::Serialized(&desc));

                    vec![
                        app_instance_arg,
                        block_arg,
                        sequences_arg,
                        sequences1_arg,
                        sequences2_arg,
                        description_arg,
                        clock_arg,
                    ]
                }),
            ));
        }
    }

    if tx_operations.is_empty() {
        return Err(anyhow!("No operations to execute in multicall"));
    }

    // Use the updated execute_transaction_block for multiple move calls across multiple app instances
    execute_transaction_block(package_id, tx_operations, gas_budget, None).await
}

/// Create and submit a transaction to restart failed jobs (with optional specific job sequences)
pub async fn restart_failed_jobs_with_sequences_tx(
    app_instance_str: &str,
    job_sequences: Option<Vec<u64>>,
    gas_budget: Option<u64>,
) -> Result<String> {
    debug!(
        "Creating restart_failed_app_jobs transaction with job_sequences: {:?}",
        job_sequences
    );

    // Default to 1 SUI for this potentially heavy operation if not specified
    let gas = gas_budget.or(Some(1_000_000_000));

    execute_app_instance_function_with_gas(
        app_instance_str,
        "restart_failed_app_jobs",
        gas,
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let job_sequences_arg = tb.input(sui_transaction_builder::Serialized(&job_sequences));
            vec![app_instance_arg, job_sequences_arg, clock_arg]
        },
    )
    .await
}

/// Create and submit a transaction to remove failed jobs (with optional specific job sequences)
pub async fn remove_failed_jobs_tx(
    app_instance_str: &str,
    job_sequences: Option<Vec<u64>>,
    gas_budget: Option<u64>,
) -> Result<String> {
    debug!(
        "Creating remove_failed_app_jobs transaction with job_sequences: {:?}",
        job_sequences
    );

    execute_app_instance_function_with_gas(
        app_instance_str,
        "remove_failed_app_jobs",
        gas_budget,
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let job_sequences_arg = tb.input(sui_transaction_builder::Serialized(&job_sequences));
            vec![app_instance_arg, job_sequences_arg, clock_arg]
        },
    )
    .await
}

/// Create and submit a transaction to submit a proof
#[allow(dead_code)]
pub(crate) async fn submit_proof_tx(
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,
    merged_sequences_1: Option<Vec<u64>>,
    merged_sequences_2: Option<Vec<u64>>,
    job_id: String,
    da_hash: String,
    cpu_cores: u8,
    prover_architecture: String,
    prover_memory: u64,
    cpu_time: u64,
) -> Result<String> {
    debug!(
        "Creating submit_proof transaction for block_number: {}, job_id: {}",
        block_number, job_id
    );

    execute_app_instance_function(
        app_instance_str,
        "submit_proof",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            let merged_sequences_1_arg =
                tb.input(sui_transaction_builder::Serialized(&merged_sequences_1));
            let merged_sequences_2_arg =
                tb.input(sui_transaction_builder::Serialized(&merged_sequences_2));
            let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));
            let da_hash_arg = tb.input(sui_transaction_builder::Serialized(&da_hash));
            let cpu_cores_arg = tb.input(sui_transaction_builder::Serialized(&cpu_cores));
            let prover_architecture_arg =
                tb.input(sui_transaction_builder::Serialized(&prover_architecture));
            let prover_memory_arg = tb.input(sui_transaction_builder::Serialized(&prover_memory));
            let cpu_time_arg = tb.input(sui_transaction_builder::Serialized(&cpu_time));

            vec![
                app_instance_arg,
                block_number_arg,
                sequences_arg,
                merged_sequences_1_arg,
                merged_sequences_2_arg,
                job_id_arg,
                da_hash_arg,
                cpu_cores_arg,
                prover_architecture_arg,
                prover_memory_arg,
                cpu_time_arg,
                clock_arg,
            ]
        },
    )
    .await
}

/// Create and submit a transaction to update state for a sequence
#[allow(dead_code)]
pub(crate) async fn update_state_for_sequence_tx(
    app_instance_str: &str,
    sequence: u64,
    new_state_data: Option<Vec<u8>>,
    new_data_availability_hash: Option<String>,
) -> Result<String> {
    debug!(
        "Creating update_state_for_sequence transaction for sequence: {}",
        sequence
    );

    execute_app_instance_function(
        app_instance_str,
        "update_state_for_sequence",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let sequence_arg = tb.input(sui_transaction_builder::Serialized(&sequence));
            let new_state_data_arg = tb.input(sui_transaction_builder::Serialized(&new_state_data));
            let new_data_availability_hash_arg = tb.input(sui_transaction_builder::Serialized(
                &new_data_availability_hash,
            ));

            vec![
                app_instance_arg,
                sequence_arg,
                new_state_data_arg,
                new_data_availability_hash_arg,
                clock_arg,
            ]
        },
    )
    .await
}

/// Create and submit a transaction to create an app job
/// This is a general function that can create any type of job by specifying method_name and data
pub async fn create_app_job_tx(
    app_instance_str: &str,
    method_name: String,
    job_description: Option<String>,
    block_number: Option<u64>,
    sequences: Option<Vec<u64>>,
    sequences1: Option<Vec<u64>>,
    sequences2: Option<Vec<u64>>,
    data: Vec<u8>,
    interval_ms: Option<u64>,
    next_scheduled_at: Option<u64>,
    settlement_chain: Option<String>,
) -> Result<String> {
    debug!(
        "Creating app job transaction for method: {}, data size: {} bytes",
        method_name,
        data.len()
    );

    let method_name_clone = method_name.clone();
    execute_app_instance_function(
        app_instance_str,
        "create_app_job",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let method_name_arg = tb.input(sui_transaction_builder::Serialized(&method_name));
            let job_description_arg =
                tb.input(sui_transaction_builder::Serialized(&job_description));
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            let sequences1_arg = tb.input(sui_transaction_builder::Serialized(&sequences1));
            let sequences2_arg = tb.input(sui_transaction_builder::Serialized(&sequences2));
            let data_arg = tb.input(sui_transaction_builder::Serialized(&data));
            // Add the periodic job parameters and settlement_chain
            let interval_ms_arg = tb.input(sui_transaction_builder::Serialized(&interval_ms));
            let next_scheduled_at_arg =
                tb.input(sui_transaction_builder::Serialized(&next_scheduled_at));
            let settlement_chain_arg =
                tb.input(sui_transaction_builder::Serialized(&settlement_chain));

            vec![
                app_instance_arg,
                method_name_arg,
                job_description_arg,
                block_number_arg,
                sequences_arg,
                sequences1_arg,
                sequences2_arg,
                data_arg,
                interval_ms_arg,
                next_scheduled_at_arg,
                settlement_chain_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        anyhow!(
            "Failed to create app job for method '{}': {}",
            method_name_clone,
            e
        )
    })
}

/// Create and submit a transaction to reject a proof
pub async fn reject_proof_tx(
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,
) -> Result<String> {
    debug!(
        "Creating reject_proof transaction for block_number: {}, sequences: {:?}",
        block_number, sequences
    );

    execute_app_instance_function(
        app_instance_str,
        "reject_proof",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            vec![app_instance_arg, block_number_arg, sequences_arg, clock_arg]
        },
    )
    .await
}

/// Create and submit a transaction to start proving (reserve proofs)
/// DEPRECATED: Use create_merge_job_with_proving_tx instead
#[allow(dead_code)]
pub(crate) async fn start_proving_tx(
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,
    merged_sequences_1: Option<Vec<u64>>,
    merged_sequences_2: Option<Vec<u64>>,
) -> Result<String> {
    debug!(
        "Creating start_proving transaction for block_number: {}, sequences: {:?}",
        block_number, sequences
    );

    // Use the helper but handle expected errors specially
    execute_app_instance_function(
        app_instance_str,
        "start_proving",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            let merged_sequences_1_arg =
                tb.input(sui_transaction_builder::Serialized(&merged_sequences_1));
            let merged_sequences_2_arg =
                tb.input(sui_transaction_builder::Serialized(&merged_sequences_2));

            vec![
                app_instance_arg,
                block_number_arg,
                sequences_arg,
                merged_sequences_1_arg,
                merged_sequences_2_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        // Special handling for expected errors
        let error_str = e.to_string();
        if error_str.contains("Transaction failed") {
            info!(
                "start_proving transaction failed (proofs may be already reserved): {}",
                error_str
            );
            anyhow!("Failed to reserve proofs - may be already reserved by another coordinator")
        } else {
            e
        }
    })
}

/// Convenience function to create a merge job (old version - just creates the job)
/// Use create_merge_job_with_proving_tx for the new version that reserves proofs first
#[allow(dead_code)]
pub(crate) async fn create_merge_job_tx(
    app_instance_str: &str,
    block_number: u64,
    sequences1: Vec<u64>,
    sequences2: Vec<u64>,
    job_description: Option<String>,
) -> Result<String> {
    // Combine and sort sequences from both proofs
    let mut combined_sequences = sequences1.clone();
    combined_sequences.extend(sequences2.clone());
    combined_sequences.sort();
    combined_sequences.dedup(); // Remove any duplicates

    debug!("Combined sequences for merge job: {:?}", combined_sequences);
    debug!(
        "Block number: {}, sequences1: {:?}, sequences2: {:?}",
        block_number, sequences1, sequences2
    );

    // Call the general create_app_job_tx function with the new fields
    create_app_job_tx(
        app_instance_str,
        "merge".to_string(),
        job_description,
        Some(block_number),       // Pass block_number
        Some(combined_sequences), // Pass the combined sequences
        Some(sequences1),         // Pass sequences1
        Some(sequences2),         // Pass sequences2
        vec![],                   // Empty data since we're using the Job fields now
        None,                     // No interval for merge jobs
        None,                     // No scheduled time for merge jobs
        None,                     // Not a settlement job
    )
    .await
}

/// Create a merge job with proof reservation
/// This function first calls start_proving to reserve the proofs, then creates the job
/// Returns the transaction digest on success
#[allow(dead_code)]
pub(crate) async fn create_merge_job_with_proving_tx(
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,  // Combined sequences
    sequences1: Vec<u64>, // First proof sequences
    sequences2: Vec<u64>, // Second proof sequences
    job_description: Option<String>,
) -> Result<String> {
    debug!(
        "Creating merge job with proving for block {}, combined sequences: {:?}, sequences1: {:?}, sequences2: {:?}",
        block_number, sequences, sequences1, sequences2
    );

    execute_app_instance_function(
        app_instance_str,
        "create_merge_job",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            let sequences1_arg = tb.input(sui_transaction_builder::Serialized(&sequences1));
            let sequences2_arg = tb.input(sui_transaction_builder::Serialized(&sequences2));
            let job_description_arg =
                tb.input(sui_transaction_builder::Serialized(&job_description));

            vec![
                app_instance_arg,
                block_number_arg,
                sequences_arg,
                sequences1_arg,
                sequences2_arg,
                job_description_arg,
                clock_arg,
            ]
        },
    )
    .await
}

/// Update block proof data availability
pub async fn update_block_proof_data_availability_tx(
    app_instance_str: &str,
    block_number: u64,
    proof_data_availability: String,
) -> Result<String> {
    debug!(
        "Creating update_block_proof_data_availability transaction for block_number: {}",
        block_number
    );

    execute_app_instance_function(
        app_instance_str,
        "update_block_proof_data_availability",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let proof_da_arg = tb.input(sui_transaction_builder::Serialized(
                &proof_data_availability,
            ));

            vec![app_instance_arg, block_number_arg, proof_da_arg, clock_arg]
        },
    )
    .await
}

/// Update block state data availability
pub async fn update_block_state_data_availability_tx(
    app_instance_str: &str,
    block_number: u64,
    state_data_availability: String,
) -> Result<String> {
    debug!(
        "Creating update_block_state_data_availability transaction for block_number: {}",
        block_number
    );

    execute_app_instance_function(
        app_instance_str,
        "update_block_state_data_availability",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let state_da_arg = tb.input(sui_transaction_builder::Serialized(
                &state_data_availability,
            ));

            vec![app_instance_arg, block_number_arg, state_da_arg, clock_arg]
        },
    )
    .await
}

/// Create and submit a transaction to increase sequence (add new action)
pub async fn increase_sequence_tx(
    app_instance_str: &str,
    optimistic_state: Vec<u8>,
    transition_data: Vec<u8>,
) -> Result<String> {
    debug!(
        "Creating increase_sequence transaction with optimistic_state size: {} bytes, transition_data size: {} bytes",
        optimistic_state.len(),
        transition_data.len()
    );

    execute_app_instance_function(
        app_instance_str,
        "increase_sequence",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let optimistic_state_arg =
                tb.input(sui_transaction_builder::Serialized(&optimistic_state));
            let transition_data_arg =
                tb.input(sui_transaction_builder::Serialized(&transition_data));

            vec![
                app_instance_arg,
                optimistic_state_arg,
                transition_data_arg,
                clock_arg,
            ]
        },
    )
    .await
}

/// Convenience function to create a settle job
pub async fn create_settle_job_tx(
    app_instance_str: &str,
    block_number: u64,
    chain: String,
    job_description: Option<String>,
) -> Result<String> {
    // Call the general create_app_job_tx function with settle method
    // Settlement jobs are periodic with 1 minute interval
    let interval_ms = 60000u64; // 1 minute in milliseconds
    let next_scheduled_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    create_app_job_tx(
        app_instance_str,
        "settle".to_string(),
        job_description,
        Some(block_number),      // Pass block_number
        None,                    // No sequences for settle
        None,                    // No sequences1 for settle
        None,                    // No sequences2 for settle
        vec![],                  // Empty data
        Some(interval_ms),       // 1 minute interval for periodic settle jobs
        Some(next_scheduled_at), // Start now
        Some(chain),             // This is a settlement job for the specified chain
    )
    .await
}

/// Terminate an app job
pub async fn terminate_app_job_tx(app_instance_str: &str, job_id: u64) -> Result<String> {
    debug!(
        "Creating terminate_app_job transaction for job_id: {}",
        job_id
    );

    execute_app_instance_function(
        app_instance_str,
        "terminate_app_job",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));

            vec![app_instance_arg, job_id_arg, clock_arg]
        },
    )
    .await
}

/// Update block settlement transaction hash
pub async fn update_block_settlement_tx_hash_tx(
    app_instance_str: &str,
    block_number: u64,
    chain: String,
    settlement_tx_hash: String,
) -> Result<String> {
    debug!(
        "Creating update_block_settlement_tx_hash transaction for block_number: {} on chain: {}",
        block_number, chain
    );

    execute_app_instance_function(
        app_instance_str,
        "update_block_settlement_tx_hash",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            // Order must match Move function signature: chain, block_number, settlement_tx_hash
            let chain_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: chain.clone().into_bytes(),
            }));
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let settlement_tx_hash_arg =
                tb.input(sui_transaction_builder::Serialized(&MoveString {
                    bytes: settlement_tx_hash.clone().into_bytes(),
                }));

            vec![
                app_instance_arg,
                chain_arg,
                block_number_arg,
                settlement_tx_hash_arg,
                clock_arg,
            ]
        },
    )
    .await
}

/// Update block settlement transaction included in block
pub async fn update_block_settlement_tx_included_in_block_tx(
    app_instance_str: &str,
    block_number: u64,
    chain: String,
    settled_at: u64,
) -> Result<String> {
    debug!(
        "Creating update_block_settlement_tx_included_in_block transaction for block_number: {} on chain: {}",
        block_number, chain
    );

    execute_app_instance_function(
        app_instance_str,
        "update_block_settlement_tx_included_in_block",
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");

            // Order must match Move function signature: chain, block_number, settled_at
            let chain_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: chain.clone().into_bytes(),
            }));
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let settled_at_arg = tb.input(sui_transaction_builder::Serialized(&settled_at));

            vec![
                app_instance_arg,
                chain_arg,
                block_number_arg,
                settled_at_arg,
                clock_arg,
            ]
        },
    )
    .await
}

/// Debug function to query job status from the blockchain
async fn query_job_status(app_instance_id: sui::Address, job_sequence: u64) -> Result<String> {
    use crate::fetch::app_instance::fetch_app_instance;

    // Use the existing fetch_app_instance function which handles formatting correctly
    match fetch_app_instance(&app_instance_id.to_string()).await {
        Ok(app_instance) => {
            // Check if this is a settlement job for any chain
            for (chain, settlement) in &app_instance.settlements {
                if let Some(settlement_job) = settlement.settlement_job {
                    if settlement_job == job_sequence {
                        debug!(
                            "Job {} is the settlement job for chain {}",
                            job_sequence, chain
                        );
                        return Ok(format!(
                            "Job {} is settlement job for chain {}",
                            job_sequence, chain
                        ));
                    }
                }
            }

            debug!("Found app_instance for job {} check", job_sequence);
            return Ok(format!("Job {} status check completed", job_sequence));
        }
        Err(e) => {
            // Don't warn for fetch errors as they're expected for some job types
            debug!(
                "Could not fetch app_instance for job {} status check: {}",
                job_sequence, e
            );
            return Ok(format!("Job {} status check skipped", job_sequence));
        }
    }
}

/// Common helper function to execute app instance transactions
/// This reduces code duplication while maintaining all error detection and features
/// Returns the transaction digest and keeps the coin lock guard alive
async fn execute_app_instance_function<F>(
    app_instance_str: &str,
    function_name: &str,
    build_args: F,
) -> Result<String>
where
    F: Fn(
        &mut sui_transaction_builder::TransactionBuilder,
        Vec<sui_sdk_types::Argument>, // object_args
        sui_sdk_types::Argument,      // clock_arg
    ) -> Vec<sui_sdk_types::Argument>,
{
    execute_app_instance_function_with_gas(app_instance_str, function_name, None, build_args).await
}

/// Common helper function to execute app instance transactions with custom gas budget
/// This reduces code duplication while maintaining all error detection and features
/// Returns the transaction digest and keeps the coin lock guard alive
/// Includes retry logic for Object version conflicts (up to 3 retries)
async fn execute_app_instance_function_with_gas<F>(
    app_instance_str: &str,
    function_name: &str,
    custom_gas_budget: Option<u64>,
    build_args: F,
) -> Result<String>
where
    F: Fn(
        &mut sui_transaction_builder::TransactionBuilder,
        Vec<sui_sdk_types::Argument>, // object_args
        sui_sdk_types::Argument,      // clock_arg
    ) -> Vec<sui_sdk_types::Argument>,
{
    let shared_state = SharedSuiState::get_instance();
    let package_id = shared_state.get_coordination_package_id_required();
    // Call the batch version with a single operation
    execute_transaction_block(
        package_id,
        vec![(
            vec![app_instance_str.to_string()],
            "app_instance".to_string(),
            function_name.to_string(),
            build_args,
        )],
        custom_gas_budget,
        None,
    )
    .await
}

/// Try to create a new block for the app instance
/// This function calls the try_create_block Move function on the blockchain
/// which will check if conditions are met to create a new block
pub async fn try_create_block_tx(app_instance_str: &str) -> Result<String> {
    execute_app_instance_function(
        app_instance_str,
        "try_create_block",
        |_tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            vec![app_instance_arg, clock_arg]
        },
    )
    .await
}

/// Purge old sequence states that have been settled
/// This function calls the purge Move function on the blockchain
/// to clean up sequence states up to the specified number of sequences
pub async fn purge_tx(
    app_instance_str: &str,
    sequences_to_purge: u64,
    gas_budget: Option<u64>,
) -> Result<String> {
    debug!(
        "Creating purge transaction for app_instance: {} with sequences_to_purge: {}",
        app_instance_str, sequences_to_purge
    );

    execute_app_instance_function_with_gas(
        app_instance_str,
        "purge",
        gas_budget,
        move |tb, object_args, clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences_to_purge));
            vec![app_instance_arg, sequences_arg, clock_arg]
        },
    )
    .await
}

/// Set a key-value pair in the app instance KV store
pub async fn set_kv_tx(app_instance_str: &str, key: String, value: String) -> Result<String> {
    debug!(
        "Creating set_kv transaction for key: {} with value length: {}",
        key,
        value.len()
    );

    execute_app_instance_function(
        app_instance_str,
        "set_kv",
        move |tb, object_args, _clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            // Use MoveString for proper serialization
            let key_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: key.clone().into_bytes(),
            }));
            let value_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: value.clone().into_bytes(),
            }));
            vec![app_instance_arg, key_arg, value_arg]
        },
    )
    .await
}

/// Delete a key-value pair from the app instance KV store
pub async fn delete_kv_tx(app_instance_str: &str, key: String) -> Result<String> {
    debug!("Creating delete_kv transaction for key: {}", key);

    execute_app_instance_function(
        app_instance_str,
        "delete_kv",
        move |tb, object_args, _clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            // Use MoveString for proper serialization
            let key_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: key.clone().into_bytes(),
            }));
            vec![app_instance_arg, key_arg]
        },
    )
    .await
}

/// Add metadata to the app instance (write-once)
pub async fn add_metadata_tx(app_instance_str: &str, key: String, value: String) -> Result<String> {
    debug!("Creating add_metadata transaction for key: {}", key);

    execute_app_instance_function(
        app_instance_str,
        "add_metadata",
        move |tb, object_args, _clock_arg| {
            let app_instance_arg = *object_args.get(0).expect("App instance argument required");
            // Use MoveString for proper serialization
            let key_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: key.clone().into_bytes(),
            }));
            let value_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: value.clone().into_bytes(),
            }));
            vec![app_instance_arg, key_arg, value_arg]
        },
    )
    .await
}
