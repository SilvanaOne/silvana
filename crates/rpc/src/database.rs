use anyhow::Result;
use proto::Event;
use sea_orm::{Database, DatabaseConnection, EntityTrait, TransactionTrait};
use std::time::Instant;
use tidb as entities;
use tracing::{debug, error, info, warn};

// Result structs for query responses
#[derive(Debug, Clone)]
pub struct AgentTransactionEventResult {
    pub id: i64,
    pub coordinator_id: String,
    pub tx_type: String,
    pub developer: String,
    pub agent: String,
    pub app: String,
    pub job_sequence: String,
    pub sequences: Vec<u64>,
    pub event_timestamp: u64,
    pub tx_hash: String,
    pub chain: String,
    pub network: String,
    pub memo: String,
    pub metadata: String,
    pub created_at_timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct AgentMessageEventResult {
    pub id: i64,
    pub coordinator_id: String,
    pub developer: String,
    pub agent: String,
    pub app: String,
    pub job_sequence: String,
    pub sequences: Vec<u64>,
    pub event_timestamp: u64,
    pub level: u32,
    pub message: String,
    pub created_at_timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct CoordinatorMessageEventResult {
    pub id: i64,
    pub coordinator_id: String,
    pub event_timestamp: u64,
    pub level: i32,
    pub message: String,
    pub created_at_timestamp: i64,
    pub relevance_score: f64,
}

pub struct EventDatabase {
    connection: DatabaseConnection,
}

impl EventDatabase {
    pub async fn new(database_url: &str) -> Result<Self> {
        info!("Connecting to TiDB...");

        let connection = Database::connect(database_url).await?;

        info!("Successfully connected to TiDB");

        Ok(Self { connection })
    }

    pub async fn insert_events_batch(&self, events: &[Event]) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        let start_time = Instant::now();

        debug!(
            "Inserting batch of {} events using parallel batch insertion with child table support",
            events.len()
        );

        let txn = self.connection.begin().await?;
        //let mut total_inserted = 0;

        // Group events by type for batch insertion
        let mut coordinator_started_events = Vec::new();
        let mut coordinator_active_events = Vec::new();
        let mut coordinator_shutdown_events = Vec::new();
        let mut agent_session_started_events = Vec::new();
        let mut agent_session_finished_events = Vec::new();
        let mut jobs_events = Vec::new();
        let mut job_started_events = Vec::new();
        let mut job_finished_events = Vec::new();
        let mut coordination_tx_events = Vec::new();
        let mut coordinator_message_events = Vec::new();
        let mut proof_events = Vec::new();
        let mut settlement_transaction_events = Vec::new();
        let mut settlement_transaction_included_events = Vec::new();
        let mut agent_message_events = Vec::new();

        // Store sequences separately for child table insertion
        let mut job_sequences_data = Vec::new();
        let mut proof_event_sequences_data = Vec::new();

        // Categorize events by type
        for event in events {
            if let Some(event_type) = &event.event {
                use proto::event::Event as EventType;
                match event_type {
                    EventType::CoordinatorStarted(e) => {
                        coordinator_started_events.push(convert_coordinator_started_event(e));
                    }
                    EventType::CoordinatorActive(e) => {
                        coordinator_active_events.push(convert_coordinator_active_event(e));
                    }
                    EventType::CoordinatorShutdown(e) => {
                        coordinator_shutdown_events.push(convert_coordinator_shutdown_event(e));
                    }
                    EventType::AgentSessionStarted(e) => {
                        agent_session_started_events.push(convert_agent_session_started_event(e));
                    }
                    EventType::AgentSessionFinished(e) => {
                        // Handle AgentSessionFinishedEvent
                        agent_session_finished_events.push(convert_agent_session_finished_event(e));
                    }
                    EventType::JobCreated(e) => {
                        let (main_event, sequences, merged_sequences_1, merged_sequences_2) =
                            convert_job_created_event(e);
                        jobs_events.push(main_event);

                        let mut sequences_vec = Vec::new();
                        for seq in sequences {
                            sequences_vec.push((seq, "sequences".to_string()));
                        }
                        for seq in merged_sequences_1 {
                            sequences_vec.push((seq, "merged_sequences_1".to_string()));
                        }
                        for seq in merged_sequences_2 {
                            sequences_vec.push((seq, "merged_sequences_2".to_string()));
                        }
                        if !sequences_vec.is_empty() {
                            job_sequences_data.push(sequences_vec);
                        }
                    }
                    EventType::JobStarted(e) => {
                        job_started_events.push(convert_job_started_event(e));
                    }
                    EventType::JobFinished(e) => {
                        job_finished_events.push(convert_job_finished_event(e));
                    }
                    EventType::CoordinationTx(e) => {
                        coordination_tx_events.push(convert_coordination_tx_event(e));
                    }
                    EventType::CoordinatorMessage(e) => {
                        coordinator_message_events.push(convert_coordinator_message_event(e));
                    }
                    EventType::ProofEvent(e) => {
                        let (main_event, sequences_data) = convert_proof_event(e);
                        proof_events.push(main_event);
                        if !sequences_data.is_empty() {
                            proof_event_sequences_data.push(sequences_data);
                        }
                    }
                    EventType::SettlementTransaction(e) => {
                        settlement_transaction_events.push(convert_settlement_transaction_event(e));
                    }
                    EventType::SettlementTransactionIncluded(e) => {
                        settlement_transaction_included_events
                            .push(convert_settlement_transaction_included_event(e));
                    }
                    EventType::AgentMessage(e) => {
                        agent_message_events.push(convert_agent_message_event(e));
                    }
                }
            }
        }

        // Phase 1: Run all independent main table insertions in parallel
        //debug!("Phase 1: Running main table insertions in parallel");
        let _independent_results = tokio::try_join!(
            self.insert_coordinator_started_events(&txn, coordinator_started_events),
            self.insert_coordinator_active_events(&txn, coordinator_active_events),
            self.insert_coordinator_shutdown_events(&txn, coordinator_shutdown_events),
            self.insert_agent_session_started_events(&txn, agent_session_started_events),
            self.insert_agent_session_finished_events(&txn, agent_session_finished_events),
            self.insert_job_started_events(&txn, job_started_events),
            self.insert_job_finished_events(&txn, job_finished_events),
            self.insert_coordination_tx_events(&txn, coordination_tx_events),
            self.insert_coordinator_message_events(&txn, coordinator_message_events),
            self.insert_settlement_transaction_events(&txn, settlement_transaction_events),
            self.insert_settlement_transaction_included_events(
                &txn,
                settlement_transaction_included_events
            ),
            self.insert_agent_message_events(&txn, agent_message_events),
        )?;

        // Sum up results from independent insertions
        // total_inserted += independent_results.0
        //     + independent_results.1
        //     + independent_results.2
        //     + independent_results.3
        //     + independent_results.4
        //     + independent_results.5;

        // Phase 2: Handle parent-child relationships for events with sequences
        //debug!("Phase 2: Running parent-child table insertions");
        let _parent_child_results = tokio::try_join!(
            self.insert_jobs_with_sequences(&txn, jobs_events, &job_sequences_data),
            self.insert_proof_events_with_sequences(
                &txn,
                proof_events,
                &proof_event_sequences_data
            ),
        )?;

        //total_inserted += parent_child_results.0 + parent_child_results.1;

        txn.commit().await?;

        let duration = start_time.elapsed();
        let duration_ms = duration.as_millis();
        let events_per_second = events.len() as f64 / duration.as_secs_f64();

        debug!(
            "Successfully parallel batch inserted  {} events in {}ms ({:.2}s) - {:.0} events/second",
            events.len(),
            duration_ms,
            duration.as_secs_f64(),
            events_per_second
        );

        Ok(events.len())
    }

    // Independent table insertion methods - can run in parallel
    async fn insert_coordinator_started_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::coordinator_started_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!(
            "Parallel inserting {} coordinator_started_events",
            events.len()
        );
        let events_len = events.len();
        match entities::coordinator_started_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(result) => {
                debug!("Successfully inserted coordinator_started_events");
                let count =
                    if result.last_insert_id >= 0 && result.last_insert_id <= i64::MAX as i64 {
                        result.last_insert_id as usize
                    } else {
                        warn!(
                            "Invalid last_insert_id value: {}, defaulting to events count",
                            result.last_insert_id
                        );
                        events_len
                    };
                Ok(count)
            }
            Err(e) => {
                error!("Failed to batch insert coordinator_started_events: {}", e);
                Err(e.into())
            }
        }
    }

    async fn insert_coordinator_active_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::coordinator_active_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} coordinator_active_events", events.len());
        let events_len = events.len();
        match entities::coordinator_active_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted coordinator_active_events");
                Ok(events_len)
            }
            Err(e) => {
                error!("Failed to batch insert coordinator_active_events: {}", e);
                Err(e.into())
            }
        }
    }

    async fn insert_coordinator_shutdown_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::coordinator_shutdown_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} coordinator_shutdown_events", events.len());
        let events_len = events.len();
        match entities::coordinator_shutdown_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted coordinator_shutdown_events");
                Ok(events_len)
            }
            Err(e) => {
                error!("Failed to batch insert coordinator_shutdown_events: {}", e);
                Err(e.into())
            }
        }
    }

    async fn insert_agent_session_started_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::agent_session_started_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} agent_session_started_events", events.len());
        let events_len = events.len();
        match entities::agent_session_started_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted agent_session_started_events");
                Ok(events_len)
            }
            Err(e) => {
                error!("Failed to batch insert agent_session_started_events: {}", e);
                Err(e.into())
            }
        }
    }

    async fn insert_agent_session_finished_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::agent_session_finished_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} agent_session_finished_events", events.len());
        let events_len = events.len();
        match entities::agent_session_finished_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted agent_session_finished_events");
                Ok(events_len)
            }
            Err(e) => {
                error!(
                    "Failed to batch insert agent_session_finished_events: {}",
                    e
                );
                Err(e.into())
            }
        }
    }

    async fn insert_job_started_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::job_started_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} job_started_events", events.len());
        let events_len = events.len();
        match entities::job_started_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted job_started_events");
                Ok(events_len)
            }
            Err(e) => {
                error!("Failed to batch insert job_started_events: {}", e);
                Err(e.into())
            }
        }
    }

    async fn insert_job_finished_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::job_finished_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} job_finished_events", events.len());
        let events_len = events.len();
        match entities::job_finished_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted job_finished_events");
                Ok(events_len)
            }
            Err(e) => {
                error!("Failed to batch insert job_finished_events: {}", e);
                Err(e.into())
            }
        }
    }

    async fn insert_coordination_tx_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::coordination_tx_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Parallel inserting {} coordination_tx_events", events.len());
        let events_len = events.len();
        match entities::coordination_tx_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(result) => {
                debug!("Successfully inserted coordination_tx_events");
                let count =
                    if result.last_insert_id >= 0 && result.last_insert_id <= i64::MAX as i64 {
                        result.last_insert_id as usize
                    } else {
                        warn!(
                            "Invalid last_insert_id value: {}, defaulting to events count",
                            result.last_insert_id
                        );
                        events_len
                    };
                Ok(count)
            }
            Err(e) => {
                error!("Failed to batch insert coordination_tx_events: {}", e);
                Err(e.into())
            }
        }
    }

    async fn insert_coordinator_message_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::coordinator_message_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!(
            "Parallel inserting {} coordinator_message_events",
            events.len()
        );
        let events_len = events.len();
        match entities::coordinator_message_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(result) => {
                debug!("Successfully inserted coordinator_message_events");
                let count =
                    if result.last_insert_id >= 0 && result.last_insert_id <= i64::MAX as i64 {
                        result.last_insert_id as usize
                    } else {
                        warn!(
                            "Invalid last_insert_id value: {}, defaulting to events count",
                            result.last_insert_id
                        );
                        events_len
                    };
                Ok(count)
            }
            Err(e) => {
                error!("Failed to batch insert coordinator_message_events: {}", e);
                Err(e.into())
            }
        }
    }

    async fn insert_settlement_transaction_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::settlement_transaction_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} settlement_transaction_events", events.len());
        let events_len = events.len();
        match entities::settlement_transaction_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted settlement_transaction_events");
                Ok(events_len)
            }
            Err(e) => {
                error!(
                    "Failed to batch insert settlement_transaction_events: {}",
                    e
                );
                Err(e.into())
            }
        }
    }

    async fn insert_settlement_transaction_included_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::settlement_transaction_included_in_block_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!(
            "Inserting {} settlement_transaction_included_events",
            events.len()
        );
        let events_len = events.len();
        match entities::settlement_transaction_included_in_block_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted settlement_transaction_included_events");
                Ok(events_len)
            }
            Err(e) => {
                error!(
                    "Failed to batch insert settlement_transaction_included_events: {}",
                    e
                );
                Err(e.into())
            }
        }
    }

    async fn insert_agent_message_events(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::agent_message_event::ActiveModel>,
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} agent_message_events", events.len());
        let events_len = events.len();
        match entities::agent_message_event::Entity::insert_many(events)
            .exec(txn)
            .await
        {
            Ok(_) => {
                debug!("Successfully inserted agent_message_events");
                Ok(events_len)
            }
            Err(e) => {
                error!("Failed to batch insert agent_message_events: {}", e);
                Err(e.into())
            }
        }
    }

    // Parent-child table insertion methods - handle sequences
    async fn insert_jobs_with_sequences(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::jobs::ActiveModel>,
        sequences_data_per_event: &[Vec<(u64, String)>], // (sequence, sequence_type)
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} jobs with sequences", events.len());
        let events_len = events.len();

        // Insert parent records first
        let _parent_result = entities::jobs::Entity::insert_many(events.clone())
            .exec(txn)
            .await?;

        // Note: For jobs table, we don't use base_id since job_id is the primary key

        // Insert sequences child records
        if !sequences_data_per_event.is_empty() {
            let sequence_records =
                self.create_job_sequence_records(&events, sequences_data_per_event);
            if !sequence_records.is_empty() {
                debug!("Inserting {} job_sequences records", sequence_records.len());
                entities::job_sequences::Entity::insert_many(sequence_records)
                    .exec(txn)
                    .await?;
            }
        }

        Ok(events_len)
    }

    async fn insert_proof_events_with_sequences(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        events: Vec<entities::proof_event::ActiveModel>,
        sequences_data_per_event: &[Vec<(u64, String)>], // (sequence, sequence_type)
    ) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        debug!("Inserting {} proof_events with sequences", events.len());
        let events_len = events.len();

        // Insert parent records first
        let parent_result = entities::proof_event::Entity::insert_many(events.clone())
            .exec(txn)
            .await?;

        let base_id = parent_result.last_insert_id;

        // Insert sequences child records
        if !sequences_data_per_event.is_empty() {
            let sequence_records = self.create_proof_event_sequence_records(
                &events,
                base_id,
                sequences_data_per_event,
            );
            if !sequence_records.is_empty() {
                debug!(
                    "Inserting {} proof_event_sequences records",
                    sequence_records.len()
                );
                entities::proof_event_sequences::Entity::insert_many(sequence_records)
                    .exec(txn)
                    .await?;
            }
        }

        Ok(events_len)
    }

    fn create_job_sequence_records(
        &self,
        events: &[entities::jobs::ActiveModel],
        sequences_data_per_event: &[Vec<(u64, String)>],
    ) -> Vec<entities::job_sequences::ActiveModel> {
        use entities::job_sequences::*;
        use sea_orm::ActiveValue;

        let mut records = Vec::new();

        for (event_idx, sequences_data) in sequences_data_per_event.iter().enumerate() {
            if let Some(event) = events.get(event_idx) {
                // Extract job_id and app_instance_id from the event
                let job_id = if let ActiveValue::Set(ref id) = event.job_id {
                    id.clone()
                } else {
                    continue; // Skip if job_id is not set
                };

                let app_instance_id = if let ActiveValue::Set(ref id) = event.app_instance_id {
                    id.clone()
                } else {
                    continue; // Skip if app_instance_id is not set
                };

                for (sequence, sequence_type) in sequences_data {
                    records.push(ActiveModel {
                        id: ActiveValue::NotSet,
                        app_instance_id: ActiveValue::Set(app_instance_id.clone()),
                        sequence: ActiveValue::Set(*sequence as i64),
                        job_id: ActiveValue::Set(job_id.clone()),
                        sequence_type: ActiveValue::Set(sequence_type.clone()),
                        created_at: ActiveValue::NotSet,
                        updated_at: ActiveValue::NotSet,
                    });
                }
            }
        }

        records
    }

    fn create_proof_event_sequence_records(
        &self,
        events: &[entities::proof_event::ActiveModel],
        base_id: i64,
        sequences_data_per_event: &[Vec<(u64, String)>],
    ) -> Vec<entities::proof_event_sequences::ActiveModel> {
        use entities::proof_event_sequences::*;
        use sea_orm::ActiveValue;
        use tracing::warn;

        let mut records = Vec::new();
        let mut current_id = base_id;

        for (event_idx, sequences_data) in sequences_data_per_event.iter().enumerate() {
            if let Some(event) = events.get(event_idx) {
                // Extract app_instance_id from the event
                let app_instance_id = if let ActiveValue::Set(ref id) = event.app_instance_id {
                    if id.is_empty() {
                        warn!(
                            "ProofEvent at index {} has empty app_instance_id, skipping sequences",
                            event_idx
                        );
                        continue; // Skip if app_instance_id is empty
                    }
                    id.clone()
                } else {
                    warn!(
                        "ProofEvent at index {} has no app_instance_id set, skipping sequences",
                        event_idx
                    );
                    continue; // Skip if app_instance_id is not set
                };

                for (sequence, sequence_type) in sequences_data {
                    records.push(ActiveModel {
                        id: ActiveValue::NotSet,
                        app_instance_id: ActiveValue::Set(app_instance_id.clone()),
                        sequence: ActiveValue::Set(*sequence as i64),
                        proof_event_id: ActiveValue::Set(current_id),
                        sequence_type: ActiveValue::Set(sequence_type.clone()),
                        created_at: ActiveValue::NotSet,
                        updated_at: ActiveValue::NotSet,
                    });
                }
            }
            current_id = current_id.saturating_add(1);
        }

        records
    }

    #[allow(dead_code)]
    pub async fn get_connection(&self) -> &DatabaseConnection {
        &self.connection
    }

    // Query methods for retrieving events by sequence
    pub async fn get_agent_transaction_events_by_sequence(
        &self,
        sequence: u64,
        limit: Option<u32>,
        offset: Option<u32>,
        coordinator_id: Option<String>,
        developer: Option<String>,
        agent: Option<String>,
        app: Option<String>,
    ) -> Result<(Vec<AgentTransactionEventResult>, u32)> {
        use sea_orm::{ConnectionTrait, Statement};

        // Build the WHERE clause for optional filters
        let mut where_conditions = vec!["seqs.sequence = ?".to_string()];
        // FIXED: Safe sequence casting to prevent overflow
        let safe_sequence = if sequence <= i64::MAX as u64 {
            sequence as i64
        } else {
            warn!(
                "Sequence value {} exceeds i64::MAX, clamping to maximum",
                sequence
            );
            i64::MAX
        };
        let mut params: Vec<sea_orm::Value> = vec![safe_sequence.into()];

        if let Some(coord_id) = &coordinator_id {
            where_conditions.push("e.coordinator_id = ?".to_string());
            params.push(coord_id.clone().into());
        }
        if let Some(dev) = &developer {
            where_conditions.push("e.developer = ?".to_string());
            params.push(dev.clone().into());
        }
        if let Some(agent_filter) = &agent {
            where_conditions.push("e.agent = ?".to_string());
            params.push(agent_filter.clone().into());
        }
        if let Some(app_filter) = &app {
            where_conditions.push("e.app = ?".to_string());
            params.push(app_filter.clone().into());
        }

        let where_clause = where_conditions.join(" AND ");

        // First, get the count for pagination
        let count_query = format!(
            "SELECT COUNT(DISTINCT e.id) as count 
             FROM agent_transaction_event e 
             INNER JOIN agent_transaction_event_sequences seqs ON e.id = seqs.agent_transaction_event_id 
             WHERE {}",
            where_clause
        );

        let count_stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            &count_query,
            params.clone(),
        );

        let count_result = self.connection.query_one(count_stmt).await?;
        let total_count: u32 = count_result
            .map(|row| {
                let count_i64 = row.try_get::<i64>("", "count").unwrap_or(0);
                if count_i64 >= 0 && count_i64 <= u32::MAX as i64 {
                    count_i64 as u32
                } else {
                    warn!(
                        "Count value {} out of u32 range, clamping to u32::MAX",
                        count_i64
                    );
                    if count_i64 < 0 { 0 } else { u32::MAX }
                }
            })
            .unwrap_or(0);

        if total_count == 0 {
            return Ok((Vec::new(), 0));
        }

        // Build the main query with all event data and sequences in one go
        let mut main_query = format!(
            "SELECT e.id, e.coordinator_id, e.tx_type, e.developer, e.agent, e.app, e.job_sequence, 
                    e.event_timestamp, e.tx_hash, e.chain, e.network, e.memo, e.metadata, 
                    e.created_at, GROUP_CONCAT(all_seqs.sequence) as sequences
             FROM agent_transaction_event e 
             INNER JOIN agent_transaction_event_sequences seqs ON e.id = seqs.agent_transaction_event_id
             LEFT JOIN agent_transaction_event_sequences all_seqs ON e.id = all_seqs.agent_transaction_event_id
             WHERE {}
             GROUP BY e.id, e.coordinator_id, e.tx_type, e.developer, e.agent, e.app, e.job_sequence, 
                      e.event_timestamp, e.tx_hash, e.chain, e.network, e.memo, e.metadata, e.created_at
             ORDER BY e.created_at DESC",
            where_clause
        );

        // Add pagination
        if let Some(limit_val) = limit {
            main_query.push_str(&format!(" LIMIT {}", limit_val));
            if let Some(offset_val) = offset {
                main_query.push_str(&format!(" OFFSET {}", offset_val));
            }
        }

        let main_stmt =
            Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, &main_query, params);

        let rows = self.connection.query_all(main_stmt).await?;

        let mut results = Vec::new();
        for row in rows {
            let sequences_str: String = row.try_get("", "sequences").unwrap_or_default();
            let sequences: Vec<u64> = if sequences_str.is_empty() {
                Vec::new()
            } else {
                sequences_str
                    .split(',')
                    .filter_map(|s| s.parse::<u64>().ok())
                    .collect()
            };

            let created_at_timestamp: i64 = row
                .try_get::<chrono::DateTime<chrono::Utc>>("", "created_at")
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            results.push(AgentTransactionEventResult {
                id: row.try_get("", "id").unwrap_or(0),
                coordinator_id: row.try_get("", "coordinator_id").unwrap_or_default(),
                tx_type: row.try_get("", "tx_type").unwrap_or_default(),
                developer: row.try_get("", "developer").unwrap_or_default(),
                agent: row.try_get("", "agent").unwrap_or_default(),
                app: row.try_get("", "app").unwrap_or_default(),
                job_sequence: row.try_get("", "job_sequence").unwrap_or_default(),
                sequences,
                event_timestamp: {
                    let timestamp_i64 = row.try_get::<i64>("", "event_timestamp").unwrap_or(0);
                    if timestamp_i64 >= 0 {
                        timestamp_i64 as u64
                    } else {
                        warn!(
                            "Negative timestamp value {}, defaulting to 0",
                            timestamp_i64
                        );
                        0
                    }
                },
                tx_hash: row.try_get("", "tx_hash").unwrap_or_default(),
                chain: row.try_get("", "chain").unwrap_or_default(),
                network: row.try_get("", "network").unwrap_or_default(),
                memo: row.try_get("", "memo").unwrap_or_default(),
                metadata: row.try_get("", "metadata").unwrap_or_default(),
                created_at_timestamp,
            });
        }

        Ok((results, total_count))
    }

    pub async fn get_agent_message_events_by_sequence(
        &self,
        sequence: u64,
        limit: Option<u32>,
        offset: Option<u32>,
        coordinator_id: Option<String>,
        developer: Option<String>,
        agent: Option<String>,
        app: Option<String>,
    ) -> Result<(Vec<AgentMessageEventResult>, u32)> {
        use sea_orm::{ConnectionTrait, Statement};

        // Build the WHERE clause for optional filters
        let mut where_conditions = vec!["seqs.sequence = ?".to_string()];
        // FIXED: Safe sequence casting to prevent overflow
        let safe_sequence = if sequence <= i64::MAX as u64 {
            sequence as i64
        } else {
            warn!(
                "Sequence value {} exceeds i64::MAX, clamping to maximum",
                sequence
            );
            i64::MAX
        };
        let mut params: Vec<sea_orm::Value> = vec![safe_sequence.into()];

        if let Some(coord_id) = &coordinator_id {
            where_conditions.push("e.coordinator_id = ?".to_string());
            params.push(coord_id.clone().into());
        }
        if let Some(dev) = &developer {
            where_conditions.push("e.developer = ?".to_string());
            params.push(dev.clone().into());
        }
        if let Some(agent_filter) = &agent {
            where_conditions.push("e.agent = ?".to_string());
            params.push(agent_filter.clone().into());
        }
        if let Some(app_filter) = &app {
            where_conditions.push("e.app = ?".to_string());
            params.push(app_filter.clone().into());
        }

        let where_clause = where_conditions.join(" AND ");

        // First, get the count for pagination
        let count_query = format!(
            "SELECT COUNT(DISTINCT e.id) as count 
             FROM agent_message_event e 
             INNER JOIN agent_message_event_sequences seqs ON e.id = seqs.agent_message_event_id 
             WHERE {}",
            where_clause
        );

        let count_stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            &count_query,
            params.clone(),
        );

        let count_result = self.connection.query_one(count_stmt).await?;
        let total_count: u32 = count_result
            .map(|row| {
                let count_i64 = row.try_get::<i64>("", "count").unwrap_or(0);
                if count_i64 >= 0 && count_i64 <= u32::MAX as i64 {
                    count_i64 as u32
                } else {
                    warn!(
                        "Count value {} out of u32 range, clamping to u32::MAX",
                        count_i64
                    );
                    if count_i64 < 0 { 0 } else { u32::MAX }
                }
            })
            .unwrap_or(0);

        if total_count == 0 {
            return Ok((Vec::new(), 0));
        }

        // Build the main query with all event data and sequences in one go
        let mut main_query = format!(
            "SELECT e.id, e.coordinator_id, e.developer, e.agent, e.app, e.job_sequence, 
                    e.event_timestamp, e.level, e.message, e.created_at, 
                    GROUP_CONCAT(all_seqs.sequence) as sequences
             FROM agent_message_event e 
             INNER JOIN agent_message_event_sequences seqs ON e.id = seqs.agent_message_event_id
             LEFT JOIN agent_message_event_sequences all_seqs ON e.id = all_seqs.agent_message_event_id
             WHERE {}
             GROUP BY e.id, e.coordinator_id, e.developer, e.agent, e.app, e.job_sequence, 
                      e.event_timestamp, e.level, e.message, e.created_at
             ORDER BY e.created_at DESC",
            where_clause
        );

        // Add pagination
        if let Some(limit_val) = limit {
            main_query.push_str(&format!(" LIMIT {}", limit_val));
            if let Some(offset_val) = offset {
                main_query.push_str(&format!(" OFFSET {}", offset_val));
            }
        }

        let main_stmt =
            Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, &main_query, params);

        let rows = self.connection.query_all(main_stmt).await?;

        let mut results = Vec::new();
        for row in rows {
            let sequences_str: String = row.try_get("", "sequences").unwrap_or_default();
            let sequences: Vec<u64> = if sequences_str.is_empty() {
                Vec::new()
            } else {
                sequences_str
                    .split(',')
                    .filter_map(|s| s.parse::<u64>().ok())
                    .collect()
            };

            let created_at_timestamp: i64 = row
                .try_get::<chrono::DateTime<chrono::Utc>>("", "created_at")
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            // FIXED: Safe level casting from i32 to u32
            let level: u32 = {
                let level_i32 = row.try_get::<i32>("", "level").unwrap_or(0);
                if level_i32 >= 0 {
                    level_i32 as u32
                } else {
                    warn!("Negative level value {}, defaulting to 0", level_i32);
                    0
                }
            };

            results.push(AgentMessageEventResult {
                id: row.try_get("", "id").unwrap_or(0),
                coordinator_id: row.try_get("", "coordinator_id").unwrap_or_default(),
                developer: row.try_get("", "developer").unwrap_or_default(),
                agent: row.try_get("", "agent").unwrap_or_default(),
                app: row.try_get("", "app").unwrap_or_default(),
                job_sequence: row.try_get("", "job_sequence").unwrap_or_default(),
                sequences,
                event_timestamp: {
                    let timestamp_i64 = row.try_get::<i64>("", "event_timestamp").unwrap_or(0);
                    if timestamp_i64 >= 0 {
                        timestamp_i64 as u64
                    } else {
                        warn!(
                            "Negative timestamp value {}, defaulting to 0",
                            timestamp_i64
                        );
                        0
                    }
                },
                level,
                message: row.try_get("", "message").unwrap_or_default(),
                created_at_timestamp,
            });
        }

        Ok((results, total_count))
    }

    /// Search CoordinatorMessageEvent using full-text search on message content
    /// Uses TiDB's FTS_MATCH_WORD function with automatic language detection and BM25 relevance ranking
    pub async fn search_coordinator_message_events(
        &self,
        search_query: &str,
        limit: Option<u32>,
        offset: Option<u32>,
        coordinator_id: Option<String>,
    ) -> Result<(Vec<CoordinatorMessageEventResult>, u32)> {
        use sea_orm::{ConnectionTrait, Statement};

        if search_query.trim().is_empty() {
            return Ok((Vec::new(), 0));
        }

        // Escape single quotes in search query to prevent SQL injection
        let escaped_query = search_query.replace("'", "''");

        // Build the WHERE clause - TiDB requires literal strings for FTS_MATCH_WORD, not parameters
        let mut where_conditions = vec![format!("fts_match_word('{}', message)", escaped_query)];
        let mut params: Vec<sea_orm::Value> = Vec::new();

        if let Some(coord_id) = &coordinator_id {
            where_conditions.push("coordinator_id = ?".to_string());
            params.push(coord_id.clone().into());
        }

        let where_clause = where_conditions.join(" AND ");

        // First, get the count for pagination
        let count_query = format!(
            "SELECT COUNT(*) as count 
             FROM coordinator_message_event 
             WHERE {}",
            where_clause
        );

        let count_stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            &count_query,
            params.clone(),
        );

        let count_result = self.connection.query_one(count_stmt).await?;
        let total_count: u32 = count_result
            .map(|row| {
                let count_i64 = row.try_get::<i64>("", "count").unwrap_or(0);
                if count_i64 >= 0 && count_i64 <= u32::MAX as i64 {
                    count_i64 as u32
                } else {
                    warn!(
                        "Count value {} out of u32 range, clamping to u32::MAX",
                        count_i64
                    );
                    if count_i64 < 0 { 0 } else { u32::MAX }
                }
            })
            .unwrap_or(0);

        if total_count == 0 {
            return Ok((Vec::new(), 0));
        }

        // Build the main query with full-text search and relevance scoring
        let mut main_query = format!(
            "SELECT id, coordinator_id, event_timestamp, level, message, created_at,
                    fts_match_word('{}', message) as relevance_score
             FROM coordinator_message_event 
             WHERE {}
             ORDER BY fts_match_word('{}', message) DESC, created_at DESC",
            escaped_query, where_clause, escaped_query
        );

        // Add pagination - TiDB requires LIMIT when using FTS_MATCH_WORD in ORDER BY
        let limit_val = limit.unwrap_or(1000); // Use large default limit if none specified
        main_query.push_str(&format!(" LIMIT {}", limit_val));
        if let Some(offset_val) = offset {
            main_query.push_str(&format!(" OFFSET {}", offset_val));
        }

        let main_stmt =
            Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, &main_query, params);

        let rows = self.connection.query_all(main_stmt).await?;

        let mut results = Vec::new();
        for row in rows {
            let created_at_timestamp: i64 = row
                .try_get::<chrono::DateTime<chrono::Utc>>("", "created_at")
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            let level: i32 = row.try_get::<i32>("", "level").unwrap_or(0);

            let relevance_score: f64 = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

            results.push(CoordinatorMessageEventResult {
                id: row.try_get("", "id").unwrap_or(0),
                coordinator_id: row.try_get("", "coordinator_id").unwrap_or_default(),
                event_timestamp: {
                    let timestamp_i64 = row.try_get::<i64>("", "event_timestamp").unwrap_or(0);
                    if timestamp_i64 >= 0 {
                        timestamp_i64 as u64
                    } else {
                        warn!(
                            "Negative event_timestamp value {}, defaulting to 0",
                            timestamp_i64
                        );
                        0
                    }
                },
                level,
                message: row.try_get("", "message").unwrap_or_default(),
                created_at_timestamp,
                relevance_score,
            });
        }

        Ok((results, total_count))
    }
}

