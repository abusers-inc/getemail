use mail_parser::{Message, MessageParser};
use proxied::Proxy;
use tokio::io::AsyncWrite;

pub trait Conn: tokio::io::AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug {}
impl<T: tokio::io::AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug> Conn for T {}

self_cell::self_cell!(
    pub struct OwnedMessage {
        owner: Vec<u8>,

        #[covariant]
        dependent: Message,
    }

    impl {Debug}
);

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ProtocolEndpoint {
    pub domain: String,
    pub port: u16,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ProtocolsConn {
    pub imap: ProtocolEndpoint,
    pub pop3: ProtocolEndpoint,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum AuthorizationMechanism {
    Password(String),
    OAuth2(String),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Mailbox {
    pub login: String,
    pub auth: AuthorizationMechanism,
    pub protocols: ProtocolsConn,
    pub proxy: Option<Proxy>,
}

#[async_trait::async_trait]
pub trait DynEmailReader: Send {
    async fn dyn_get_filtered_emails(
        &mut self,
        filter: Option<Box<dyn Filter>>,
    ) -> anyhow::Result<Vec<OwnedMessage>>;
}

mod _obj_safety_guard {
    pub fn _test(input: Box<dyn super::DynEmailReader>) {}
}

pub trait EmailReader: DynEmailReader {
    fn get_filtered_emails(
        &mut self,
        filter: Option<impl Filter>,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<OwnedMessage>>> + Send;
}

pub trait Email: EmailReader {
    type IdleHandle: IdleHandle<Output = Self>;

    fn idlize(self) -> impl std::future::Future<Output = anyhow::Result<Self::IdleHandle>> + Send;
}

pub trait Connector {
    type Protocol: Email;
    fn connect(
        mailbox: Mailbox,
    ) -> impl std::future::Future<Output = anyhow::Result<Self::Protocol>> + Send;
}

pub trait IdleHandle: Send {
    type Output;
    fn done(self) -> impl std::future::Future<Output = anyhow::Result<Self::Output>> + Send;
}

pub trait Filter: Send + Sync {
    fn filter(&self, msg: &OwnedMessage) -> bool;
}

pub mod filters;

mod common;
pub mod imap_protocol;
pub mod pop3_protocol;

pub use common::*;
