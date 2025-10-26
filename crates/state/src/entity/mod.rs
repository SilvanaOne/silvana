//! Sea-ORM entities for state management tables

pub mod app_instances;
pub mod user_actions;
pub mod optimistic_state;
pub mod state;
pub mod objects;
pub mod object_versions;
pub mod app_instance_kv_string;
pub mod app_instance_kv_binary;
pub mod object_lock_queue;
pub mod lock_request_bundle;
pub mod jobs;

// Re-export entities for convenience
pub use app_instances::Entity as AppInstances;
pub use user_actions::Entity as UserActions;
pub use optimistic_state::Entity as OptimisticState;
pub use state::Entity as State;
pub use objects::Entity as Objects;
pub use object_versions::Entity as ObjectVersions;
pub use app_instance_kv_string::Entity as AppInstanceKvString;
pub use app_instance_kv_binary::Entity as AppInstanceKvBinary;
pub use object_lock_queue::Entity as ObjectLockQueue;
pub use lock_request_bundle::Entity as LockRequestBundle;
pub use jobs::Entity as Jobs;