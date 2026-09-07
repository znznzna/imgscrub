//! JPEG のセグメント単位の操作。

pub mod filter;
pub mod segment;
pub mod verify;

pub use filter::{decide, Action, DropReason, Options};
pub use segment::{check_eoi, scan, scan_data, App, JpegError, Segment, EOI, SOI, SOS};
pub use verify::{verify, VerifyError};
