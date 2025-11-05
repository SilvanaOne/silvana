//! Agent logging utilities that mirror TypeScript SDK functionality

use crate::client::CoordinatorClient;
use crate::error::Result;
use crate::proto::coordinator::LogLevel;
use once_cell::sync::OnceCell;

/// Global agent instance for logging
static AGENT: OnceCell<Agent> = OnceCell::new();

/// Agent logging interface
/// Provides methods to log messages to both the console and coordinator
pub struct Agent;

impl Agent {
    /// Initialize the global agent instance
    pub fn init() {
        AGENT.set(Agent).ok();
    }

    /// Get the global agent instance
    pub fn global() -> &'static Agent {
        AGENT.get_or_init(|| Agent)
    }

    /// Log a debug message
    pub async fn debug(&self, message: impl AsRef<str>) -> Result<()> {
        let msg = message.as_ref();
        tracing::debug!("{}", msg);

        if let Ok(client) = CoordinatorClient::global().await {
            let mut client = client.write();
            client.agent_message(LogLevel::Debug, msg).await.ok();
        }

        Ok(())
    }

    /// Log an info message
    pub async fn info(&self, message: impl AsRef<str>) -> Result<()> {
        let msg = message.as_ref();
        tracing::info!("{}", msg);

        if let Ok(client) = CoordinatorClient::global().await {
            let mut client = client.write();
            client.agent_message(LogLevel::Info, msg).await.ok();
        }

        Ok(())
    }

    /// Log a warning message
    pub async fn warn(&self, message: impl AsRef<str>) -> Result<()> {
        let msg = message.as_ref();
        tracing::warn!("{}", msg);

        if let Ok(client) = CoordinatorClient::global().await {
            let mut client = client.write();
            client.agent_message(LogLevel::Warn, msg).await.ok();
        }

        Ok(())
    }

    /// Log an error message
    pub async fn error(&self, message: impl AsRef<str>) -> Result<()> {
        let msg = message.as_ref();
        tracing::error!("{}", msg);

        if let Ok(client) = CoordinatorClient::global().await {
            let mut client = client.write();
            client.agent_message(LogLevel::Error, msg).await.ok();
        }

        Ok(())
    }

    /// Log a fatal message
    pub async fn fatal(&self, message: impl AsRef<str>) -> Result<()> {
        let msg = message.as_ref();
        tracing::error!("FATAL: {}", msg);

        if let Ok(client) = CoordinatorClient::global().await {
            let mut client = client.write();
            client.agent_message(LogLevel::Fatal, msg).await.ok();
        }

        Ok(())
    }
}

// Global convenience functions that mirror TypeScript SDK

/// Log a debug message using the global agent
pub async fn debug(message: impl AsRef<str>) -> Result<()> {
    Agent::global().debug(message).await
}

/// Log an info message using the global agent
pub async fn info(message: impl AsRef<str>) -> Result<()> {
    Agent::global().info(message).await
}

/// Log a warning message using the global agent
pub async fn warn(message: impl AsRef<str>) -> Result<()> {
    Agent::global().warn(message).await
}

/// Log an error message using the global agent
pub async fn error(message: impl AsRef<str>) -> Result<()> {
    Agent::global().error(message).await
}

/// Log a fatal message using the global agent
pub async fn fatal(message: impl AsRef<str>) -> Result<()> {
    Agent::global().fatal(message).await
}

/// Macro for formatting agent log messages similar to TypeScript console.log
#[macro_export]
macro_rules! agent_log {
    (debug, $($arg:tt)*) => {
        $crate::agent::debug(&format!($($arg)*)).await
    };
    (info, $($arg:tt)*) => {
        $crate::agent::info(&format!($($arg)*)).await
    };
    (warn, $($arg:tt)*) => {
        $crate::agent::warn(&format!($($arg)*)).await
    };
    (error, $($arg:tt)*) => {
        $crate::agent::error(&format!($($arg)*)).await
    };
    (fatal, $($arg:tt)*) => {
        $crate::agent::fatal(&format!($($arg)*)).await
    };
}