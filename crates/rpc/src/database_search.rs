use crate::proto;
use crate::database::{EventDatabase, parse_json_array, SearchResult};
use anyhow::Result;
use sea_orm::{ConnectionTrait, Statement, QueryResult};
use std::time::Instant;
use tracing::debug;

/// Search all tables with fulltext indexes in parallel and return the most relevant events
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

    // Default limit if not specified
    let query_limit = limit.unwrap_or(10) as usize;

    // We'll query each table with a higher limit to ensure we get enough results
    let per_table_limit = query_limit * 2;

    // Prepare all SQL queries
    let queries = prepare_queries(&escaped_query, per_table_limit);

    // Execute all queries in parallel
    let conn = crate::database::get_connection(database);
    let results = tokio::join!(
        execute_query(conn, &queries.coordinator_message),
        execute_query(conn, &queries.agent_message),
        execute_query(conn, &queries.jobs),
        execute_query(conn, &queries.proof_event),
        execute_query(conn, &queries.coordination_tx),
        execute_query(conn, &queries.agent_session),
        execute_query(conn, &queries.settlement_tx),
        execute_query(conn, &queries.job_started),
        execute_query(conn, &queries.job_finished),
        execute_query(conn, &queries.settlement_included),
    );

    // Process results and collect all events
    let mut all_events: Vec<proto::EventWithRelevance> = Vec::new();

    // Process coordinator_message_event results
    if let Ok(rows) = results.0 {
        for row in rows {
            if let Some(event) = parse_coordinator_message_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process agent_message_event results
    if let Ok(rows) = results.1 {
        for row in rows {
            if let Some(event) = parse_agent_message_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process jobs results
    if let Ok(rows) = results.2 {
        for row in rows {
            if let Some(event) = parse_job_created_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process proof_event results
    if let Ok(rows) = results.3 {
        for row in rows {
            if let Some(event) = parse_proof_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process coordination_tx_event results
    if let Ok(rows) = results.4 {
        for row in rows {
            if let Some(event) = parse_coordination_tx_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process agent_session results
    if let Ok(rows) = results.5 {
        for row in rows {
            if let Some(event) = parse_agent_session_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process settlement_transaction_event results
    if let Ok(rows) = results.6 {
        for row in rows {
            if let Some(event) = parse_settlement_tx_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process job_started_event results
    if let Ok(rows) = results.7 {
        for row in rows {
            if let Some(event) = parse_job_started_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process job_finished_event results
    if let Ok(rows) = results.8 {
        for row in rows {
            if let Some(event) = parse_job_finished_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Process settlement_transaction_included_in_block_event results
    if let Ok(rows) = results.9 {
        for row in rows {
            if let Some(event) = parse_settlement_included_event(&row) {
                all_events.push(event);
            }
        }
    }

    // Sort all events by relevance score (descending)
    all_events.sort_by(|a, b| {
        b.relevance_score.partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Take only the requested limit
    all_events.truncate(query_limit);

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

struct SearchQueries {
    coordinator_message: String,
    agent_message: String,
    jobs: String,
    proof_event: String,
    coordination_tx: String,
    agent_session: String,
    settlement_tx: String,
    job_started: String,
    job_finished: String,
    settlement_included: String,
}

fn prepare_queries(escaped_query: &str, limit: usize) -> SearchQueries {
    SearchQueries {
        coordinator_message: format!(
            "SELECT id, coordinator_id, event_timestamp, level, message,
             fts_match_bm25('{}', message) as relevance_score
             FROM coordinator_message_event
             WHERE fts_match_word('{}', message)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query, limit
        ),

        agent_message: format!(
            "SELECT id, coordinator_id, session_id, job_id, event_timestamp, level, message,
             fts_match_bm25('{}', message) as relevance_score
             FROM agent_message_event
             WHERE fts_match_word('{}', message)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query, limit
        ),

        jobs: format!(
            "SELECT job_id, coordinator_id, session_id, app_instance_id, app_method,
             job_sequence, sequences, merged_sequences_1, merged_sequences_2, event_timestamp,
             GREATEST(
                 IF(fts_match_word('{}', coordinator_id), fts_match_bm25('{}', coordinator_id), 0),
                 IF(fts_match_word('{}', session_id), fts_match_bm25('{}', session_id), 0),
                 IF(fts_match_word('{}', app_instance_id), fts_match_bm25('{}', app_instance_id), 0),
                 IF(fts_match_word('{}', app_method), fts_match_bm25('{}', app_method), 0),
                 IF(fts_match_word('{}', job_id), fts_match_bm25('{}', job_id), 0)
             ) as relevance_score
             FROM jobs
             WHERE fts_match_word('{}', coordinator_id)
                OR fts_match_word('{}', session_id)
                OR fts_match_word('{}', app_instance_id)
                OR fts_match_word('{}', app_method)
                OR fts_match_word('{}', job_id)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            limit
        ),

        proof_event: format!(
            "SELECT id, coordinator_id, session_id, app_instance_id, job_id,
             data_availability, block_number, block_proof, proof_event_type,
             sequences, merged_sequences_1, merged_sequences_2, event_timestamp,
             GREATEST(
                 IF(fts_match_word('{}', coordinator_id), fts_match_bm25('{}', coordinator_id), 0),
                 IF(fts_match_word('{}', session_id), fts_match_bm25('{}', session_id), 0),
                 IF(fts_match_word('{}', app_instance_id), fts_match_bm25('{}', app_instance_id), 0),
                 IF(fts_match_word('{}', job_id), fts_match_bm25('{}', job_id), 0)
             ) as relevance_score
             FROM proof_event
             WHERE fts_match_word('{}', coordinator_id)
                OR fts_match_word('{}', session_id)
                OR fts_match_word('{}', app_instance_id)
                OR fts_match_word('{}', job_id)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            limit
        ),

        coordination_tx: format!(
            "SELECT id, coordinator_id, tx_hash, event_timestamp,
             GREATEST(
                 IF(fts_match_word('{}', coordinator_id), fts_match_bm25('{}', coordinator_id), 0),
                 IF(fts_match_word('{}', tx_hash), fts_match_bm25('{}', tx_hash), 0)
             ) as relevance_score
             FROM coordination_tx_event
             WHERE fts_match_word('{}', coordinator_id)
                OR fts_match_word('{}', tx_hash)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query,
            escaped_query,
            limit
        ),

        agent_session: format!(
            "SELECT id, coordinator_id, developer, agent, agent_method, session_id, event_timestamp,
             GREATEST(
                 IF(fts_match_word('{}', coordinator_id), fts_match_bm25('{}', coordinator_id), 0),
                 IF(fts_match_word('{}', developer), fts_match_bm25('{}', developer), 0),
                 IF(fts_match_word('{}', agent), fts_match_bm25('{}', agent), 0),
                 IF(fts_match_word('{}', agent_method), fts_match_bm25('{}', agent_method), 0),
                 IF(fts_match_word('{}', session_id), fts_match_bm25('{}', session_id), 0)
             ) as relevance_score
             FROM agent_session
             WHERE fts_match_word('{}', coordinator_id)
                OR fts_match_word('{}', developer)
                OR fts_match_word('{}', agent)
                OR fts_match_word('{}', agent_method)
                OR fts_match_word('{}', session_id)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            limit
        ),

        settlement_tx: format!(
            "SELECT id, coordinator_id, session_id, app_instance_id, chain, job_id,
             block_number, tx_hash, event_timestamp,
             GREATEST(
                 IF(fts_match_word('{}', coordinator_id), fts_match_bm25('{}', coordinator_id), 0),
                 IF(fts_match_word('{}', session_id), fts_match_bm25('{}', session_id), 0),
                 IF(fts_match_word('{}', app_instance_id), fts_match_bm25('{}', app_instance_id), 0),
                 IF(fts_match_word('{}', job_id), fts_match_bm25('{}', job_id), 0),
                 IF(fts_match_word('{}', tx_hash), fts_match_bm25('{}', tx_hash), 0)
             ) as relevance_score
             FROM settlement_transaction_event
             WHERE fts_match_word('{}', coordinator_id)
                OR fts_match_word('{}', session_id)
                OR fts_match_word('{}', app_instance_id)
                OR fts_match_word('{}', job_id)
                OR fts_match_word('{}', tx_hash)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            limit
        ),

        job_started: format!(
            "SELECT id, coordinator_id, session_id, app_instance_id, job_id, event_timestamp,
             GREATEST(
                 IF(fts_match_word('{}', coordinator_id), fts_match_bm25('{}', coordinator_id), 0),
                 IF(fts_match_word('{}', session_id), fts_match_bm25('{}', session_id), 0),
                 IF(fts_match_word('{}', app_instance_id), fts_match_bm25('{}', app_instance_id), 0),
                 IF(fts_match_word('{}', job_id), fts_match_bm25('{}', job_id), 0)
             ) as relevance_score
             FROM job_started_event
             WHERE fts_match_word('{}', coordinator_id)
                OR fts_match_word('{}', session_id)
                OR fts_match_word('{}', app_instance_id)
                OR fts_match_word('{}', job_id)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            limit
        ),

        job_finished: format!(
            "SELECT job_id, coordinator_id, duration, cost, event_timestamp, result,
             GREATEST(
                 IF(fts_match_word('{}', coordinator_id), fts_match_bm25('{}', coordinator_id), 0),
                 IF(fts_match_word('{}', job_id), fts_match_bm25('{}', job_id), 0)
             ) as relevance_score
             FROM job_finished_event
             WHERE fts_match_word('{}', coordinator_id)
                OR fts_match_word('{}', job_id)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query,
            escaped_query,
            limit
        ),

        settlement_included: format!(
            "SELECT id, coordinator_id, session_id, app_instance_id, chain, job_id,
             block_number, event_timestamp,
             GREATEST(
                 IF(fts_match_word('{}', coordinator_id), fts_match_bm25('{}', coordinator_id), 0),
                 IF(fts_match_word('{}', session_id), fts_match_bm25('{}', session_id), 0),
                 IF(fts_match_word('{}', app_instance_id), fts_match_bm25('{}', app_instance_id), 0),
                 IF(fts_match_word('{}', job_id), fts_match_bm25('{}', job_id), 0)
             ) as relevance_score
             FROM settlement_transaction_included_in_block_event
             WHERE fts_match_word('{}', coordinator_id)
                OR fts_match_word('{}', session_id)
                OR fts_match_word('{}', app_instance_id)
                OR fts_match_word('{}', job_id)
             ORDER BY relevance_score DESC
             LIMIT {}",
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query, escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            escaped_query,
            limit
        ),
    }
}

async fn execute_query(
    conn: &sea_orm::DatabaseConnection,
    query: &str,
) -> Result<Vec<QueryResult>> {
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::MySql,
        query,
        vec![],
    );
    conn.query_all(stmt).await.map_err(|e| anyhow::anyhow!("Query failed: {}", e))
}

// Parsing functions for each event type
fn parse_coordinator_message_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let level = row.try_get::<i8>("", "level").ok()? as i32;
    let message = row.try_get::<String>("", "message").ok()?;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

    let event = proto::Event {
        event: Some(proto::event::Event::CoordinatorMessage(
            proto::CoordinatorMessageEvent {
                coordinator_id,
                event_timestamp,
                level,
                message,
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_agent_message_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let job_id = row.try_get::<Option<String>>("", "job_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let level = row.try_get::<i8>("", "level").ok()? as i32;
    let message = row.try_get::<String>("", "message").ok()?;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

    let event = proto::Event {
        event: Some(proto::event::Event::AgentMessage(
            proto::AgentMessageEvent {
                coordinator_id,
                session_id,
                job_id,
                event_timestamp,
                level,
                message,
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_job_created_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let app_method = row.try_get::<String>("", "app_method").ok()?;
    let job_sequence = row.try_get::<i64>("", "job_sequence").ok()? as u64;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

    // Parse JSON arrays
    let sequences = row.try_get::<Option<String>>("", "sequences")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();
    let merged_sequences_1 = row.try_get::<Option<String>>("", "merged_sequences_1")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();
    let merged_sequences_2 = row.try_get::<Option<String>>("", "merged_sequences_2")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();

    let event = proto::Event {
        event: Some(proto::event::Event::JobCreated(
            proto::JobCreatedEvent {
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
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id: -1, // Jobs table doesn't have an id column
        event: Some(event),
        relevance_score,
    })
}

fn parse_proof_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let data_availability = row.try_get::<String>("", "data_availability").ok()?;
    let block_number = row.try_get::<i64>("", "block_number").ok()? as u64;
    let block_proof = row.try_get::<Option<bool>>("", "block_proof").ok()?;
    let proof_event_type_str = row.try_get::<String>("", "proof_event_type").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

    // Parse JSON arrays
    let sequences = row.try_get::<Option<String>>("", "sequences")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();
    let merged_sequences_1 = row.try_get::<Option<String>>("", "merged_sequences_1")
        .ok()
        .flatten()
        .and_then(|s| parse_json_array(&s).ok())
        .unwrap_or_default();
    let merged_sequences_2 = row.try_get::<Option<String>>("", "merged_sequences_2")
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
        event: Some(proto::event::Event::ProofEvent(
            proto::ProofEvent {
                coordinator_id,
                session_id,
                app_instance_id,
                job_id,
                data_availability,
                block_number,
                block_proof,
                proof_event_type,
                sequences,
                merged_sequences_1,
                merged_sequences_2,
                event_timestamp,
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_coordination_tx_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let tx_hash = row.try_get::<String>("", "tx_hash").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

    let event = proto::Event {
        event: Some(proto::event::Event::CoordinationTx(
            proto::CoordinationTxEvent {
                coordinator_id,
                tx_hash,
                event_timestamp,
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_agent_session_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let developer = row.try_get::<String>("", "developer").ok()?;
    let agent = row.try_get::<String>("", "agent").ok()?;
    let agent_method = row.try_get::<String>("", "agent_method").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

    let event = proto::Event {
        event: Some(proto::event::Event::AgentSessionStarted(
            proto::AgentSessionStartedEvent {
                coordinator_id,
                developer,
                agent,
                agent_method,
                session_id,
                event_timestamp,
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_settlement_tx_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let chain = row.try_get::<String>("", "chain").ok()?;
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let block_number = row.try_get::<i64>("", "block_number").ok()? as u64;
    let tx_hash = row.try_get::<String>("", "tx_hash").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

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
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_job_started_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

    let event = proto::Event {
        event: Some(proto::event::Event::JobStarted(
            proto::JobStartedEvent {
                coordinator_id,
                session_id,
                app_instance_id,
                job_id,
                event_timestamp,
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}

fn parse_job_finished_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let duration = row.try_get::<i64>("", "duration").ok()? as u64;
    let cost = row.try_get::<i64>("", "cost").ok()? as u64;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let result_str = row.try_get::<String>("", "result").ok()?;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

    // Convert result string to enum
    let result = match result_str.as_str() {
        "JOB_RESULT_COMPLETED" => proto::JobResult::Completed as i32,
        "JOB_RESULT_FAILED" => proto::JobResult::Failed as i32,
        "JOB_RESULT_TERMINATED" => proto::JobResult::Terminated as i32,
        _ => proto::JobResult::Unspecified as i32,
    };

    let event = proto::Event {
        event: Some(proto::event::Event::JobFinished(
            proto::JobFinishedEvent {
                coordinator_id,
                job_id,
                duration,
                cost,
                event_timestamp,
                result,
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id: -1, // job_finished_event uses job_id as primary key
        event: Some(event),
        relevance_score,
    })
}

fn parse_settlement_included_event(row: &QueryResult) -> Option<proto::EventWithRelevance> {
    let id = row.try_get::<i64>("", "id").ok()?;
    let coordinator_id = row.try_get::<String>("", "coordinator_id").ok()?;
    let session_id = row.try_get::<String>("", "session_id").ok()?;
    let app_instance_id = row.try_get::<String>("", "app_instance_id").ok()?;
    let chain = row.try_get::<String>("", "chain").ok()?;
    let job_id = row.try_get::<String>("", "job_id").ok()?;
    let block_number = row.try_get::<i64>("", "block_number").ok()? as u64;
    let event_timestamp = row.try_get::<i64>("", "event_timestamp").ok()? as u64;
    let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

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
            }
        )),
    };

    Some(proto::EventWithRelevance {
        id,
        event: Some(event),
        relevance_score,
    })
}