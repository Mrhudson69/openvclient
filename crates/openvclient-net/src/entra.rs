//! Microsoft Entra ID (Azure AD) access tokens, obtained with the standard
//! OAuth flows and shared by the Azure connectors (SQL, Storage). Tokens are
//! cached until shortly before they expire.

use std::time::{Duration, Instant};

use openvclient_core::{Error, Notifier, Result};
use serde_json::Value as J;

/// What the token is for: an OAuth v2 scope and the v1 resource used by
/// managed identity and the Azure CLI.
#[derive(Clone, Copy, Debug)]
pub struct Audience {
    pub scope: &'static str,
    pub resource: &'static str,
}

pub const AZURE_SQL: Audience = Audience { scope: "https://database.windows.net/.default", resource: "https://database.windows.net/" };
pub const AZURE_STORAGE: Audience = Audience { scope: "https://storage.azure.com/.default", resource: "https://storage.azure.com/" };
/// Microsoft's public client id used by Azure tooling for user sign-in.
pub const DEFAULT_PUBLIC_CLIENT: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46";

#[derive(Clone)]
pub enum Flow {
    Password {
        tenant: String,
        client_id: String,
        user: String,
        password: String,
    },
    ServicePrincipal {
        tenant: String,
        client_id: String,
        secret: String,
    },
    ManagedIdentity {
        client_id: Option<String>,
    },
    DeviceCode {
        tenant: String,
        client_id: String,
    },
    /// Environment service principal, then managed identity, then Azure CLI.
    Default,
}

#[derive(Default)]
pub struct Cache {
    token: Option<(String, Instant)>,
}

