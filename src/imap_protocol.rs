use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use async_imap::types::NameAttribute;
use futures::StreamExt;
use mail_parser::MessageParser;

use crate::{
    common, Conn, Connector, DynEmailReader, Email, EmailReader, Filter, IdleHandle, Mailbox,
    OwnedMessage,
};

pub struct PlainAuth {
    pub login: String,
    pub password: String,
}

impl async_imap::Authenticator for PlainAuth {
    type Response = String;

    fn process(&mut self, _: &[u8]) -> Self::Response {
        let login_data = format!("\0{}\0{}", self.login.clone(), self.password.clone());
        login_data
    }
}

struct OAuth2 {
    pub login: String,
    pub token: String,
}

impl async_imap::Authenticator for OAuth2 {
    type Response = String;

    fn process(&mut self, _: &[u8]) -> Self::Response {
        let payload = format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            &self.login, &self.token
        );
        payload
    }
}

pub struct ImapProtocol {
    session: async_imap::Session<Box<dyn Conn>>,
}

impl ImapProtocol {
    pub async fn crawl_messages(
        &mut self,
        folder: &str,
        filter: Option<&impl Filter>,
    ) -> anyhow::Result<Vec<OwnedMessage>> {
        let mailbox = self.session.select(folder).await?;
        let fetch_result = self
            .session
            .fetch(format!("1:{}", mailbox.exists), "RFC822")
            .await?
            .collect::<Vec<_>>()
            .await;

        let mut res = Vec::new();

        let msg_parser = MessageParser::new();
        for fetch in fetch_result.into_iter() {
            let fetch = fetch?;
            let body = fetch
                .body()
                .map(|body| body.to_vec())
                .unwrap_or_else(Vec::new);

            let new_msg = OwnedMessage::try_new(body, |x| {
                msg_parser
                    .parse(x)
                    .ok_or(anyhow::anyhow!("message parsing failed"))
            })?;

            if let Some(ref filter) = filter {
                if filter.filter(&new_msg) {
                    res.push(new_msg);
                } else {
                }
            } else {
                res.push(new_msg);
            }
        }

        Ok(res)
    }

    async fn get_folders(&mut self) -> anyhow::Result<Vec<String>> {
        let server_folders: Vec<_> = self
            .session
            .list(None, Some("*"))
            .await?
            .filter_map(|x| async move {
                x.ok()
                    .filter(|x| {
                        x.attributes()
                            .iter()
                            .all(|x| ![NameAttribute::Sent, NameAttribute::Drafts].contains(x))
                    })
                    .map(|x| x.name().to_string())
            })
            .collect()
            .await;
        Ok(server_folders)
    }
    pub async fn crawl_folders(
        &mut self,
        folders: &Vec<String>,
        filter: Option<impl Filter>,
    ) -> anyhow::Result<Vec<OwnedMessage>> {
        let mut res = Vec::new();

        for folder in folders.iter() {
            res.extend(
                self.crawl_messages(folder, filter.as_ref())
                    .await?
                    .into_iter(),
            );
        }

        Ok(res)
    }

    async fn get_filtered_emails(
        &mut self,
        filter: Option<impl Filter>,
    ) -> anyhow::Result<Vec<OwnedMessage>> {
        let folders = self.get_folders().await?;

        self.crawl_folders(&folders, filter.as_ref()).await
    }
}

pub struct ImapIdleHandle {
    handle: tokio::task::JoinHandle<anyhow::Result<async_imap::Session<Box<dyn Conn>>>>,
    stop_flag: Arc<AtomicBool>,
}

impl ImapIdleHandle {
    // pub async fn init(&mut self) -> anyhow::Result<()> {
    //     self.handle.init().await?;
    //     Ok(())
    // }

    pub async fn done(self) -> anyhow::Result<ImapProtocol> {
        self.stop_flag
            .store(true, std::sync::atomic::Ordering::Release);
        let result = self.handle.await?;
        result.map(|x| ImapProtocol { session: x })
    }
}

impl IdleHandle for ImapIdleHandle {
    type Output = ImapProtocol;

    fn done(self) -> impl std::future::Future<Output = anyhow::Result<Self::Output>> + Send {
        async move { self.done().await }
    }
}

#[async_trait::async_trait]
impl DynEmailReader for ImapProtocol {
    fn dyn_get_filtered_emails(
        &mut self,
        filter: Option<Box<dyn Filter>>,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<OwnedMessage>>> + Send {
        self.get_filtered_emails(filter)
    }
}
impl EmailReader for ImapProtocol {
    fn get_filtered_emails(
        &mut self,
        filter: Option<impl Filter>,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<OwnedMessage>>> + Send {
        self.get_filtered_emails(filter)
    }
}
impl Email for ImapProtocol {
    type IdleHandle = ImapIdleHandle;

    fn idlize(self) -> impl std::future::Future<Output = anyhow::Result<Self::IdleHandle>> + Send {
        async move {
            let flag = Arc::new(AtomicBool::new(false));
            let flag_clone = flag.clone();
            let session = self.session;
            let task = || async move {
                let mut session = session;

                while !flag_clone.load(std::sync::atomic::Ordering::Acquire) {
                    session.noop().await?;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Ok(session)
            };

            let task = tokio::task::spawn(task());
            Ok(ImapIdleHandle {
                handle: task,
                stop_flag: flag,
            })
        }
    }
}

pub struct ImapConnector;
impl Connector for ImapConnector {
    type Protocol = ImapProtocol;

    async fn connect(mailbox: Mailbox) -> anyhow::Result<Self::Protocol> {
        let stream = common::connect_maybe_proxied_stream_tls(
            mailbox.protocols.imap.domain.clone(),
            mailbox.protocols.imap.port,
            mailbox.proxy.clone(),
        )
        .await?;

        let mut client: async_imap::Client<Box<dyn Conn>> =
            async_imap::Client::new(Box::new(stream));

        client.run_command_and_check_ok("CAPABILITY", None).await?;

        let client = match mailbox.auth.clone() {
            crate::AuthorizationMechanism::Password(password) => {
                client
                    .authenticate(
                        "PLAIN",
                        PlainAuth {
                            login: mailbox.login.clone(),
                            password,
                        },
                    )
                    .await
            }
            crate::AuthorizationMechanism::OAuth2(token) => {
                client
                    .authenticate(
                        "XOAUTH2",
                        OAuth2 {
                            login: mailbox.login.clone(),
                            token,
                        },
                    )
                    .await
            }
        }
        .map_err(|x| x.0)?;
        // let client = client.login(creds.0, creds.1).await.map_err(|x| x.0)?;

        Ok(ImapProtocol {
            session: client,
            // mail: mailbox.clone(),
        })
    }
}
