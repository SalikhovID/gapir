//! Platform-independent logic of Gapir. Nothing here touches the OS UI, audio or input APIs,
//! so it stays unit-testable and reusable when Linux and Windows are added.

pub mod machine;
