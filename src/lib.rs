use eyre::OptionExt;
use imap_protocol::ImapConnector;
pub use mail_parser;
use mail_parser::{Message, MessageParser};
use pop3_protocol::Pop3Connector;
use proxied::Proxy;
use server_map::{ArcMap, ServerMap};
use tokio::io::AsyncWrite;

pub mod filters;

mod common;
mod imap_protocol;
mod pop3_protocol;

trait Conn: tokio::io::AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug {}
impl<T: tokio::io::AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug> Conn for T {}

pub mod server_map;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum AuthorizationMechanism {
    Password(String),
    OAuth2(String),
}

/// Credentials of mailbox
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Mailbox {
    #[serde(alias = "login", alias = "username")]
    pub email: String,
    pub auth: AuthorizationMechanism,
}

impl Mailbox {
    /// Credentials of mailbox
    ///
    /// Get domain part of username
    /// someusername@gmail.com
    ///              ^^^^^^^^^
    pub fn get_domain(&self) -> Option<&str> {
        let (_, domain) = self.email.split_once('@')?;
        Some(domain)
    }
}

pub type OwnedMessage = Message<'static>;

/// Dynamic email reader
#[async_trait::async_trait]
pub trait DynEmailReader: Send {
    /// Read a list of emails, matching `filter`
    ///
    /// To not to use any filters, provide `Filters::none().dynamize()` as argument
    async fn dyn_get_filtered_emails(
        &mut self,
        filter: Box<dyn Filter>,
    ) -> eyre::Result<Vec<OwnedMessage>>;
}

mod _obj_safety_guard {
    pub fn _test(_: Box<dyn super::DynEmailReader>) {}
}

/// Connect to any protocol in mailbox
///
/// Tries to extract `domain` and query it's endpoints in provided `ServerMap`
/// Errors if `domain` is not present in `ServerMap`
pub async fn connect_any(
    mailbox: Mailbox,
    proxy: Option<Proxy>,
    map: &ServerMap,
) -> eyre::Result<Box<dyn DynEmailReader>> {
    let entry = map.get_by_domain(
        mailbox
            .get_domain()
            .ok_or_eyre("invalid email: it doesn't have domain")?,
    );

    let Some(entry) = entry else {
        eyre::bail!("this domain connection details are unknown");
    };

    let mut err: Option<eyre::Report> = None;
    if let Some(imap) = entry.get_imap() {
        match ImapConnector::connect(mailbox.clone(), imap, proxy.clone()).await {
            Ok(imap) => return Ok(Box::new(imap)),
            Err(imap_err) => err = Some(imap_err.into()),
        }
    }

    if let Some(pop3) = entry.get_pop3() {
        let conn = Pop3Connector::connect(mailbox.clone(), pop3, proxy.clone()).await?;
        return Ok(Box::new(conn));
    }

    Err(err.unwrap())
}

/// Connect to any protocol in mailbox using global ServerMap
///
/// Tries to extract `domain` and query it's endpoints in global `ServerMap`
/// Errors if `domain` is not present in global `ServerMap`
pub async fn connect_any_global_map(
    mailbox: Mailbox,
    proxy: Option<Proxy>,
) -> eyre::Result<Box<dyn DynEmailReader>> {
    let map = ArcMap::global();
    let read_lock = map.read().await;

    connect_any(mailbox, proxy, &read_lock).await
}

pub use filters::*;
