use std::{
    io::Read,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use async_imap::types::NameAttribute;
use futures::StreamExt;
use mail_parser::MessageParser;
use proxied::Proxy;

use crate::{
    common,
    server_map::{self},
    Conn, DynEmailReader, Filter, Mailbox, OwnedMessage,
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
        filter: &impl Filter,
    ) -> eyre::Result<Vec<OwnedMessage>> {
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

            let new_msg = msg_parser
                .parse(body.as_slice())
                .map(|x| x.into_owned())
                .ok_or(eyre::eyre!("message parsing failed"))?;

            if filter.filter(&new_msg) {
                res.push(new_msg);
            }
        }

        Ok(res)
    }

    async fn get_folders(&mut self) -> eyre::Result<Vec<String>> {
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
        filter: &impl Filter,
    ) -> eyre::Result<Vec<OwnedMessage>> {
        let mut res = Vec::new();

        for folder in folders.iter() {
            res.extend(self.crawl_messages(folder, filter).await?.into_iter());
        }

        Ok(res)
    }

    async fn get_filtered_emails(
        &mut self,
        filter: impl Filter,
    ) -> eyre::Result<Vec<OwnedMessage>> {
        let folders = self.get_folders().await?;

        self.crawl_folders(&folders, &filter).await
    }
}

#[async_trait::async_trait]
impl DynEmailReader for ImapProtocol {
    async fn dyn_get_filtered_emails(
        &mut self,
        filter: Box<dyn Filter>,
    ) -> eyre::Result<Vec<OwnedMessage>> {
        self.get_filtered_emails(filter).await
    }
}

pub struct ImapConnector;
impl ImapConnector {
    pub async fn connect(
        mailbox: Mailbox,
        server_map::Imap(endpoint): &server_map::Imap,
        proxy: Option<Proxy>,
    ) -> eyre::Result<ImapProtocol> {
        let stream = common::connect_maybe_proxied_stream_tls(
            endpoint.domain.clone(),
            endpoint.port,
            proxy.clone(),
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
                            login: mailbox.email.clone(),
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
                            login: mailbox.email.clone(),
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
