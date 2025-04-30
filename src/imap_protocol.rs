use async_imap::types::NameAttribute;
use futures::StreamExt;
use mail_parser::MessageParser;
use proxied::Proxy;

use crate::{
    common,
    server_map::{self},
    Conn, DynEmailReader, Error, Filter, Mailbox, OwnedMessage,
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

struct OAuth2<'a> {
    pub login: &'a str,
    pub token: &'a str,
}

impl<'a> async_imap::Authenticator for OAuth2<'a> {
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
    ) -> Result<Vec<OwnedMessage>, Error> {
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
                .ok_or(Error::MessageParseFailed)?;

            if filter.filter(&new_msg) {
                res.push(new_msg);
            }
        }

        Ok(res)
    }

    async fn get_folders(&mut self) -> Result<Vec<String>, Error> {
        let mut server_folders = vec![];

        let mut stream = self.session.list(None, Some("*")).await?;

        while let Some(item) = stream.next().await {
            match item {
                Ok(item) => {
                    let is_ok_folder = item
                        .attributes()
                        .iter()
                        .all(|x| ![NameAttribute::Sent, NameAttribute::Drafts].contains(x));

                    if is_ok_folder {
                        server_folders.push(item.name().to_string());
                    }
                }

                Err(err) => {
                    return Err(Error::Imap(err));
                }
            }
        }

        Ok(server_folders)
    }
    pub async fn crawl_folders(
        &mut self,
        folders: &Vec<String>,
        filter: &impl Filter,
    ) -> Result<Vec<OwnedMessage>, Error> {
        let mut res = Vec::new();

        for folder in folders.iter() {
            res.extend(self.crawl_messages(folder, filter).await?.into_iter());
        }

        Ok(res)
    }

    async fn get_filtered_emails(
        &mut self,
        filter: impl Filter,
    ) -> Result<Vec<OwnedMessage>, Error> {
        let folders = self.get_folders().await?;

        self.crawl_folders(&folders, &filter).await
    }
}

#[async_trait::async_trait]
impl DynEmailReader for ImapProtocol {
    async fn dyn_get_filtered_emails(
        &mut self,
        filter: Box<dyn Filter>,
    ) -> Result<Vec<OwnedMessage>, Error> {
        self.get_filtered_emails(filter).await
    }
}

pub struct ImapConnector;
impl ImapConnector {
    pub async fn connect(
        mailbox: Mailbox,
        server_map::Imap(endpoint): &server_map::Imap,
        proxy: Option<Proxy>,
    ) -> Result<ImapProtocol, Error> {
        let stream = common::connect_maybe_proxied_stream_tls(
            endpoint.domain.clone(),
            endpoint.port,
            proxy.clone(),
        )
        .await?;

        let mut client: async_imap::Client<Box<dyn Conn>> =
            async_imap::Client::new(Box::new(stream));

        client.run_command_and_check_ok("CAPABILITY", None).await?;

        let client = match mailbox.oauth2.as_ref() {
            None => {
                client
                    .authenticate(
                        "PLAIN",
                        PlainAuth {
                            login: mailbox.email.clone(),
                            password: mailbox.password,
                        },
                    )
                    .await
            }
            Some(oauth) => {
                client
                    .authenticate(
                        "XOAUTH2",
                        OAuth2 {
                            login: &mailbox.email,
                            token: &oauth.access.token,
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
