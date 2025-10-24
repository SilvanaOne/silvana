use crate::database::{EventDatabase, SearchResult, parse_json_array};
use crate::fulltext_indexes::FULLTEXT_INDEXES;
use crate::proto;
use anyhow::Result;
use futures::future::join_all;
use sea_orm::{ConnectionTrait, QueryResult, Statement};
use std::collections::HashSet;
use std::time::Instant;
use tracing::debug;

/// Determine if a field is an ID field (hex string) or a text field
fn is_id_field(column_name: &str) -> bool {
    matches!(
        column_name,
        "coordinator_id" | "app_instance_id" | "job_id" | "session_id" | "tx_hash"
    )
}

/// Search all fulltext indexes in parallel, using appropriate search method for each field type
/// - ID fields (hex strings) use LIKE queries
/// - Text fields use fts_match_word for fulltext search
pub async fn search_all_events_parallel(
    database: &EventDatabase,
    search_query: &str,
    limit: Option<u32>,
) -> Result<SearchResult> {
    let start_time = Instant::now();

    if search_query.trim().is_empty() {
        return Ok(SearchResult {
            events: vec![],
            total_count: 0,
            returned_count: 0,
        });
    }

    // Escape single quotes in search query to prevent SQL injection
    let escaped_query = search_query.replace("'", "''");
    let query_limit = limit.unwrap_or(10) as usize;

    // Create a query for each fulltext index
    let mut queries = Vec::new();

    for index in FULLTEXT_INDEXES.iter() {
        let primary_key = get_primary_key_column(index.table_name);

        // Use different query strategy based on field type
        let query = if is_id_field(index.column_name) {
            // For ID fields (hex strings), use LIKE query
            format!(
                "SELECT {} as pk, '{}' as table_name, '{}' as matched_column, \
                 1.0 as relevance_score \
                 FROM {} \
                 WHERE {} LIKE '%{}%' \
                 ORDER BY relevance_score DESC \
                 LIMIT {}",
                primary_key,
                index.table_name,
                index.column_name,
                index.table_name,
                index.column_name,
                escaped_query,
                query_limit * 2 // Get more results to ensure we have enough after deduplication
            )
        } else {
            // For text fields, use fts_match_word for proper fulltext search
            // Note: fts_match_word can only be used once per query in TiDB
            format!(
                "SELECT {} as pk, '{}' as table_name, '{}' as matched_column, \
                 fts_match_word('{}', {}) as relevance_score \
                 FROM {} \
                 WHERE fts_match_word('{}', {}) \
                 ORDER BY relevance_score DESC \
                 LIMIT {}",
                primary_key,
                index.table_name,
                index.column_name,
                escaped_query,
                index.column_name,
                index.table_name,
                escaped_query,
                index.column_name,
                query_limit * 2
            )
        };

        queries.push((index.table_name, index.column_name, query));
    }

    // Execute all queries in parallel
    let conn = crate::database::get_connection(database);
    let mut futures = Vec::new();

    for (_table, _column, query) in queries {
        let conn_clone = conn.clone();
        let query_clone = query.clone();
        futures.push(async move { execute_search_query(&conn_clone, &query_clone).await });
    }

    let results = join_all(futures).await;

    // Collect all matching records with their scores
    #[derive(Debug)]
    struct SearchMatch {
        table_name: String,
        primary_key: String,
        relevance_score: f64,
    }

    let mut matches: Vec<SearchMatch> = Vec::new();

    for result in results {
        if let Ok(rows) = result {
            for row in rows {
                // Try to get values by column index instead of name
                match row.try_get_by_index::<String>(0) {
                    Ok(pk) => {
                        let table_name = row.try_get_by_index::<String>(1).unwrap_or_default();
                        let _matched_column = row.try_get_by_index::<String>(2).unwrap_or_default();
                        // Relevance score might be returned as integer (1) for LIKE queries
                        let relevance_score = row
                            .try_get_by_index::<f64>(3)
                            .or_else(|_| row.try_get_by_index::<i64>(3).map(|v| v as f64))
                            .or_else(|_| row.try_get_by_index::<f32>(3).map(|v| v as f64))
                            .unwrap_or(1.0); // Default to 1.0 for ID field matches

                        matches.push(SearchMatch {
                            table_name,
                            primary_key: pk,
                            relevance_score,
                        });
                    }
                    Err(e) => {
                        // Try to get as i64 for numeric primary keys
                        if let Ok(pk_num) = row.try_get_by_index::<i64>(0) {
                            let table_name = row.try_get_by_index::<String>(1).unwrap_or_default();
                            let _matched_column =
                                row.try_get_by_index::<String>(2).unwrap_or_default();
                            // Relevance score might be returned as integer (1) for LIKE queries
                            let relevance_score = row
                                .try_get_by_index::<f64>(3)
                                .or_else(|_| row.try_get_by_index::<i64>(3).map(|v| v as f64))
                                .or_else(|_| row.try_get_by_index::<f32>(3).map(|v| v as f64))
                                .unwrap_or(1.0); // Default to 1.0 for ID field matches

                            matches.push(SearchMatch {
                                table_name,
                                primary_key: pk_num.to_string(),
                                relevance_score,
                            });
                        } else {
                            debug!("Failed to get pk from row: {:?}", e);
                        }
                    }
                }
            }
        } else if let Err(e) = result {
            debug!("Query failed: {:?}", e);
        }
    }

    // Sort by relevance score (for text fields, this is BM25 score; for ID fields, it's 1.0)
    matches.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Deduplicate by (table, primary_key) keeping highest score
    let mut seen = HashSet::new();
    let mut unique_matches = Vec::new();

    for match_ in matches {
        let key = format!("{}:{}", match_.table_name, match_.primary_key);
        if !seen.contains(&key) {
            seen.insert(key);
            unique_matches.push(match_);
        }
    }

    // Take only requested limit
    unique_matches.truncate(query_limit);

    // Now fetch the full records for each match
    let mut all_events: Vec<proto::EventWithRelevance> = Vec::new();

    for match_ in unique_matches {
        match fetch_full_event(
            conn,
            &match_.table_name,
            &match_.primary_key,
            match_.relevance_score,
        )
        .await
        {
            Ok(Some(event)) => {
                all_events.push(event);
            }
            Ok(None) => {
                debug!(
                    "No event found for table={}, pk={}",
                    match_.table_name, match_.primary_key
                );
            }
            Err(e) => {
                debug!(
                    "Failed to fetch event from table={}, pk={}: {:?}",
                    match_.table_name, match_.primary_key, e
                );
                return Err(e);
            }
        }
    }

    let total_count = all_events.len();
    let returned_count = all_events.len();

    debug!(
        "Parallel search completed in {}ms: query='{}', found={} events",
        start_time.elapsed().as_millis(),
        search_query,
        returned_count
    );

    Ok(SearchResult {
        events: all_events,
        total_count,
        returned_count,
    })
}

