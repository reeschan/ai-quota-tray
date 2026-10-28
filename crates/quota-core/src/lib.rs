//! Quota snapshots for the AI services a developer actually uses.
//!
//! Providers return only values present in an official response, a local file
//! the tool itself wrote, or an explicitly enabled unofficial endpoint.
//! Missing data becomes an error string, never a guessed percentage or balance.

#![forbid(unsafe_code)]

pub mod alerts;
pub mod config;
pub mod http;
pub mod model;
pub mod money;
pub mod paths;
pub mod providers;
pub mod schedule;
pub mod secrets;
pub mod service;
pub mod statusline;
pub mod store;
pub mod watch;

pub use alerts::{crossing_alerts, Alert};
pub use config::Config;
pub use http::{HttpClient, HttpRequest, HttpResponse, ReqwestHttp};
pub use model::{display_name, Snapshot, Source, UsageWindow, PROVIDER_ORDER};
pub use money::Money;
pub use providers::{build_providers, Provider};
pub use secrets::{KeyringStore, MemorySecretStore, SecretStore, SECRET_KEYS};
pub use service::fetch_enabled;
pub use store::SnapshotStore;
