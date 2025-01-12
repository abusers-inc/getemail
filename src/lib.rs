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
#[serde(untagged)]
pub enum AuthorizationMechanism {
    Password { password: String },
    OAuth2 { token: String },
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

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("this mailbox is invalid: can't distinguish domain part")]
    MailboxInvalidDomain { login: String },

    #[error("couldn't find server entry for this mailbox: {domain}")]
    ServerNotFound { domain: String },

    #[error("couldn't parse message")]
    MessageParseFailed,

    #[error("Imap-specific error")]
    Imap(#[from] async_imap::error::Error),

    #[error("pop-specific erro")]
    Pop(#[from] async_pop2::error::Error),

    #[error("failed connection to proxy")]
    Proxy(#[from] proxied::ConnectError),

    #[error("failed to resolve dns of email server")]
    ResolveDns,

    #[error("socket failed")]
    Socket(#[from] std::io::Error),
}

/// Dynamic email reader
#[async_trait::async_trait]
pub trait DynEmailReader: Send {
    /// Read a list of emails, matching `filter`
    ///
    /// To not to use any filters, provide `Filters::none().dynamize()` as argument
    async fn dyn_get_filtered_emails(
        &mut self,
        filter: Box<dyn Filter>,
    ) -> Result<Vec<OwnedMessage>, Error>;
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
) -> Result<Box<dyn DynEmailReader>, Error> {
    let domain = mailbox.get_domain().ok_or(Error::MailboxInvalidDomain {
        login: mailbox.email.clone(),
    })?;
    let entry = map.get_by_domain(domain);

    let Some(entry) = entry else {
        return Err(Error::ServerNotFound {
            domain: domain.to_owned(),
        });
    };

    let mut err: Option<Error> = None;
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
) -> Result<Box<dyn DynEmailReader>, Error> {
    let map = ArcMap::global();
    let read_lock = map.read().await;

    connect_any(mailbox, proxy, &read_lock).await
}

pub use filters::*;
