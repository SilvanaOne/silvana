fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only compile coordinator proto (server only)
    // The events proto client is provided by the proto crate
    tonic_prost_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .build_server(true)
        .build_client(false)
        .compile_protos(
            &["../../proto/silvana/coordinator/v1/coordinator.proto"],
            &["../../proto"],
        )?;
    Ok(())
}