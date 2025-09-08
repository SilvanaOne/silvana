pub mod config;
pub mod error;
pub mod register;
pub mod task;
pub mod monitor;
pub mod status;
pub mod cli;

pub use config::{Config, DeploymentAddresses, load_deployment_data, load_abi};
pub use error::{AvsOperatorError, Result};
pub use register::{register_operator, is_operator_registered};
pub use task::{
    create_task,
    create_single_task,
    start_creating_tasks,
    generate_random_task_name,
};
pub use monitor::{
    monitor_new_tasks,
    monitor_new_tasks_polling,
    sign_and_respond_to_task,
};
pub use status::{
    get_operator_status,
    get_operator_details,
};