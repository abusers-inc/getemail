use std::borrow::Cow;

use chrono::{DateTime, Utc};
use futures::StreamExt;
use mail_parser::{Message, MessageParser};
use proxied::Proxy;
use tokio::io::AsyncWrite;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EmailProviderData {
    pub imap_domain: String,
    pub imap_port: u16,
    pub folders_to_check: Vec<String>,
}

pub trait Conn: tokio::io::AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug {}
impl<T: tokio::io::AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug> Conn for T {}

pub trait EmailIdleHandle: Send {
    type Output;
    fn done(self) -> impl std::future::Future<Output = anyhow::Result<Self::Output>> + Send;
}

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

pub trait EmailProtocol: Send {
    type IdleHandle: EmailIdleHandle<Output = Self>;
    fn get_filtered_emails(
        &mut self,
        filter: Option<&impl Filter>,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<OwnedMessage>>> + Send;
    fn idlize(self) -> impl std::future::Future<Output = anyhow::Result<Self::IdleHandle>> + Send;
}
pub trait EmailProtocolConnector {
    type Protocol: EmailProtocol;
    fn connect(
        mailbox: Mailbox,
    ) -> impl std::future::Future<Output = anyhow::Result<Self::Protocol>> + Send;
}

pub trait Filter: Send + Sync {
    fn filter(&self, msg: &OwnedMessage) -> bool;
}

#[derive(Debug, Clone)]
pub enum ProtocolKind {
    IMAP,
    POP3,
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

impl Filter for DateFilter {
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

impl Filter for SubjectFilter {
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

impl<First: Filter, Second: Filter> Filter for AndFilter<First, Second> {
    fn filter(&self, msg: &crate::OwnedMessage) -> bool {
        self.first_filter.filter(msg) && self.second_filter.filter(msg)
    }
}

pub struct RegexFilter {
    regex: regex::Regex,
}

impl RegexFilter {
    pub fn new(regex: regex::Regex) -> Self {
        Self { regex }
    }
}

impl Filter for RegexFilter {
    fn filter(&self, msg: &OwnedMessage) -> bool {
        let msg = msg.borrow_dependent();
        let empty = Cow::Owned(String::new());
        let body = msg.body_html(0).unwrap_or(empty);

        self.regex.is_match(body.as_ref())
    }
}

mod common;
pub mod imap_protocol;
pub mod pop3_protocol;

pub use common::*;
