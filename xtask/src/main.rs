use clap::{Parser, Subcommand};
use duct::cmd;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build Docker images for every binary
    DockerBuild,
    /// Wipe & re-apply TiDB migrations on the local dev container
    DbReset,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::DockerBuild => {
            println!("Building Docker images...");
            //cmd!("docker", "build", "-f", "docker/rpc.Dockerfile", ".").run()?;
            //cmd!("docker", "build", "-f", "docker/coordinator.Dockerfile", ".").run()?;
        }
        Cmd::DbReset => {
            println!("Resetting database...");
            cmd!(
                "docker",
                "exec",
                "tidb",
                "sh",
                "-c",
                "source /migrations/reset.sql"
            )
            .run()?;
        }
    }
    Ok(())
}
