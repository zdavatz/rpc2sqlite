//! HTTP access to the public RPC API at gate.bag.admin.ch.
//!
//! The Angular UI talks to `/rpc/public/api`. Every call must carry the
//! `XSRF-TOKEN` cookie value in an `X-XSRF-TOKEN` header, otherwise the
//! server answers 403. The cookie is issued on the first GET of any UI page.

use reqwest::blocking::Client;
use reqwest::cookie::{CookieStore, Jar};
use reqwest::Url;
use serde_json::Value;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub const BASE: &str = "https://www.gate.bag.admin.ch";
const UI_URL: &str = "https://www.gate.bag.admin.ch/rpc/ui/search/general";
const API: &str = "https://www.gate.bag.admin.ch/rpc/public/api";
const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// A client bound to one XSRF token. Cheap to clone (Arc inside reqwest).
#[derive(Clone)]
pub struct RpcClient {
    http: Client,
    token: String,
}

impl RpcClient {
    /// Fetch the UI page once to obtain the XSRF cookie and build a client.
    pub fn new() -> Result<Self, Error> {
        let jar = Arc::new(Jar::default());
        let http = Client::builder()
            .cookie_provider(jar.clone())
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(120))
            .build()?;

        http.get(UI_URL).send()?.error_for_status()?;

        // The cookie has Path=/rpc, so ask the jar for that path.
        let url = Url::parse(&format!("{}/rpc/", BASE))?;
        let header = jar
            .cookies(&url)
            .ok_or("no cookies received from RPC server")?;
        let header = header.to_str()?;
        let token = header
            .split(';')
            .map(str::trim)
            .find_map(|c| c.strip_prefix("XSRF-TOKEN="))
            .ok_or("XSRF-TOKEN cookie missing in response")?
            .to_string();

        Ok(RpcClient { http, token })
    }

    fn get_json(&self, url: &str) -> Result<Value, Error> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let resp = self
                .http
                .get(url)
                .header("Accept", "application/json")
                .header("X-XSRF-TOKEN", &self.token)
                .header("X-Requested-With", "XMLHttpRequest")
                .send();
            match resp {
                Ok(r) if r.status().is_success() => return Ok(r.json()?),
                Ok(r) if attempt < 5 && (r.status().is_server_error() || r.status() == 429) => {
                    thread::sleep(Duration::from_secs(2 * attempt));
                }
                Ok(r) => return Err(format!("HTTP {} for {}", r.status(), url).into()),
                Err(e) if attempt < 5 => {
                    eprintln!("  retry {} after error: {}", attempt, e);
                    thread::sleep(Duration::from_secs(2 * attempt));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// One page of the "Produktsuche" with an empty filter.
    /// Returns the raw Spring `Page` object (`content`, `last`, `totalElements`, ...).
    pub fn search_page(&self, page: u32, size: u32) -> Result<Value, Error> {
        let url = format!("{}/products/advance-search?page={}&size={}", API, page, size);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let resp = self
                .http
                .post(&url)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .header("X-XSRF-TOKEN", &self.token)
                .header("X-Requested-With", "XMLHttpRequest")
                .body("{}")
                .send();
            match resp {
                Ok(r) if r.status().is_success() => return Ok(r.json()?),
                Ok(r) if attempt < 5 => {
                    eprintln!("  page {}: HTTP {}, retry {}", page, r.status(), attempt);
                    thread::sleep(Duration::from_secs(3 * attempt));
                }
                Ok(r) => return Err(format!("HTTP {} for page {}", r.status(), page).into()),
                Err(e) if attempt < 5 => {
                    eprintln!("  page {}: {}, retry {}", page, e, attempt);
                    thread::sleep(Duration::from_secs(3 * attempt));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Full product record as shown on `/rpc/ui/products/{cpId}`.
    pub fn product_detail(&self, cp_id: &str) -> Result<Value, Error> {
        self.get_json(&format!("{}/products/product/{}", API, cp_id))
    }
}

/// Iterate over all search pages, calling `on_page` with each `content` array.
/// Stops when the server reports `last == true`.
pub fn download_all_products<F>(client: &RpcClient, page_size: u32, mut on_page: F) -> Result<u64, Error>
where
    F: FnMut(&[Value]) -> Result<(), Error>,
{
    let mut page = 0u32;
    let mut seen = 0u64;
    loop {
        let body = client.search_page(page, page_size)?;
        let content = body
            .get("content")
            .and_then(Value::as_array)
            .ok_or("search response missing 'content'")?;
        let total = body.get("totalElements").and_then(Value::as_u64).unwrap_or(0);
        let total_pages = body.get("totalPages").and_then(Value::as_u64).unwrap_or(0);
        seen += content.len() as u64;
        eprintln!(
            "[list] page {}/{}: {} items ({}/{})",
            page + 1,
            total_pages,
            content.len(),
            seen,
            total
        );
        on_page(content)?;
        let last = body.get("last").and_then(Value::as_bool).unwrap_or(true);
        if last || content.is_empty() {
            break;
        }
        page += 1;
    }
    Ok(seen)
}
