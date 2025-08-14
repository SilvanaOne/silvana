fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(
            &["../../proto/silvana/coordinator/v1/coordinator.proto"],
            &["../../proto"],
        )?;
    Ok(())
}