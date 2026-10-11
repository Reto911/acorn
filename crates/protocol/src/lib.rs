//! Shared messages and generated client/server code for the Worker protocol.

/// Worker protocol versions shared by the backend and Worker.
pub mod worker {
    /// Generated messages and gRPC interfaces for `acorn.worker.v1`.
    #[allow(
        missing_docs,
        clippy::missing_docs_in_private_items,
        reason = "Tonic/Prost output is maintained by the code generator."
    )]
    pub mod v1 {
        tonic::include_proto!("acorn.worker.v1");
    }
}
