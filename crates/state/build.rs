use prost_build::Config;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let state_descriptor_path =
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("state_descriptor.bin");

    // Determine proto file paths - check for Docker environment first, then local development
    let (proto_files, proto_includes) = if std::path::Path::new("./proto/state.proto").exists() {
        // Docker environment - proto files are in ./proto/
        (
            vec!["./proto/state.proto"],
            vec!["./proto"],
        )
    } else {
        // Local development - proto files are in ../../proto/
        (
            vec!["../../proto/silvana/state/v1/state.proto"],
            vec!["../../proto"],
        )
    };

    // Configure tonic-build for gRPC services
    let mut config = Config::new();
    config.protoc_arg("--experimental_allow_proto3_optional");

    // Enable prost-reflect for runtime reflection
    prost_reflect_build::Builder::new()
        .descriptor_pool("crate::DESCRIPTOR_POOL")
        .configure(&mut config, &proto_files, &proto_includes)
        .expect("Failed to configure reflection for state proto");

    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(&state_descriptor_path)
        .compile_protos_with_config(config, &proto_files, &proto_includes)?;

    // Tell cargo to recompile if any .proto files change
    if std::path::Path::new("./proto/").exists() {
        println!("cargo:rerun-if-changed=./proto/");
    } else {
        println!("cargo:rerun-if-changed=../../proto/silvana/state/");
    }

    Ok(())
}