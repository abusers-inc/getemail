use std::sync::Arc;

use chrono::{DateTime, Utc};
use proxied::Proxy;
use rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;

use crate::Conn;
fn create_connector() -> TlsConnector {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect(),
    };

    let config = rustls::client::ClientConfig::builder()
        .with_root_certificates(Arc::new(root_store))
        .with_no_client_auth();

    TlsConnector::from(Arc::new(config))
}

pub(crate) async fn connect_maybe_proxied_stream_tls(
    domain: String,
    port: u16,
    proxy: Option<Proxy>,
) -> eyre::Result<Box<dyn Conn>> {
    let connector = create_connector();

    let tunnel: Box<dyn Conn> = match proxy {
        Some(proxy) => Box::new(
            proxy
                .connect_tcp(proxied::NetworkTarget::Domain {
                    domain: domain.clone(),
                    port,
                })
                .await?,
        ),
        None => {
            let mut resolved = tokio::net::lookup_host((domain.clone(), port)).await?;
            let mut mail_socket = resolved
                .next()
                .ok_or_else(|| eyre::eyre!("mail lookup failed"))?;
            mail_socket.set_port(port);

            let stream = tokio::net::TcpStream::connect(mail_socket).await?;
            stream.set_nodelay(true)?;
            stream.set_linger(None)?;

            Box::new(stream)
        }
    };
    let stream = connector
        .connect(ServerName::try_from(domain.clone()).unwrap(), tunnel)
        .await?;

    Ok(Box::new(stream))
}
