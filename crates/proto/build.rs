use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let descriptor_path = PathBuf::from("proto/events_descriptor.bin");

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

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(descriptor_path)
        .compile_protos(&proto_files, &proto_includes)?;

    // Tell cargo to recompile if any .proto files change
    if std::path::Path::new("./proto/").exists() {
        println!("cargo:rerun-if-changed=./proto/");
    } else {
        println!("cargo:rerun-if-changed=../../proto/");
    }

    Ok(())
}