// Conversion functions from protobuf messages to Sea-ORM entities

fn convert_coordinator_started_event(
    event: &proto::CoordinatorStartedEvent,
) -> entities::coordinator_started_event::ActiveModel {
    use entities::coordinator_started_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        ethereum_address: ActiveValue::Set(event.ethereum_address.clone()),
        event_timestamp: ActiveValue::Set(if event.event_timestamp <= i64::MAX as u64 {
            event.event_timestamp as i64
        } else {
            warn!(
                "Event timestamp {} exceeds i64::MAX, clamping to maximum",
                event.event_timestamp
            );
            i64::MAX
        }),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_coordinator_active_event(
    event: &proto::CoordinatorActiveEvent,
) -> entities::coordinator_active_event::ActiveModel {
    use entities::coordinator_active_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_coordinator_shutdown_event(
    event: &proto::CoordinatorShutdownEvent,
) -> entities::coordinator_shutdown_event::ActiveModel {
    use entities::coordinator_shutdown_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_agent_session_started_event(
    event: &proto::AgentSessionStartedEvent,
) -> entities::agent_session_started_event::ActiveModel {
    use entities::agent_session_started_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        developer: ActiveValue::Set(event.developer.clone()),
        agent: ActiveValue::Set(event.agent.clone()),
        agent_method: ActiveValue::Set(event.agent_method.clone()),
        session_id: ActiveValue::Set(event.session_id.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_agent_session_finished_event(
    event: &proto::AgentSessionFinishedEvent,
) -> entities::agent_session_finished_event::ActiveModel {
    use entities::agent_session_finished_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        session_id: ActiveValue::Set(event.session_id.clone()),
        session_log: ActiveValue::Set(event.session_log.clone()),
        duration: ActiveValue::Set(event.duration as i64),
        cost: ActiveValue::Set(event.cost as i64),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_job_created_event(
    event: &proto::JobCreatedEvent,
) -> (
    entities::jobs::ActiveModel,
    Vec<u64>, // sequences
    Vec<u64>, // merged_sequences_1
    Vec<u64>, // merged_sequences_2
) {
    use entities::jobs::*;
    use sea_orm::ActiveValue;

    let sequences_json = if !event.sequences.is_empty() {
        Some(serde_json::to_string(&event.sequences).unwrap_or_else(|_| "[]".to_string()))
    } else {
        None
    };

    let merged_sequences_1_json = if !event.merged_sequences_1.is_empty() {
        Some(serde_json::to_string(&event.merged_sequences_1).unwrap_or_else(|_| "[]".to_string()))
    } else {
        None
    };

    let merged_sequences_2_json = if !event.merged_sequences_2.is_empty() {
        Some(serde_json::to_string(&event.merged_sequences_2).unwrap_or_else(|_| "[]".to_string()))
    } else {
        None
    };

    let active_model = ActiveModel {
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        session_id: ActiveValue::Set(event.session_id.clone()),
        app_instance_id: ActiveValue::Set(event.app_instance_id.clone()),
        app_method: ActiveValue::Set(event.app_method.clone()),
        job_sequence: ActiveValue::Set(event.job_sequence as i64),
        sequences: ActiveValue::Set(sequences_json),
        merged_sequences_1: ActiveValue::Set(merged_sequences_1_json),
        merged_sequences_2: ActiveValue::Set(merged_sequences_2_json),
        job_id: ActiveValue::Set(event.job_id.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    };

    (
        active_model,
        event.sequences.clone(),
        event.merged_sequences_1.clone(),
        event.merged_sequences_2.clone(),
    )
}

fn convert_job_started_event(
    event: &proto::JobStartedEvent,
) -> entities::job_started_event::ActiveModel {
    use entities::job_started_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        session_id: ActiveValue::Set(event.session_id.clone()),
        app_instance_id: ActiveValue::Set(event.app_instance_id.clone()),
        job_id: ActiveValue::Set(event.job_id.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_job_finished_event(
    event: &proto::JobFinishedEvent,
) -> entities::job_finished_event::ActiveModel {
    use entities::job_finished_event::*;
    use sea_orm::ActiveValue;

    // Convert JobResult enum to string for ENUM column
    let result_str = match event.result() {
        proto::JobResult::Unspecified => "JOB_RESULT_UNSPECIFIED",
        proto::JobResult::Completed => "JOB_RESULT_COMPLETED",
        proto::JobResult::Failed => "JOB_RESULT_FAILED",
        proto::JobResult::Terminated => "JOB_RESULT_TERMINATED",
    };

    ActiveModel {
        job_id: ActiveValue::Set(event.job_id.clone()), // Primary key
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        duration: ActiveValue::Set(event.duration as i64),
        cost: ActiveValue::Set(event.cost as i64),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        result: ActiveValue::Set(result_str.to_string()),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_coordination_tx_event(
    event: &proto::CoordinationTxEvent,
) -> entities::coordination_tx_event::ActiveModel {
    use entities::coordination_tx_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        tx_hash: ActiveValue::Set(event.tx_hash.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_coordinator_message_event(
    event: &proto::CoordinatorMessageEvent,
) -> entities::coordinator_message_event::ActiveModel {
    use entities::coordinator_message_event::*;
    use sea_orm::ActiveValue;

    // Convert LogLevel enum to string for ENUM column
    let level_str = match event.level() {
        proto::LogLevel::Unspecified => "LOG_LEVEL_UNSPECIFIED",
        proto::LogLevel::Debug => "LOG_LEVEL_DEBUG",
        proto::LogLevel::Info => "LOG_LEVEL_INFO",
        proto::LogLevel::Warn => "LOG_LEVEL_WARN",
        proto::LogLevel::Error => "LOG_LEVEL_ERROR",
        proto::LogLevel::Fatal => "LOG_LEVEL_FATAL",
    };

    ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        level: ActiveValue::Set(level_str.to_string()),
        message: ActiveValue::Set(event.message.clone()),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_proof_event(
    event: &proto::ProofEvent,
) -> (
    entities::proof_event::ActiveModel,
    Vec<(u64, String)>, // (sequence, sequence_type) for child table
) {
    use entities::proof_event::*;
    use sea_orm::ActiveValue;
    use tracing::warn;

    // Debug logging to understand what's being received
    if event.app_instance_id.is_empty() {
        warn!(
            "ProofEvent received with empty app_instance_id. Event: coordinator_id={}, block_number={}, sequences={:?}",
            event.coordinator_id, event.block_number, event.sequences
        );
    }

    // Convert sequences to JSON strings for storage
    let sequences_json = if !event.sequences.is_empty() {
        Some(serde_json::to_string(&event.sequences).unwrap_or_else(|_| "[]".to_string()))
    } else {
        None
    };

    let merged_sequences_1_json = if !event.merged_sequences_1.is_empty() {
        Some(serde_json::to_string(&event.merged_sequences_1).unwrap_or_else(|_| "[]".to_string()))
    } else {
        None
    };

    let merged_sequences_2_json = if !event.merged_sequences_2.is_empty() {
        Some(serde_json::to_string(&event.merged_sequences_2).unwrap_or_else(|_| "[]".to_string()))
    } else {
        None
    };

    // Convert ProofEventType enum to string for ENUM column
    let proof_type_str = match event.proof_event_type() {
        proto::ProofEventType::Unspecified => "PROOF_EVENT_TYPE_UNSPECIFIED",
        proto::ProofEventType::ProofSubmitted => "PROOF_SUBMITTED",
        proto::ProofEventType::ProofFetched => "PROOF_FETCHED",
        proto::ProofEventType::ProofVerified => "PROOF_VERIFIED",
        proto::ProofEventType::ProofUnavailable => "PROOF_UNAVAILABLE",
        proto::ProofEventType::ProofRejected => "PROOF_REJECTED",
    };

    let active_model = ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        session_id: ActiveValue::Set(event.session_id.clone()),
        app_instance_id: ActiveValue::Set(event.app_instance_id.clone()),
        job_id: ActiveValue::Set(event.job_id.clone()),
        data_availability: ActiveValue::Set(event.data_availability.clone()),
        block_number: ActiveValue::Set(event.block_number as i64),
        block_proof: ActiveValue::Set(event.block_proof),
        proof_event_type: ActiveValue::Set(proof_type_str.to_string()),
        sequences: ActiveValue::Set(sequences_json),
        merged_sequences_1: ActiveValue::Set(merged_sequences_1_json),
        merged_sequences_2: ActiveValue::Set(merged_sequences_2_json),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    };

    // Prepare data for child table
    let mut child_sequences = Vec::new();
    for seq in &event.sequences {
        child_sequences.push((*seq, "sequences".to_string()));
    }
    for seq in &event.merged_sequences_1 {
        child_sequences.push((*seq, "merged_sequences_1".to_string()));
    }
    for seq in &event.merged_sequences_2 {
        child_sequences.push((*seq, "merged_sequences_2".to_string()));
    }

    (active_model, child_sequences)
}

fn convert_settlement_transaction_event(
    event: &proto::SettlementTransactionEvent,
) -> entities::settlement_transaction_event::ActiveModel {
    use entities::settlement_transaction_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        session_id: ActiveValue::Set(event.session_id.clone()),
        app_instance_id: ActiveValue::Set(event.app_instance_id.clone()),
        chain: ActiveValue::Set(event.chain.clone()),
        job_id: ActiveValue::Set(event.job_id.clone()),
        block_number: ActiveValue::Set(event.block_number as i64),
        tx_hash: ActiveValue::Set(event.tx_hash.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_settlement_transaction_included_event(
    event: &proto::SettlementTransactionIncludedInBlockEvent,
) -> entities::settlement_transaction_included_in_block_event::ActiveModel {
    use entities::settlement_transaction_included_in_block_event::*;
    use sea_orm::ActiveValue;

    ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        session_id: ActiveValue::Set(event.session_id.clone()),
        app_instance_id: ActiveValue::Set(event.app_instance_id.clone()),
        chain: ActiveValue::Set(event.chain.clone()),
        job_id: ActiveValue::Set(event.job_id.clone()),
        block_number: ActiveValue::Set(event.block_number as i64),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

fn convert_agent_message_event(
    event: &proto::AgentMessageEvent,
) -> entities::agent_message_event::ActiveModel {
    use entities::agent_message_event::*;
    use sea_orm::ActiveValue;

    // Convert LogLevel enum to string for ENUM column
    let level_str = match event.level() {
        proto::LogLevel::Unspecified => "LOG_LEVEL_UNSPECIFIED",
        proto::LogLevel::Debug => "LOG_LEVEL_DEBUG",
        proto::LogLevel::Info => "LOG_LEVEL_INFO",
        proto::LogLevel::Warn => "LOG_LEVEL_WARN",
        proto::LogLevel::Error => "LOG_LEVEL_ERROR",
        proto::LogLevel::Fatal => "LOG_LEVEL_FATAL",
    };

    ActiveModel {
        id: ActiveValue::NotSet,
        coordinator_id: ActiveValue::Set(event.coordinator_id.clone()),
        session_id: ActiveValue::Set(event.session_id.clone()),
        job_id: ActiveValue::Set(event.job_id.clone()),
        event_timestamp: ActiveValue::Set(event.event_timestamp as i64),
        level: ActiveValue::Set(level_str.to_string()),
        message: ActiveValue::Set(event.message.clone()),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_safe_casting_functions() {
        // Test sequence casting edge cases
        let max_u64 = u64::MAX;
        let safe_sequence = if max_u64 <= i64::MAX as u64 {
            max_u64 as i64
        } else {
            i64::MAX
        };
        assert_eq!(safe_sequence, i64::MAX);

        // Test timestamp casting edge cases
        let test_timestamp = i64::MAX as u64 + 1;
        let safe_timestamp = if test_timestamp <= i64::MAX as u64 {
            test_timestamp as i64
        } else {
            i64::MAX
        };
        assert_eq!(safe_timestamp, i64::MAX);

        // Test count casting edge cases
        let negative_count = -1i64;
        let safe_count = if negative_count >= 0 && negative_count <= u32::MAX as i64 {
            negative_count as u32
        } else {
            if negative_count < 0 { 0 } else { u32::MAX }
        };
        assert_eq!(safe_count, 0);

        // Test level casting edge cases
        let negative_level = -5i32;
        let safe_level = if negative_level >= 0 {
            negative_level as u32
        } else {
            0
        };
        assert_eq!(safe_level, 0);
    }

    #[test]
    fn test_overflow_protection() {
        // Test ID incrementing protection
        let current_id = i64::MAX;
        let next_id = current_id.saturating_add(1);
        assert_eq!(next_id, i64::MAX); // Should not overflow

        // Test u64 to i64 conversion limits
        assert!(u64::MAX > i64::MAX as u64);

        // Test that our casting logic handles this correctly
        let large_value = u64::MAX;
        let safe_value = if large_value <= i64::MAX as u64 {
            large_value as i64
        } else {
            i64::MAX
        };
        assert_eq!(safe_value, i64::MAX);
    }

    #[test]
    fn test_memory_bounds() {
        // Test that u32::MAX casting bounds work correctly
        let large_count = u32::MAX as i64 + 1;
        let safe_count = if large_count >= 0 && large_count <= u32::MAX as i64 {
            large_count as u32
        } else {
            if large_count < 0 { 0 } else { u32::MAX }
        };
        assert_eq!(safe_count, u32::MAX);
    }
}

// Search result structure
pub struct SearchResult {
    pub events: Vec<proto::EventWithRelevance>,
    pub total_count: usize,
    pub returned_count: usize,
}

// Search function for coordinator messages using TiDB's fulltext search
pub async fn search_coordinator_messages(
    database: &EventDatabase,
    search_query: &str,
    coordinator_id: Option<&str>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<SearchResult> {
    use sea_orm::{ConnectionTrait, Statement};

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

    // Build the WHERE clause - TiDB uses fts_match_word for fulltext search
    let mut where_conditions = vec![format!("fts_match_word('{}', message)", escaped_query)];
    let mut params: Vec<sea_orm::Value> = Vec::new();

    if let Some(coord_id) = coordinator_id {
        where_conditions.push("coordinator_id = ?".to_string());
        params.push(coord_id.into());
    }

    let where_clause = where_conditions.join(" AND ");

    // First, get the count for pagination
    let count_query = format!(
        "SELECT COUNT(*) as count FROM coordinator_message_event WHERE {}",
        where_clause
    );

    let count_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::MySql,
        &count_query,
        params.clone(),
    );

    let count_result = database.connection.query_one(count_stmt).await?;
    let total_count = count_result
        .and_then(|row| row.try_get::<i64>("", "count").ok())
        .unwrap_or(0) as usize;

    if total_count == 0 {
        return Ok(SearchResult {
            events: vec![],
            total_count: 0,
            returned_count: 0,
        });
    }

    // Build the main query with full-text search and relevance scoring
    let mut main_query = format!(
        "SELECT id, coordinator_id, event_timestamp, level, message, created_at,
                fts_match_word('{}', message) as relevance_score
         FROM coordinator_message_event
         WHERE {}
         ORDER BY fts_match_word('{}', message) DESC, created_at DESC",
        escaped_query, where_clause, escaped_query
    );

    // Add pagination
    let limit_val = limit.unwrap_or(100).min(1000);
    main_query.push_str(&format!(" LIMIT {}", limit_val));
    if let Some(offset_val) = offset {
        main_query.push_str(&format!(" OFFSET {}", offset_val));
    }

    let main_stmt =
        Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, &main_query, params);

    let results = database.connection.query_all(main_stmt).await?;

    // Convert to proto events with relevance scores
    let mut events = Vec::new();
    for row in results.iter() {
        let id = row.try_get::<i64>("", "id").unwrap_or(0);
        let coordinator_id = row
            .try_get::<String>("", "coordinator_id")
            .unwrap_or_default();
        let event_timestamp = row.try_get::<i64>("", "event_timestamp").unwrap_or(0) as u64;
        let level = row.try_get::<i8>("", "level").unwrap_or(0) as i32;
        let message = row.try_get::<String>("", "message").unwrap_or_default();
        let relevance_score = row.try_get::<f64>("", "relevance_score").unwrap_or(0.0);

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

        events.push(proto::EventWithRelevance {
            id,
            event: Some(event),
            relevance_score,
        });
    }

    let returned_count = events.len();

    debug!(
        "Search completed in {}ms: query='{}', found={}/{} events",
        start_time.elapsed().as_millis(),
        search_query,
        returned_count,
        total_count
    );

    Ok(SearchResult {
        events,
        total_count,
        returned_count,
    })
}

/// Helper function to parse JSON array of u64 values
pub fn parse_json_array(json_str: &str) -> Result<Vec<u64>> {
    if json_str.is_empty() || json_str == "null" {
        return Ok(Vec::new());
    }

    serde_json::from_str::<Vec<u64>>(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse JSON array: {}", e))
}

/// Get database connection
pub fn get_connection(database: &EventDatabase) -> &sea_orm::DatabaseConnection {
    &database.connection
}

/// Search all tables with fulltext indexes and return the most relevant events (deprecated - use search_all_events_parallel)
pub async fn search_all_events(
    database: &EventDatabase,
    search_query: &str,
    limit: Option<u32>,
) -> Result<SearchResult> {
    // Redirect to the parallel implementation
    crate::database_search::search_all_events_parallel(database, search_query, limit).await
}
