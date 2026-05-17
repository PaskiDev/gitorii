//! Smart HTTP transport over reqwest (rustls).
//!
//! Implements git's Smart HTTP protocol (v0/v1) as a libgit2 subtransport.
//! Each `Service` maps to one HTTP request:
//!   UploadPackLs    GET  {url}/info/refs?service=git-upload-pack
//!   UploadPack      POST {url}/git-upload-pack
//!   ReceivePackLs   GET  {url}/info/refs?service=git-receive-pack
//!   ReceivePack     POST {url}/git-receive-pack
//!
//! libgit2 owns pkt-line framing and pack negotiation; we just shuttle bytes.

use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::time::Duration;

use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use git2::{Error, Remote};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};

const UA: &str = concat!("torii/", env!("CARGO_PKG_VERSION"));

fn http_timeout() -> Duration {
    let secs = std::env::var("TORII_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(300); // 5 min default
    Duration::from_secs(secs)
}

pub fn factory(remote: &Remote<'_>) -> Result<Transport, Error> {
    Transport::smart(remote, true, HttpsSubtransport::new())
}

struct HttpsSubtransport {
    client: Client,
}

impl HttpsSubtransport {
    fn new() -> Self {
        let client = Client::builder()
            .user_agent(UA)
            // Timeouts so a hung server doesn't freeze torii forever.
            // connect: 10s — a server we can't reach in 10s is down.
            // request: 5min — large pack uploads/downloads need slack;
            //                 callers can override with TORII_HTTP_TIMEOUT_SECS.
            .connect_timeout(Duration::from_secs(10))
            .timeout(http_timeout())
            .build()
            .expect("reqwest client");
        Self { client }
    }
}

impl SmartSubtransport for HttpsSubtransport {
    fn action(
        &self,
        url: &str,
        action: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, Error> {
        let auth = resolve_auth(url);
        let stream = match action {
            Service::UploadPackLs => {
                HttpStream::ls(&self.client, url, "git-upload-pack", auth)?
            }
            Service::ReceivePackLs => {
                HttpStream::ls(&self.client, url, "git-receive-pack", auth)?
            }
            Service::UploadPack => {
                HttpStream::rpc(self.client.clone(), url, "git-upload-pack", auth)
            }
            Service::ReceivePack => {
                HttpStream::rpc(self.client.clone(), url, "git-receive-pack", auth)
            }
        };
        Ok(Box::new(stream))
    }

    fn close(&self) -> Result<(), Error> {
        Ok(())
    }
}

/// Bidirectional stream bridging libgit2's Read/Write to one HTTP request.
///
/// Two modes:
/// - `Ls`: GET completed at construction; reads stream the response body.
/// - `Rpc`: POST deferred. Writes buffer the request body. First read flushes
///   the buffer as the POST and then streams the response.
struct HttpStream {
    inner: Mutex<Inner>,
}

enum Inner {
    /// info/refs response — already fetched, just stream out.
    Ls { resp: Response },
    /// upload-pack / receive-pack — buffer writes, fire on first read.
    Rpc {
        client: Client,
        url: String,
        service: &'static str,
        auth: Option<String>,
        sent: bool,
        req_body: Vec<u8>,
        resp: Option<Response>,
    },
}

impl HttpStream {
    fn ls(
        client: &Client,
        base_url: &str,
        service: &str,
        auth: Option<String>,
    ) -> Result<Self, Error> {
        let url = format!("{}/info/refs?service={}", base_url.trim_end_matches('/'), service);
        let mut req = client.get(&url).header(USER_AGENT, UA).header(ACCEPT, "*/*");
        if let Some(a) = &auth {
            req = req.header(AUTHORIZATION, a);
        }
        let resp = req.send().map_err(io_err)?;
        let resp = check_status(resp, base_url, auth.is_some())?;
        Ok(Self {
            inner: Mutex::new(Inner::Ls { resp }),
        })
    }

    fn rpc(
        client: Client,
        base_url: &str,
        service: &'static str,
        auth: Option<String>,
    ) -> Self {
        let url = format!("{}/{}", base_url.trim_end_matches('/'), service);
        Self {
            inner: Mutex::new(Inner::Rpc {
                client,
                url,
                service,
                auth,
                sent: false,
                req_body: Vec::new(),
                resp: None,
            }),
        }
    }
}

impl Read for HttpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        match &mut *inner {
            Inner::Ls { resp } => resp.read(buf),
            Inner::Rpc {
                client,
                url,
                service,
                auth,
                sent,
                req_body,
                resp,
            } => {
                if !*sent {
                    let body = std::mem::take(req_body);
                    let req_ct = format!("application/x-{}-request", service);
                    let resp_ct = format!("application/x-{}-result", service);
                    let mut req = client
                        .post(url.as_str())
                        .header(USER_AGENT, UA)
                        .header(CONTENT_TYPE, req_ct)
                        .header(ACCEPT, resp_ct)
                        .body(body);
                    if let Some(a) = auth.as_deref() {
                        req = req.header(AUTHORIZATION, a);
                    }
                    let r = req.send().map_err(to_io)?;
                    let r = check_status(r, url, auth.is_some())
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
                    *resp = Some(r);
                    *sent = true;
                }
                resp.as_mut().unwrap().read(buf)
            }
        }
    }
}

