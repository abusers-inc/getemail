use std::{
    borrow::Cow,
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::{Arc, LazyLock},
};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProtocolEndpoint {
    pub domain: String,
    pub port: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct Imap(pub Endpoint);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct Pop3(pub Endpoint);

pub type Endpoint = ProtocolEndpoint;
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum Endpoints {
    Pop3 { pop3: Pop3 },

    Imap { imap: Imap },

    Full { pop3: Pop3, imap: Imap },
}

impl Endpoints {
    pub fn get_pop3(&self) -> Option<&Pop3> {
        match self {
            Endpoints::Pop3 { pop3 } => Some(&pop3),
            Endpoints::Imap { .. } => None,
            Endpoints::Full { pop3, .. } => Some(&pop3),
        }
    }
    pub fn get_imap(&self) -> Option<&Imap> {
        match self {
            Endpoints::Imap { imap } => Some(&imap),
            Endpoints::Full { imap, .. } => Some(&imap),
            Endpoints::Pop3 { .. } => None,
        }
    }
}

pub type Domain = String;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Server {
    pub domains: Vec<Domain>,
    #[serde(flatten)]
    pub endpoint: Endpoints,
}

pub struct ServerMap {
    map: HashMap<Domain, &'static Endpoints>,
}

impl ServerMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn add_server(&mut self, server: Server) {
        let endpoint = Box::leak(Box::new(server.endpoint));
        for domain in server.domains {
            self.map.insert(domain, endpoint);
        }
    }

    pub fn get_by_domain<'this>(
        &'this self,
        domain: impl Into<String>,
    ) -> Option<&'static Endpoints> {
        self.map.get(&domain.into()).map(|d| *d)
    }

    pub fn servers(&self) -> Vec<Server> {
        let map = self.map.clone();

        let mut reversed_map: HashMap<Endpoints, Server> = HashMap::new();

        for (domain, endpoint) in map.into_iter() {
            let entry = reversed_map.entry(endpoint.clone());

            entry
                .and_modify(|server| server.domains.push(domain.clone()))
                .or_insert(Server {
                    domains: vec![domain],
                    endpoint: endpoint.clone(),
                });
        }

        reversed_map.into_iter().map(|(_, server)| server).collect()
    }
}

static GLOBAL_ASYNC_MAP: LazyLock<ArcMap> = LazyLock::new(|| ArcMap::new());

#[derive(Clone)]
pub struct ArcMap {
    inner: Arc<RwLock<ServerMap>>,
}

impl ArcMap {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ServerMap::new())),
        }
    }

    pub fn global() -> Self {
        GLOBAL_ASYNC_MAP.clone()
    }

    pub async fn read(&self) -> impl Deref<Target = ServerMap> {
        self.inner.clone().read_owned().await
    }
    pub async fn write(&self) -> impl DerefMut<Target = ServerMap> {
        self.inner.clone().write_owned().await
    }
}
