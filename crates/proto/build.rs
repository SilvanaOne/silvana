use prost_build::Config;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let events_descriptor_path =
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("events_descriptor.bin");

    // Determine proto file paths - check for Docker environment first, then local development
    let (proto_files, proto_includes) = if std::path::Path::new("./proto/options.proto").exists() {
        // Docker environment - proto files are in ./proto/
        (
            vec!["./proto/options.proto", "./proto/events.proto"],
            vec!["./proto"],
        )
    } else {
        // Local development - proto files are in ../../proto/
        (
            vec![
                "../../proto/silvana/options/v1/options.proto",
                "../../proto/silvana/events/v1/events.proto",
            ],
            vec!["../../proto"],
        )
    };

    // ------------- configure tonic-build for gRPC services only ----------
    let mut config = Config::new();

    prost_reflect_build::Builder::new()
        .descriptor_pool("crate::DESCRIPTOR_POOL")
        .configure(&mut config, &proto_files, &proto_includes)
        .expect("Failed to configure reflection for protos");

    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(&events_descriptor_path) // write the .bin set
        .compile_protos_with_config(config, &proto_files, &proto_includes)?;

    // tonic_build::configure()
    //     .protoc_arg("--experimental_allow_proto3_optional")
    //     .build_server(true)
    //     .build_client(true)
    //     .file_descriptor_set_path(&events_descriptor_path) // write the .bin set
    //     .compile_protos(&proto_files, &proto_includes)?;

    // prost_reflect_build::Builder::new()
    //     .descriptor_pool("crate::DESCRIPTOR_POOL")
    //     .compile_protos(&proto_files, &proto_includes)
    //     .expect("Failed to compile protos");

    // Tell cargo to recompile if any .proto files change
    if std::path::Path::new("./proto/").exists() {
        println!("cargo:rerun-if-changed=./proto/");
    } else {
        println!("cargo:rerun-if-changed=../../proto/");
    }

    Ok(())
}
