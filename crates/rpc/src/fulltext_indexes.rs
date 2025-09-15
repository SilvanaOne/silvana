// Auto-generated fulltext index metadata
// DO NOT EDIT - Generated from protobuf definitions

use lazy_static::lazy_static;

#[derive(Debug, Clone)]
pub struct FulltextIndex {
    pub table_name: &'static str,
    pub column_name: &'static str,
    pub index_name: &'static str,
}

lazy_static! {
    pub static ref FULLTEXT_INDEXES: Vec<FulltextIndex> = vec![
        FulltextIndex {
            table_name: "coordinator_started_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "coordinator_active_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "coordinator_shutdown_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "agent_session",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "agent_session",
            column_name: "developer",
            index_name: "ft_idx_developer",
        },
        FulltextIndex {
            table_name: "agent_session",
            column_name: "agent",
            index_name: "ft_idx_agent",
        },
        FulltextIndex {
            table_name: "agent_session",
            column_name: "agent_method",
            index_name: "ft_idx_agent_method",
        },
        FulltextIndex {
            table_name: "agent_session",
            column_name: "session_id",
            index_name: "ft_idx_session_id",
        },
        FulltextIndex {
            table_name: "agent_session_finished_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "agent_session_finished_event",
            column_name: "session_id",
            index_name: "ft_idx_session_id",
        },
        FulltextIndex {
            table_name: "jobs",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "jobs",
            column_name: "session_id",
            index_name: "ft_idx_session_id",
        },
        FulltextIndex {
            table_name: "jobs",
            column_name: "app_instance_id",
            index_name: "ft_idx_app_instance_id",
        },
        FulltextIndex {
            table_name: "jobs",
            column_name: "app_method",
            index_name: "ft_idx_app_method",
        },
        FulltextIndex {
            table_name: "jobs",
            column_name: "job_id",
            index_name: "ft_idx_job_id",
        },
        FulltextIndex {
            table_name: "job_started_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "job_started_event",
            column_name: "session_id",
            index_name: "ft_idx_session_id",
        },
        FulltextIndex {
            table_name: "job_started_event",
            column_name: "app_instance_id",
            index_name: "ft_idx_app_instance_id",
        },
        FulltextIndex {
            table_name: "job_started_event",
            column_name: "job_id",
            index_name: "ft_idx_job_id",
        },
        FulltextIndex {
            table_name: "job_finished_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "job_finished_event",
            column_name: "job_id",
            index_name: "ft_idx_job_id",
        },
        FulltextIndex {
            table_name: "coordination_tx_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "coordination_tx_event",
            column_name: "tx_hash",
            index_name: "ft_idx_tx_hash",
        },
        FulltextIndex {
            table_name: "coordinator_message_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "coordinator_message_event",
            column_name: "message",
            index_name: "ft_idx_message",
        },
        FulltextIndex {
            table_name: "proof_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "proof_event",
            column_name: "session_id",
            index_name: "ft_idx_session_id",
        },
        FulltextIndex {
            table_name: "proof_event",
            column_name: "app_instance_id",
            index_name: "ft_idx_app_instance_id",
        },
        FulltextIndex {
            table_name: "proof_event",
            column_name: "job_id",
            index_name: "ft_idx_job_id",
        },
        FulltextIndex {
            table_name: "settlement_transaction_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "settlement_transaction_event",
            column_name: "session_id",
            index_name: "ft_idx_session_id",
        },
        FulltextIndex {
            table_name: "settlement_transaction_event",
            column_name: "app_instance_id",
            index_name: "ft_idx_app_instance_id",
        },
        FulltextIndex {
            table_name: "settlement_transaction_event",
            column_name: "job_id",
            index_name: "ft_idx_job_id",
        },
        FulltextIndex {
            table_name: "settlement_transaction_event",
            column_name: "tx_hash",
            index_name: "ft_idx_tx_hash",
        },
        FulltextIndex {
            table_name: "settlement_transaction_included_in_block_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "settlement_transaction_included_in_block_event",
            column_name: "session_id",
            index_name: "ft_idx_session_id",
        },
        FulltextIndex {
            table_name: "settlement_transaction_included_in_block_event",
            column_name: "app_instance_id",
            index_name: "ft_idx_app_instance_id",
        },
        FulltextIndex {
            table_name: "settlement_transaction_included_in_block_event",
            column_name: "job_id",
            index_name: "ft_idx_job_id",
        },
        FulltextIndex {
            table_name: "agent_message_event",
            column_name: "coordinator_id",
            index_name: "ft_idx_coordinator_id",
        },
        FulltextIndex {
            table_name: "agent_message_event",
            column_name: "session_id",
            index_name: "ft_idx_session_id",
        },
        FulltextIndex {
            table_name: "agent_message_event",
            column_name: "job_id",
            index_name: "ft_idx_job_id",
        },
        FulltextIndex {
            table_name: "agent_message_event",
            column_name: "message",
            index_name: "ft_idx_message",
        },
    ];
}

pub fn get_indexes_for_table(table_name: &str) -> Vec<&'static FulltextIndex> {
    FULLTEXT_INDEXES
        .iter()
        .filter(|idx| idx.table_name == table_name)
        .collect()
}

pub fn get_tables_with_fulltext() -> Vec<&'static str> {
    let mut tables: Vec<&str> = FULLTEXT_INDEXES.iter().map(|idx| idx.table_name).collect();
    tables.sort();
    tables.dedup();
    tables
}