/// Get the primary key column name for a table
fn get_primary_key_column(table_name: &str) -> &'static str {
    match table_name {
        "jobs" => "job_id",
        "job_started_event" => "job_id",
        "job_finished_event" => "job_id",
        "agent_session" => "session_id",
        "agent_session_finished_event" => "session_id",
        _ => "id",
    }
}

/// Execute a search query and return results
async fn execute_search_query(
    conn: &sea_orm::DatabaseConnection,
    query: &str,
) -> Result<Vec<QueryResult>> {
    let stmt = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query, vec![]);
    conn.query_all(stmt)
        .await
        .map_err(|e| anyhow::anyhow!("Query failed: {}", e))
}

/// Fetch a full event record by table name and primary key
async fn fetch_full_event(
    conn: &sea_orm::DatabaseConnection,
    table_name: &str,
    primary_key: &str,
    relevance_score: f64,
) -> Result<Option<proto::EventWithRelevance>> {
    let pk_column = get_primary_key_column(table_name);

    // Check if primary key is numeric (for id columns) or string (for job_id, session_id)
    let query = if pk_column == "id" {
        // Numeric primary key - don't quote
        format!(
            "SELECT * FROM {} WHERE {} = {}",
            table_name, pk_column, primary_key
        )
    } else {
        // String primary key - quote it
        format!(
            "SELECT * FROM {} WHERE {} = '{}'",
            table_name, pk_column, primary_key
        )
    };

    let stmt = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, &query, vec![]);

    let result = conn.query_one(stmt).await?;

    if let Some(row) = result {
        // Parse based on table name
        match table_name {
            "coordinator_message_event" => {
                Ok(parse_coordinator_message_event(&row, relevance_score))
            }
            "agent_message_event" => Ok(parse_agent_message_event(&row, relevance_score)),
            "jobs" => Ok(parse_job_created_event(&row, relevance_score)),
            "proof_event" => Ok(parse_proof_event(&row, relevance_score)),
            "coordination_tx_event" => Ok(parse_coordination_tx_event(&row, relevance_score)),
            "agent_session" => Ok(parse_agent_session_event(&row, relevance_score)),
            "agent_session_finished_event" => {
                Ok(parse_agent_session_finished_event(&row, relevance_score))
            }
            "settlement_transaction_event" => Ok(parse_settlement_tx_event(&row, relevance_score)),
            "job_started_event" => Ok(parse_job_started_event(&row, relevance_score)),
            "job_finished_event" => Ok(parse_job_finished_event(&row, relevance_score)),
            "settlement_transaction_included_in_block_event" => {
                Ok(parse_settlement_included_event(&row, relevance_score))
            }
            "coordinator_started_event" => {
                Ok(parse_coordinator_started_event(&row, relevance_score))
            }
            "coordinator_active_event" => Ok(parse_coordinator_active_event(&row, relevance_score)),
            "coordinator_shutdown_event" => {
                Ok(parse_coordinator_shutdown_event(&row, relevance_score))
            }
            _ => Ok(None),
        }
    } else {
        Ok(None)
    }
}

