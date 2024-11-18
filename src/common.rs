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

pub async fn connect_maybe_proxied_stream_tls(
    domain: String,
    port: u16,
    proxy: Option<Proxy>,
) -> anyhow::Result<Box<dyn Conn>> {
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
                .ok_or_else(|| anyhow::anyhow!("mail lookup failed"))?;
            mail_socket.set_port(port);

            Box::new(tokio::net::TcpStream::connect(mail_socket).await?)
        }
    };
    let stream = connector
        .connect(ServerName::try_from(domain.clone()).unwrap(), tunnel)
        .await?;

    Ok(Box::new(stream))
}

pub enum DateFilterMode {
    Since,
    Earlier,
}
pub struct DateFilter {
    mode: DateFilterMode,
    date: DateTime<Utc>,
}

impl DateFilter {
    pub fn since(date: DateTime<Utc>) -> Self {
        Self {
            mode: DateFilterMode::Since,
            date,
        }
    }

    pub fn since_now() -> Self {
        Self {
            mode: DateFilterMode::Since,
            date: Utc::now(),
        }
    }
}

impl super::Filter for DateFilter {
    fn filter(&self, msg: &crate::OwnedMessage) -> bool {
        let Some(msg_date) = msg
            .borrow_dependent()
            .date()
            .and_then(|x| DateTime::<Utc>::from_timestamp(x.to_timestamp(), 0))
        else {
            return false;
        };
        match self.mode {
            DateFilterMode::Since => msg_date > self.date,
            DateFilterMode::Earlier => msg_date < self.date,
        }
    }
}

pub struct SubjectFilter {
    subject: String,
}
impl SubjectFilter {
    pub fn new(subject: String) -> Self {
        Self { subject }
    }
}

impl super::Filter for SubjectFilter {
    fn filter(&self, msg: &crate::OwnedMessage) -> bool {
        let Some(msg_subject) = msg.borrow_dependent().subject() else {
            return false;
        };

        msg_subject == self.subject.as_str()
    }
}

pub struct AndFilter<First, Second> {
    first_filter: First,
    second_filter: Second,
}

impl<F, S> AndFilter<F, S> {
    pub fn new(f: F, s: S) -> Self {
        Self {
            first_filter: f,
            second_filter: s,
        }
    }
}

impl<First: super::Filter, Second: super::Filter> super::Filter for AndFilter<First, Second> {
    fn filter(&self, msg: &crate::OwnedMessage) -> bool {
        self.first_filter.filter(msg) && self.second_filter.filter(msg)
    }
}
