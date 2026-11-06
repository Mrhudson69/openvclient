//! Entra ID tokens for SQL Server; the flows live in `openvclient_net::entra`.

pub use openvclient_net::entra::{Cache, DEFAULT_PUBLIC_CLIENT, Flow};

pub async fn acquire(flow: &Flow, notifier: Option<&openvclient_core::Notifier>) -> openvclient_core::Result<(String, u64)> {
    openvclient_net::entra::acquire(flow, openvclient_net::entra::AZURE_SQL, notifier).await
}
