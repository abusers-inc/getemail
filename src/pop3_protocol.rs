use anyhow::anyhow;
use async_pop::response::types::DataType;

use super::*;

pub struct Pop3 {
    client: async_pop::Client<Box<dyn Conn>>,
    mailbox_info: Mailbox,
}

pub struct Pop3Handle(Mailbox);

impl EmailIdleHandle for Pop3Handle {
    type Output = Pop3;

    async fn done(self) -> anyhow::Result<Self::Output> {
        let connect = Pop3Connector::connect(self.0).await?;
        Ok(connect)
    }
}

impl EmailProtocol for Pop3 {
    type IdleHandle = Pop3Handle;

    async fn get_filtered_emails(
        &mut self,
        filter: Option<&impl Filter>,
    ) -> anyhow::Result<Vec<OwnedMessage>> {
        let stat = self.client.stat().await?;
        let total_msg_count = stat.counter().value()?;
        let mut result = Vec::new();
        if total_msg_count < 1 {
            return Ok(result);
        }

        for curr_msg_id in 1..=total_msg_count {
            let bytes = self.client.retr(curr_msg_id).await?.to_vec();

            let parser = MessageParser::new();
            let new_msg = OwnedMessage::try_new(bytes, |x| {
                parser.parse(x).ok_or(anyhow!("message parsing failed"))
            })?;

            match filter {
                Some(filter) if filter.filter(&new_msg) => result.push(new_msg),
                _ => {
                    let brw = new_msg.borrow_dependent();
                    tracing::info!(event = "mail.filtered_out", msg = ?brw.subject(), date = ?brw.date());
                }
            }
        }

        Ok(result)
    }

    async fn idlize(mut self) -> anyhow::Result<Self::IdleHandle> {
        self.client.quit().await?;
        Ok(Pop3Handle(self.mailbox_info))
    }
}

pub struct Pop3Connector;
impl EmailProtocolConnector for Pop3Connector {
    type Protocol = Pop3;

    async fn connect(mailbox: Mailbox) -> anyhow::Result<Self::Protocol> {
        let stream = common::connect_maybe_proxied_stream_tls(
            mailbox.protocols.pop3.domain.clone(),
            mailbox.protocols.pop3.port,
            mailbox.proxy.clone(),
        )
        .await?;

        let mut client = async_pop::new(stream).await?;

        match &mailbox.auth {
            AuthorizationMechanism::Password(password) => {
                client.login(&mailbox.login, &password).await?;
            }
            AuthorizationMechanism::OAuth2(token) => {
                let authorizer = async_pop::sasl::OAuth2Authenticator::new(&mailbox.login, token);
                client.auth(authorizer).await?;
            }
        };

        Ok(Pop3 {
            client,
            mailbox_info: mailbox,
        })
    }
}