impl Write for HttpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        match &mut *inner {
            Inner::Ls { .. } => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "cannot write to Ls stream",
            )),
            Inner::Rpc { req_body, sent, .. } => {
                if *sent {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "cannot write after read started",
                    ));
                }
                req_body.extend_from_slice(buf);
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn io_err(e: reqwest::Error) -> Error {
    Error::from_str(&format!("https transport: {}", e))
}

fn to_io(e: reqwest::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}

/// Translate auth-relevant HTTP statuses into actionable errors before libgit2
/// sees an opaque "transport error". On success returns the response unchanged.
fn check_status(resp: Response, base_url: &str, had_auth: bool) -> Result<Response, Error> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let host = host_of(base_url).unwrap_or_else(|| "remote".to_string());
    let msg = match status.as_u16() {
        401 if !had_auth => format!(
            "{} requires auth (HTTP 401). Set {} or TORII_HTTPS_TOKEN.",
            host,
            env_var_name_for(&host).unwrap_or("a host token")
        ),
        401 => format!(
            "{} rejected the credentials (HTTP 401). Check token scope and validity.",
            host
        ),
        403 => format!(
            "{} forbade the request (HTTP 403). Token may lack scope or repo is restricted.",
            host
        ),
        404 => format!(
            "{} returned 404. Repo may not exist or token cannot see it.",
            host
        ),
        s => format!("{} returned HTTP {}", host, s),
    };
    Err(Error::from_str(&format!("https transport: {}", msg)))
}

fn env_var_name_for(host: &str) -> Option<&'static str> {
    Some(match host {
        h if h.contains("github.") => "GITHUB_TOKEN",
        h if h.contains("gitlab.") => "GITLAB_TOKEN",
        h if h.contains("codeberg.") => "CODEBERG_TOKEN",
        h if h.contains("bitbucket.") => "BITBUCKET_TOKEN",
        h if h.contains("gitea.") => "GITEA_TOKEN",
        h if h.contains("forgejo.") => "FORGEJO_TOKEN",
        h if h.contains("sr.ht") || h.contains("sourcehut.") => "SOURCEHUT_TOKEN",
        _ => return None,
    })
}

