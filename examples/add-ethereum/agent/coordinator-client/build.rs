use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Determine proto file paths - check for Docker environment first, then local development
    let (proto_file, proto_dir) = if std::path::Path::new("../proto/silvana/coordinator/v1/coordinator.proto").exists() {
        // Docker environment - proto files are in ../proto/ (copied by Makefile)
        (
            "../proto/silvana/coordinator/v1/coordinator.proto",
            "../proto",
        )
    } else {
        // Local development - proto files are in ../../../../proto/
        (
            "../../../../proto/silvana/coordinator/v1/coordinator.proto",
            "../../../../proto",
        )
    };

    // Check if proto file exists
    let proto_path = PathBuf::from(proto_file);
    if !proto_path.exists() {
        eprintln!("Warning: Proto file not found at {:?}", proto_path);
        eprintln!("Current directory: {:?}", std::env::current_dir()?);
    }

    // Compile the proto file using standard tonic-build
    tonic_build::configure()
        .build_server(false) // We only need the client
        .build_client(true)
        .compile_protos(&[proto_file], &[proto_dir])?;

    // Tell cargo to recompile if the proto file changes
    println!("cargo:rerun-if-changed={}", proto_file);
    println!("cargo:rerun-if-changed={}", proto_dir);

    Ok(())
}