// Parsing functions for each event type
fn parse_coordinator_message_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let level = row.try_get::<i8>("", "level").ok()? as i32;
    let message = row.try_get::<String>("", "message").ok()?;

    let event = proto::Event {
        event: Some(proto::event::Event::CoordinatorMessage(
            proto::CoordinatorMessageEvent {
                coordinator_id,
                event_timestamp,
                level,
                message,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_agent_message_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    // The columns from the SELECT * are in table order:
    // id, coordinator_id, session_id, job_id, event_timestamp, level, message, created_at, updated_at
    let id = row.try_get_by_index::<i64>(0).ok()?;
    let coordinator_id = row.try_get_by_index::<String>(1).ok()?;
    let session_id = row.try_get_by_index::<String>(2).ok()?;
    let job_id = row.try_get_by_index::<Option<String>>(3).ok()?;
    let event_timestamp = row.try_get_by_index::<i64>(4).ok()? as u64;
    let level_str = row.try_get_by_index::<String>(5).ok()?;
    let message = row.try_get_by_index::<String>(6).ok()?;

    // Convert level string enum to i32
    let level = match level_str.as_str() {
        "LOG_LEVEL_UNSPECIFIED" => 0,
        "LOG_LEVEL_DEBUG" => 1,
        "LOG_LEVEL_INFO" => 2,
        "LOG_LEVEL_WARN" => 3,
        "LOG_LEVEL_ERROR" => 4,
        "LOG_LEVEL_FATAL" => 5,
        _ => 2, // Default to INFO
    } as i32;

    let event = proto::Event {
        event: Some(proto::event::Event::AgentMessage(
            proto::AgentMessageEvent {
                coordinator_id,
                session_id,
                job_id,
                event_timestamp,
                level,
                message,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_job_created_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let app_method = row.try_get::<String>("", "app_method").ok()?;
    let job_sequence = row.try_get::<i64>("", "job_sequence").ok()? as u64;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    // Parse JSON arrays
    let sequences = row
        .try_get::<Option<String>>("", "sequences")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();
    let merged_sequences_1 = row
        .try_get::<Option<String>>("", "merged_sequences_1")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();
    let merged_sequences_2 = row
        .try_get::<Option<String>>("", "merged_sequences_2")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();

    let event = proto::Event {
        event: Some(proto::event::Event::JobCreated(proto::JobCreatedEvent {
            coordinator_id,
            session_id,
            app_instance_id,
            app_method,
            job_sequence,
            sequences,
            merged_sequences_1,
            merged_sequences_2,
            job_id,
            event_timestamp,
        })),
    };

    Some(proto::EventWithRelevance {
        id: -1, // Jobs table doesn't have an id column
        event: Some(event),
        relevance_score,
    })
}

fn parse_proof_event(row: &QueryResult, relevance_score: f64) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let data_availability = row.try_get::<String>("", "data_availability").ok()?;
    let block_number = row.try_get::<i64>("", "block_number").ok()? as u64;
    let block_proof = row.try_get::<Option<bool>>("", "block_proof").ok()?;
    let settlement_proof = row.try_get::<Option<bool>>("", "settlement_proof").ok()?;
    let settlement_proof_chain = row.try_get::<Option<String>>("", "settlement_proof_chain").ok()?;
    let proof_event_type_str = row.try_get::<String>("", "proof_event_type").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    // Parse JSON arrays
    let sequences = row
        .try_get::<Option<String>>("", "sequences")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();
    let merged_sequences_1 = row
        .try_get::<Option<String>>("", "merged_sequences_1")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();
    let merged_sequences_2 = row
        .try_get::<Option<String>>("", "merged_sequences_2")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();

    // Convert proof event type string to enum
    let proof_event_type = match proof_event_type_str.as_str() {
        "PROOF_SUBMITTED" => proto::ProofEventType::ProofSubmitted as i32,
        "PROOF_FETCHED" => proto::ProofEventType::ProofFetched as i32,
        "PROOF_VERIFIED" => proto::ProofEventType::ProofVerified as i32,
        "PROOF_UNAVAILABLE" => proto::ProofEventType::ProofUnavailable as i32,
        "PROOF_REJECTED" => proto::ProofEventType::ProofRejected as i32,
        _ => proto::ProofEventType::Unspecified as i32,
    };

    let event = proto::Event {
        event: Some(proto::event::Event::ProofEvent(proto::ProofEvent {
            coordinator_id,
            session_id,
            app_instance_id,
            job_id,
            data_availability,
            block_number,
            block_proof,
            settlement_proof,
            settlement_proof_chain,
            proof_event_type,
            sequences,
            merged_sequences_1,
            merged_sequences_2,
            event_timestamp,
        })),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_coordination_tx_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let tx_hash = row.try_get::<String>("", "tx_hash").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::CoordinationTx(
            proto::CoordinationTxEvent {
                coordinator_id,
                tx_hash,
                event_timestamp,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_agent_session_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let developer = row.try_get::<String>("", "developer").ok()?;
    let agent = row.try_get::<String>("", "agent").ok()?;
    let agent_method = row.try_get::<String>("", "agent_method").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::AgentSessionStarted(
            proto::AgentSessionStartedEvent {
                coordinator_id,
                developer,
                agent,
                agent_method,
                session_id,
                event_timestamp,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_settlement_tx_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let chain = row.try_get::<String>("", "chain").ok()?;
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let block_number = row.try_get::<i64>("", "block_number").ok()? as u64;
    let tx_hash = row.try_get::<String>("", "tx_hash").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::SettlementTransaction(
            proto::SettlementTransactionEvent {
                coordinator_id,
                session_id,
                app_instance_id,
                chain,
                job_id,
                block_number,
                tx_hash,
                event_timestamp,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_job_started_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::JobStarted(proto::JobStartedEvent {
            coordinator_id,
            session_id,
            app_instance_id,
            job_id,
            event_timestamp,
        })),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_job_finished_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let duration = row.try_get::<i64>("", "duration").ok()? as u64;
    let cost = row.try_get::<i64>("", "cost").ok()? as u64;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let result_str = row.try_get::<String>("", "result").ok()?;

    // Convert result string to enum
    let result = match result_str.as_str() {
        "JOB_RESULT_COMPLETED" => proto::JobResult::Completed as i32,
        "JOB_RESULT_FAILED" => proto::JobResult::Failed as i32,
        "JOB_RESULT_TERMINATED" => proto::JobResult::Terminated as i32,
        _ => proto::JobResult::Unspecified as i32,
    };

    let event = proto::Event {
        event: Some(proto::event::Event::JobFinished(proto::JobFinishedEvent {
            coordinator_id,
            job_id,
            duration,
            cost,
            event_timestamp,
            result,
        })),
    };

    Some(proto::EventWithRelevance {
        id: -1, // job_finished_event uses job_id as primary key
        event: Some(event),
        relevance_score,
    })
}

fn parse_settlement_included_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let chain = row.try_get::<String>("", "chain").ok()?;
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let block_number = row.try_get::<i64>("", "block_number").ok()? as u64;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::SettlementTransactionIncluded(
            proto::SettlementTransactionIncludedInBlockEvent {
                coordinator_id,
                session_id,
                app_instance_id,
                chain,
                job_id,
                block_number,
                event_timestamp,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_coordinator_started_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let ethereum_address = row.try_get::<String>("", "ethereum_address").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::CoordinatorStarted(
            proto::CoordinatorStartedEvent {
                coordinator_id,
                ethereum_address,
                event_timestamp,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_coordinator_active_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::CoordinatorActive(
            proto::CoordinatorActiveEvent {
                coordinator_id,
                event_timestamp,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_coordinator_shutdown_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::CoordinatorShutdown(
            proto::CoordinatorShutdownEvent {
                coordinator_id,
                event_timestamp,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_agent_session_finished_event(
    row: &QueryResult,
    relevance_score: f64,
) -> Option<proto::EventWithRelevance> {
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let session_log = row.try_get::<String>("", "session_log").ok()?;
    let duration = row.try_get::<i64>("", "duration").ok()? as u64;
    let cost = row.try_get::<i64>("", "cost").ok()? as u64;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;

    let event = proto::Event {
        event: Some(proto::event::Event::AgentSessionFinished(
            proto::AgentSessionFinishedEvent {
                coordinator_id,
                session_id,
                session_log,
                duration,
                cost,
                event_timestamp,
            },
        )),
    };

    Some(proto::EventWithRelevance {
        id: -1, // agent_session_finished_event uses session_id as primary key
        event: Some(event),
        relevance_score,
    })
}