/// Resolve an `Authorization` header value for the given URL.
///
/// Lookup order (first match wins):
/// 1. Host-specific env var: GITHUB_TOKEN, GITLAB_TOKEN, CODEBERG_TOKEN,
///    GITEA_TOKEN, BITBUCKET_TOKEN, FORGEJO_TOKEN, SOURCEHUT_TOKEN
/// 2. Generic fallback: TORII_HTTPS_TOKEN
/// 3. None — request goes anonymous (works for public repos)
///
/// Returns Basic auth with user `x-access-token` and token as password —
/// the most portable form across GitHub, GitLab, Codeberg, Gitea, Forgejo.
fn resolve_auth(url: &str) -> Option<String> {
    let host = host_of(url)?;
    // Token precedence is now centralised in `crate::auth::resolve_token`:
    //   per-host env var > generic TORII_HTTPS_TOKEN > local repo store
    //   > global store (~/.config/torii/auth.toml).
    // Set with:  torii auth set <provider> <token>  (since 0.7.1)
    let token = env_token_for(&host)
        .or_else(|| std::env::var("TORII_HTTPS_TOKEN").ok())
        .or_else(|| config_token_for(&host))?;
    Some(basic_auth("x-access-token", &token))
}

fn config_token_for(host: &str) -> Option<String> {
    let provider = if host.contains("github.") {
        "github"
    } else if host.contains("gitlab.") {
        "gitlab"
    } else if host.contains("codeberg.") {
        "codeberg"
    } else if host.contains("gitea.") {
        "gitea"
    } else if host.contains("forgejo.") {
        "forgejo"
    } else if host.contains("bitbucket.") {
        "bitbucket"
    } else {
        return None;
    };
    // Go through the unified resolver so local-repo overrides and env
    // var precedence are honoured here too — used to be a direct config
    // read that ignored both.
    crate::auth::resolve_token(provider, ".").value
}

fn host_of(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host_port = after_scheme.split('/').next().unwrap_or("");
    let host = host_port.split(':').next().unwrap_or("");
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn env_token_for(host: &str) -> Option<String> {
    let var = match host {
        h if h.contains("github.") => "GITHUB_TOKEN",
        h if h.contains("gitlab.") => "GITLAB_TOKEN",
        h if h.contains("codeberg.") => "CODEBERG_TOKEN",
        h if h.contains("bitbucket.") => "BITBUCKET_TOKEN",
        h if h.contains("gitea.") => "GITEA_TOKEN",
        h if h.contains("forgejo.") => "FORGEJO_TOKEN",
        h if h.contains("sr.ht") || h.contains("sourcehut.") => "SOURCEHUT_TOKEN",
        _ => return None,
    };
    std::env::var(var).ok().filter(|s| !s.is_empty())
}

fn basic_auth(user: &str, pass: &str) -> String {
    let raw = format!("{}:{}", user, pass);
    format!("Basic {}", base64_encode(raw.as_bytes()))
}

/// Minimal RFC 4648 base64 encoder (standard alphabet, with padding).
fn base64_encode(input: &[u8]) -> String {
    const TAB: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | input[i + 2] as u32;
        out.push(TAB[((n >> 18) & 0x3F) as usize] as char);
        out.push(TAB[((n >> 12) & 0x3F) as usize] as char);
        out.push(TAB[((n >> 6) & 0x3F) as usize] as char);
        out.push(TAB[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(TAB[((n >> 18) & 0x3F) as usize] as char);
        out.push(TAB[((n >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(TAB[((n >> 18) & 0x3F) as usize] as char);
        out.push(TAB[((n >> 12) & 0x3F) as usize] as char);
        out.push(TAB[((n >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_extraction() {
        assert_eq!(host_of("https://github.com/owner/repo").as_deref(), Some("github.com"));
        assert_eq!(
            host_of("https://gitlab.example.com:8443/group/repo").as_deref(),
            Some("gitlab.example.com")
        );
        assert_eq!(host_of("https://codeberg.org/foo/bar").as_deref(), Some("codeberg.org"));
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn basic_auth_format() {
        // x-access-token:secret → eC1hY2Nlc3MtdG9rZW46c2VjcmV0
        assert_eq!(basic_auth("x-access-token", "secret"), "Basic eC1hY2Nlc3MtdG9rZW46c2VjcmV0");
    }
}
