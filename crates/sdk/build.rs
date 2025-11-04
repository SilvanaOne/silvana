use prost_build::Config;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Determine proto file paths - check for environment or relative paths
    let proto_file = "../../proto/silvana/coordinator/v1/coordinator.proto";
    let proto_dir = "../../proto";

    // Check if proto file exists
    let proto_path = PathBuf::from(proto_file);
    if !proto_path.exists() {
        eprintln!("Warning: Proto file not found at {:?}", proto_path);
        eprintln!("Current directory: {:?}", std::env::current_dir()?);
        eprintln!("Looking for proto at: {}", proto_file);
    }

    // Configure prost-build
    let mut config = Config::new();
    config.protoc_arg("--experimental_allow_proto3_optional");

    // Compile the proto file using tonic-prost-build
    tonic_prost_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .build_server(false) // We only need the client
        .build_client(true)
        .compile_with_config(config, &[proto_file], &[proto_dir])?;

    // Tell cargo to recompile if the proto file changes
    println!("cargo:rerun-if-changed={}", proto_file);
    println!("cargo:rerun-if-changed={}", proto_dir);

    Ok(())
}