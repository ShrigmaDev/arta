use std::sync::RwLock;

use reqwest::StatusCode;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_with::skip_serializing_none;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize)]
struct TransmissionRequest {
    method: Method,
    arguments: Option<Args>,
}

pub trait ResponseArgs {}

#[derive(Deserialize, Serialize)]
pub struct TransmissionResponse<ResponseArgs> {
    arguments: ResponseArgs,
    result: String,
}

#[derive(Serialize)]
enum Method {
    #[serde(rename = "session-get")]
    SessionGet,
    #[serde(rename = "torrent-add")]
    TorrentAdd,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Args {
    SessionGet(SessionGetArgs),
    TorrentAdd(TorrentAddArgs),
}

// TODO: (from spec) Either filename or metainfo must be included. All other arguments are optional  (OR just let user decide)
// TODO: cookies are supposed to have a particular format, maybe enforce through types/serde? or just let user provide in string format
#[skip_serializing_none]
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub struct TorrentAddArgs {
    cookies: Option<String>,
    download_dir: Option<String>,
    filename: Option<String>,
    labels: Option<String>,
    metainfo: Option<String>,
    paused: Option<String>,
    peer_limit: Option<u32>,
    bandwidth_priority: Option<u32>, // -1, 0, 1 for LOW MEDIUM HIGH priority torrent
    files_wanted: Option<Vec<u32>>,
    files_unwanted: Option<Vec<u32>>,
    priority_high: Option<Vec<u32>>,
    priority_low: Option<Vec<u32>>,
    priority_normal: Option<Vec<u32>>,
}

// #[skip_serializing_none]
#[derive(Deserialize, Serialize)]
pub struct Torrent {
    name: Option<String>,
    id: Option<u32>,
    #[serde(rename = "hashString")]
    hash_string: Option<String>,
}

#[skip_serializing_none]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct TorrentAdd {
    torrent_added: Option<Torrent>,
    torrent_duplicate: Option<Torrent>,
}

impl ResponseArgs for TorrentAdd {}

#[derive(Serialize)]
struct SessionGetArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<SessionGetFields>>,
}

#[derive(Serialize)]
pub enum SessionGetFields {
    #[serde(rename = "rpc-version")]
    RPCVersion,
    #[serde(rename = "config-dir")]
    ConfigDir,
}

#[skip_serializing_none]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SessionGet {
    rpc_version: Option<u32>,
    config_dir: Option<String>,
}
impl ResponseArgs for SessionGet {}

pub struct Client {
    url: String,
    session_id: RwLock<Option<String>>,
    http_client: reqwest::Client,
}

impl Client {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            session_id: None.into(),
            http_client: reqwest::Client::new(),
        }
    }

    async fn request<T: ResponseArgs + DeserializeOwned>(
        &self,
        method: Method,
        arguments: Option<Args>,
    ) -> Result<TransmissionResponse<T>> {
        let data = TransmissionRequest { method, arguments };
        // change to logging (tracing crate?)
        println!("Sending request: {}", serde_json::to_string(&data).unwrap());

        const RETRIES: u8 = 5;
        for _retry in 0..RETRIES {
            let mut request = self.http_client.post(&self.url);
            if let Some(session_id) = self.session_id.read().unwrap().as_deref() {
                request = request.header("X-Transmission-Session-id", session_id);
            }
            request = request.json(&data);
            let response = request.send().await?;
            println!("status code = {}", response.status());
            if response.status() == StatusCode::CONFLICT {
                *self.session_id.write().unwrap() = Some(
                    response.headers()["X-Transmission-Session-id"]
                        .to_str()
                        .unwrap()
                        .to_owned(),
                );
                continue;
            }
            let deserialized_response: TransmissionResponse<T> = response.json().await?;
            return Ok(deserialized_response);
        }
        Err(format!(
            "Failed after {} retries to send request to transmission server",
            RETRIES
        )
        .into())
    }

    pub async fn session_get(
        &mut self,
        fields: Option<Vec<SessionGetFields>>,
    ) -> Result<TransmissionResponse<SessionGet>> {
        self.request(
            Method::SessionGet,
            Some(Args::SessionGet(SessionGetArgs { fields })),
        )
        .await
    }

    pub async fn torrent_add(
        &mut self,
        args: TorrentAddArgs,
    ) -> Result<TransmissionResponse<TorrentAdd>> {
        self.request(Method::TorrentAdd, Some(Args::TorrentAdd(args)))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_get() {
        let url = "http://127.0.0.1:9091/transmission/rpc".to_owned();
        let mut trans_client = Client::new(&url);
        let res = trans_client
            .session_get(Some(vec![SessionGetFields::RPCVersion]))
            .await;
        match res {
            Ok(res) => {
                println!("Got response: {}", serde_json::to_string(&res).unwrap());
            }
            Err(e) => {
                dbg!(e);
            }
        };
    }

    #[tokio::test]
    async fn test_torrent_add() {
        let url = "http://127.0.0.1:9091/transmission/rpc".to_owned();
        let mut trans_client = Client::new(&url);
        let arch_iso_magnet = "magnet:?xt=urn:btih:7a9c4a72e79fcf5f65f091e462b60e589af3f865&dn=archlinux-2023.08.01-x86_64.iso".to_owned();
        let res = trans_client
            .torrent_add(TorrentAddArgs {
                filename: Some(arch_iso_magnet),
                ..Default::default()
            })
            .await;
        match res {
            Ok(res) => {
                println!("Got response: {}", serde_json::to_string(&res).unwrap());
            }
            Err(e) => {
                dbg!(e);
            }
        };
    }
}
