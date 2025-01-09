use async_pop2::response::types::DataType;

use super::*;

pub struct Pop3 {
    client: async_pop2::Client<Box<dyn Conn>>,
    mailbox_info: Mailbox,
}

impl Pop3 {
    async fn get_filtered_emails(
        &mut self,
        filter: impl Filter,
    ) -> Result<Vec<OwnedMessage>, Error> {
        let stat = self.client.stat().await?;
        let total_msg_count = stat.counter().value()?;
        let mut result = Vec::new();
        if total_msg_count < 1 {
            return Ok(result);
        }

        for curr_msg_id in 1..=total_msg_count {
            let bytes = self.client.retr(curr_msg_id).await?.to_vec();

            let parser = MessageParser::new();
            let new_msg = parser
                .parse(bytes.as_slice())
                .ok_or(Error::MessageParseFailed)?
                .into_owned();

            if filter.filter(&new_msg) {
                result.push(new_msg)
            }
        }

        Ok(result)
    }
}

#[async_trait::async_trait]
impl DynEmailReader for Pop3 {
    async fn dyn_get_filtered_emails(
        &mut self,
        filter: Box<dyn Filter>,
    ) -> Result<Vec<OwnedMessage>, Error> {
        self.get_filtered_emails(filter).await
    }
}

pub struct Pop3Connector;
impl Pop3Connector {
    pub async fn connect(
        mailbox: Mailbox,
        server_map::Pop3(endpoint): &server_map::Pop3,
        proxy: Option<Proxy>,
    ) -> Result<Pop3, Error> {
        let stream = common::connect_maybe_proxied_stream_tls(
            endpoint.domain.clone(),
            endpoint.port,
            proxy.clone(),
        )
        .await?;

        let mut client = async_pop2::new(stream).await?;

        match &mailbox.auth {
            AuthorizationMechanism::Password(password) => {
                client.login(&mailbox.email, &password).await?;
            }
            AuthorizationMechanism::OAuth2(token) => {
                let authorizer = async_pop2::sasl::OAuth2Authenticator::new(&mailbox.email, token);
                client.auth(authorizer).await?;
            }
        };

        Ok(Pop3 {
            client,
            mailbox_info: mailbox,
        })
    }
}

pub use filters::*;
