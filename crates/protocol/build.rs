//! Generate Worker protocol messages and gRPC interfaces during the Cargo build.

/// Regenerate the shared protocol when its source or compiler selection changes.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let worker_proto = "../../proto/acorn/worker/v1/worker.proto";
    println!("cargo:rerun-if-changed={worker_proto}");
    println!("cargo:rerun-if-env-changed=PROTOC");

    tonic_prost_build::configure().compile_protos(&[worker_proto], &["../../proto"])?;
    Ok(())
}
