//! Core of OpenVClient: the connector contracts every database or infrastructure
//! service implements, plus the shared data types the UI renders.
//!
//! A connector is a [`Driver`] registered in a [`DriverRegistry`]. Connecting
//! yields a [`SessionHandle`], whose variant says which family of operations
//! the service supports (SQL, key-value, containers…). Within a family, each
//! session reports precise [`SqlCapabilities`]-style flags so the UI only
//! offers actions the engine really implements.

pub mod containers;
pub mod driver;
pub mod error;
pub mod explain;
pub mod kv;
pub mod profile;
pub mod registry;
pub mod resource;
pub mod sql;
pub mod sqltext;
pub mod value;

pub use driver::*;
pub use error::{Error, ErrorKind, Result, redact};
pub use profile::*;
pub use registry::DriverRegistry;
pub use value::*;
