pub mod api;
pub mod client;
pub mod error;
pub mod models;

pub use client::TcgClient;
pub use error::{Result, TcgError};
pub use models::*;