impl Cache {
    pub fn get(&self) -> Option<String> {
        self.token.as_ref().filter(|(_, exp)| Instant::now() + Duration::from_secs(120) < *exp).map(|(t, _)| t.clone())
    }
    pub fn put(&mut self, token: String, expires_in: u64) {
        self.token = Some((token, Instant::now() + Duration::from_secs(expires_in.max(60))));
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap_or_default()
}

fn authority(tenant: &str) -> String {
    format!("https://login.microsoftonline.com/{}/oauth2/v2.0", if tenant.trim().is_empty() { "organizations" } else { tenant.trim() })
}

fn token_from(v: &J) -> Result<(String, u64)> {
    if let Some(t) = v.get("access_token").and_then(|t| t.as_str()) {
        let exp = v.get("expires_in").and_then(|e| e.as_u64().or_else(|| e.as_str().and_then(|s| s.parse().ok()))).unwrap_or(3600);
        return Ok((t.to_string(), exp));
    }
    let code = v.get("error").and_then(|e| e.as_str()).unwrap_or("error");
    let desc = v.get("error_description").and_then(|e| e.as_str()).unwrap_or("").lines().next().unwrap_or("");
    Err(Error::auth(format!("Microsoft Entra ID sign-in failed: {code}")).with_detail(desc.to_string()))
}

async fn post(url: &str, form: &[(&str, &str)]) -> Result<J> {
    let r = client().post(url).form(form).send().await.map_err(|e| Error::connection(format!("Could not reach Microsoft Entra ID: {e}")))?;
    r.json().await.map_err(|e| Error::other(format!("Unexpected token response: {e}")))
}

/// Get an access token, prompting through `notifier` for device-code sign-in.
pub async fn acquire(flow: &Flow, aud: Audience, notifier: Option<&Notifier>) -> Result<(String, u64)> {
    #[allow(non_snake_case)]
    let (SCOPE, RESOURCE) = (aud.scope, aud.resource);
    match flow {
        Flow::Password { tenant, client_id, user, password } => {
            let v = post(&format!("{}/token", authority(tenant)), &[("grant_type", "password"), ("client_id", client_id), ("username", user), ("password", password), ("scope", SCOPE)]).await?;
            token_from(&v)
        }
        Flow::ServicePrincipal { tenant, client_id, secret } => {
            if tenant.trim().is_empty() {
                return Err(Error::invalid("A tenant ID is required for service principal sign-in"));
            }
            let v = post(&format!("{}/token", authority(tenant)), &[("grant_type", "client_credentials"), ("client_id", client_id), ("client_secret", secret), ("scope", SCOPE)]).await?;
            token_from(&v)
        }
        Flow::ManagedIdentity { client_id } => managed_identity(client_id.as_deref(), RESOURCE).await,
        Flow::DeviceCode { tenant, client_id } => {
            let start = post(&format!("{}/devicecode", authority(tenant)), &[("client_id", client_id), ("scope", SCOPE)]).await?;
            let device_code = start.get("device_code").and_then(|d| d.as_str()).ok_or_else(|| token_from(&start).err().unwrap_or_else(|| Error::other("No device code returned")))?.to_string();
            let message = start.get("message").and_then(|m| m.as_str()).unwrap_or("Open https://microsoft.com/devicelogin and enter the code shown.").to_string();
            if let Some(n) = notifier {
                (n.0)(message);
            }
            let mut interval = start.get("interval").and_then(|i| i.as_u64()).unwrap_or(5);
            let deadline = Instant::now() + Duration::from_secs(start.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(900));
            while Instant::now() < deadline {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                let v =
                    post(&format!("{}/token", authority(tenant)), &[("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("client_id", client_id), ("device_code", &device_code)]).await?;
                match v.get("error").and_then(|e| e.as_str()) {
                    Some("authorization_pending") => continue,
                    Some("slow_down") => interval += 5,
                    _ => return token_from(&v),
                }
            }
            Err(Error::auth("Device sign-in timed out"))
        }
        Flow::Default => {
            if let (Ok(t), Ok(c), Ok(s)) = (std::env::var("AZURE_TENANT_ID"), std::env::var("AZURE_CLIENT_ID"), std::env::var("AZURE_CLIENT_SECRET")) {
                return Box::pin(acquire(&Flow::ServicePrincipal { tenant: t, client_id: c, secret: s }, aud, notifier)).await;
            }
            if let Ok(r) = managed_identity(std::env::var("AZURE_CLIENT_ID").ok().as_deref(), RESOURCE).await {
                return Ok(r);
            }
            azure_cli(RESOURCE).await
        }
    }
}

async fn managed_identity(client_id: Option<&str>, resource: &str) -> Result<(String, u64)> {
    #[allow(non_snake_case)]
    let RESOURCE = resource;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap_or_default();
    // App Service / Functions / Container Apps expose IDENTITY_ENDPOINT; VMs use IMDS.
    let req = match (std::env::var("IDENTITY_ENDPOINT"), std::env::var("IDENTITY_HEADER")) {
        (Ok(ep), Ok(h)) => {
            let mut r = http.get(ep).header("X-IDENTITY-HEADER", h).query(&[("api-version", "2019-08-01"), ("resource", RESOURCE)]);
            if let Some(c) = client_id {
                r = r.query(&[("client_id", c)]);
            }
            r
        }
        _ => {
            let mut r = http.get("http://169.254.169.254/metadata/identity/oauth2/token").header("Metadata", "true").query(&[("api-version", "2018-02-01"), ("resource", RESOURCE)]);
            if let Some(c) = client_id {
                r = r.query(&[("client_id", c)]);
            }
            r
        }
    };
    let v: J = req.send().await.map_err(|_| Error::connection("No managed identity endpoint here (this only works on Azure hosts)"))?.json().await.map_err(|e| Error::other(e.to_string()))?;
    token_from(&v)
}

async fn azure_cli(resource: &str) -> Result<(String, u64)> {
    #[allow(non_snake_case)]
    let RESOURCE = resource;
    let out = tokio::process::Command::new("az")
        .args(["account", "get-access-token", "--resource", RESOURCE, "--output", "json"])
        .output()
        .await
        .map_err(|_| Error::auth("No Azure credentials found: set AZURE_TENANT_ID/AZURE_CLIENT_ID/AZURE_CLIENT_SECRET, run on an Azure host, or sign in with `az login`"))?;
    if !out.status.success() {
        return Err(Error::auth("Azure CLI is not signed in (run `az login`)").with_detail(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    let v: J = serde_json::from_slice(&out.stdout).map_err(|e| Error::other(e.to_string()))?;
    let token = v.get("accessToken").and_then(|t| t.as_str()).ok_or_else(|| Error::auth("Azure CLI returned no token"))?.to_string();
    Ok((token, 3000))
}

impl std::fmt::Debug for Flow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Flow::Password { .. } => "Password",
            Flow::ServicePrincipal { .. } => "ServicePrincipal",
            Flow::ManagedIdentity { .. } => "ManagedIdentity",
            Flow::DeviceCode { .. } => "DeviceCode",
            Flow::Default => "Default",
        })
    }
}
