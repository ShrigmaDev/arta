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
    #[serde(rename = "torrent-get")]
    TorrentGet,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Args {
    SessionGet(SessionGetArgs),
    TorrentAdd(TorrentAddArgs),
    TorrentGet(TorrentGetArgs),
}

// TODO: "format" argument
// TODO: "ids" can also be strings (hashes, 'recently-active' etc) check spec
#[skip_serializing_none]
#[derive(Serialize, Default)]
pub struct TorrentGetArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<TorrentGetFields>>,
    ids: Option<Vec<u32>>,
}

// TODO complete rest of fields
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TorrentGetFields {
    Error,
    ErrorString,
    Eta, // seconds
    HashString,
    Id,
    LeftUntilDone, // bytes
    PercentDone,   // [0..1]
    Name,
    RateDownload, // bytes per sec
    SizeWhenDone, // ?
    TotalSize,    // ?
    Status,       // 0-6, defined in spec
    #[serde(rename = "peer-limit")]
    // i love that some fields are camelCase and some are kebab-case :D THANKS TRANSMISSION
    PeerLimit,
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize)]
pub struct TorrentGet {
    torrents: Vec<Torrent>,
    removed: Option<Vec<Torrent>>,
}

impl ResponseArgs for TorrentGet {}

#[skip_serializing_none]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Torrent {
    error: Option<u32>,
    error_string: Option<String>,
    eta: Option<i32>,
    hash_string: Option<String>,
    id: Option<i32>,
    left_until_done: Option<i32>,
    name: Option<String>,
    #[serde(rename = "peer-limit")]
    peer_limit: Option<i32>,
    percent_done: Option<f32>,
    rate_download: Option<i32>,
    size_when_done: Option<i32>,
    status: Option<i32>,
    total_size: Option<i32>,
}

// TODO: (from spec) Either filename or metainfo must be included. All other arguments are optional  (OR just let user decide)
// TODO: cookies are supposed to have a particular format, maybe enforce through types/serde? or just let user provide in string format
#[skip_serializing_none]
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub struct TorrentAddArgs {
    pub cookies: Option<String>,
    pub download_dir: Option<String>,
    pub filename: Option<String>,
    pub labels: Option<String>,
    pub metainfo: Option<String>,
    pub paused: Option<String>,
    pub peer_limit: Option<u32>,
    pub bandwidth_priority: Option<u32>, // -1, 0, 1 for LOW MEDIUM HIGH priority torrent
    pub files_wanted: Option<Vec<u32>>,
    pub files_unwanted: Option<Vec<u32>>,
    pub priority_high: Option<Vec<u32>>,
    pub priority_low: Option<Vec<u32>>,
    pub priority_normal: Option<Vec<u32>>,
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
        &self,
        fields: Option<Vec<SessionGetFields>>,
    ) -> Result<TransmissionResponse<SessionGet>> {
        self.request(
            Method::SessionGet,
            Some(Args::SessionGet(SessionGetArgs { fields })),
        )
        .await
    }

    pub async fn torrent_add(
        &self,
        args: TorrentAddArgs,
    ) -> Result<TransmissionResponse<TorrentAdd>> {
        self.request(Method::TorrentAdd, Some(Args::TorrentAdd(args)))
            .await
    }

    pub async fn torrent_get(
        &self,
        args: TorrentGetArgs,
    ) -> Result<TransmissionResponse<TorrentGet>> {
        self.request(Method::TorrentGet, Some(Args::TorrentGet(args)))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_get() {
        let url = "http://127.0.0.1:9091/transmission/rpc".to_owned();
        let trans_client = Client::new(&url);
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
        let trans_client = Client::new(&url);
        let arch_iso_magnet = "magnet:?xt=urn:btih:ab6ad7ff24b5ed3a61352a1f1a7811a8c3cc6dde&dn=archlinux-2023.09.01-x86_64.iso".to_owned();
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

    #[tokio::test]
    async fn test_torrent_get() {
        let url = "http://127.0.0.1:9091/transmission/rpc".to_owned();
        let trans_client = Client::new(&url);
        let res = trans_client
            .torrent_get(TorrentGetArgs {
                fields: Some(vec![
                    TorrentGetFields::Name,
                    TorrentGetFields::Eta,
                    TorrentGetFields::PeerLimit,
                ]),
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
