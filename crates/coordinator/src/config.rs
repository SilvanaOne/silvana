#[derive(Debug, Clone)]
pub struct Config {
    pub rpc_url: String,
    pub package_id: String,
    pub modules: Vec<String>,
    pub use_tee: bool,
    pub container_timeout_secs: u64,
}