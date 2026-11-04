//! Platform-independent logic of Gapir. Nothing here touches the OS UI, audio or input APIs,
//! so it stays unit-testable and reusable when Linux and Windows are added.

pub mod chunker;
pub mod delivery;
pub mod engine;
pub mod machine;
pub mod model_store;
pub mod settings;
pub mod textfmt;
pub mod version;
