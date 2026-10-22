//! Device authorization. Tokens never cross IPC. Minecraft access tokens stay
//! in Rust memory; only the Microsoft refresh token is remembered on disk
//! (see vault.rs) so a restart does not ask for the device code again.
use crate::error::{AppError, AppResult, ErrorCode};
use serde::Serialize;
use super::vault::{self, Remembered};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const AUTH: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";

/// Cheap to clone: clones share the same sessions (the UI and the MCP tools).
#[derive(Default, Clone)]
pub struct AuthState {
    inner: std::sync::Arc<Mutex<Inner>>,
}
#[derive(Default)]
struct Inner {
    pending: HashMap<uuid::Uuid, Pending>,
    accounts: HashMap<String, Session>,
    remembered: Option<BTreeMap<String, Remembered>>,
}
impl Inner {
    fn remembered(&mut self) -> &mut BTreeMap<String, Remembered> {
        self.remembered
            .get_or_insert_with(|| if cfg!(test) { BTreeMap::new() } else { vault::load() })
    }
    fn persist(&mut self) {
        let accounts = self.remembered().clone();
        if !cfg!(test) {
            if let Err(error) = vault::save(&accounts) {
                tracing::warn!("could not remember the Minecraft sign-in: {error:?}");
            }
        }
    }
}
struct Pending {
    client_id: String,
    device_code: String,
    expires: Instant,
    next_poll: Instant,
    interval: u64,
}
struct Session {
    account: Account,
    _token: String,
    expires: Instant,
}

#[derive(Clone, Serialize)]
pub struct Account {
    pub id: String,
    pub name: String,
}
/// The profile's active skin. The texture URL is public (textures.minecraft.net).
#[derive(Serialize)]
pub struct Skin {
    pub url: Option<String>,
    /// `classic` (4px arms) or `slim` (3px arms).
    pub variant: String,
    /// Capes the account owns; at most one is active.
    pub capes: Vec<Cape>,
}
#[derive(Serialize)]
pub struct Cape {
    pub id: String,
    pub alias: String,
    pub url: String,
    pub active: bool,
}
#[derive(Serialize)]
pub struct DeviceLogin {
    pub id: uuid::Uuid,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LoginResult {
    Pending { interval: u64 },
    Complete { account: Account },
}

/// Nebula Host's own Entra registration (public client, personal accounts,
/// device code). It is an identifier, not a secret, and the only one Nebula
/// signs in with: it cannot be replaced from the app or the environment.
/// Minecraft services accept it because Mojang approved it.
const NEBULA_CLIENT_ID: &str = "185747c5-d848-40be-8a49-c92efdc1ae88";

pub fn client_id() -> &'static str {
    NEBULA_CLIENT_ID
}
fn rejected(stage: &str) -> AppError {
    // Never include upstream response bodies: they can contain credentials.
    AppError::new(ErrorCode::UpstreamRejected).with("stage", stage)
}
fn field<'a>(value: &'a Value, key: &str, stage: &str) -> AppResult<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rejected(stage))
}

impl AuthState {
    pub async fn credentials(&self, id: &str) -> AppResult<super::client::Credentials> {
        let mut inner = self.inner.lock().await;
        let fresh = inner
            .accounts
            .get(id)
            .is_some_and(|session| session.expires > Instant::now() + Duration::from_secs(300));
        if !fresh {
            if let Some(saved) = inner.remembered().get(id).cloned() {
                match refresh(&saved.refresh_token).await {
                    Ok((session, next)) if session.account.id == id => {
                        let name = session.account.name.clone();
                        inner.accounts.insert(id.to_string(), session);
                        inner
                            .remembered()
                            .insert(id.to_string(), Remembered { name, refresh_token: next });
                        inner.persist();
                    }
                    Ok(_) | Err(_) => {
                        // Revoked, expired after long disuse, or another profile:
                        // forget it so the UI asks for a new sign-in.
                        inner.remembered().remove(id);
                        inner.persist();
                    }
                }
            }
        }
        let session = inner
            .accounts
            .get(id)
            .filter(|session| session.expires > Instant::now() + Duration::from_secs(300))
            .ok_or_else(|| {
                AppError::new(ErrorCode::UpstreamRejected)
                    .detail("Sign in to Minecraft again before playing.")
            })?;
        Ok(super::client::Credentials {
            name: session.account.name.clone(),
            uuid: session.account.id.clone(),
            token: session._token.clone(),
            demo: false,
            expires: Some(session.expires),
        })
    }
    pub async fn begin(&self) -> AppResult<DeviceLogin> {
        let client_id = client_id().to_string();
        let response = crate::game::http_client()
            .post(format!("{AUTH}/devicecode"))
            .form(&[
                ("client_id", client_id.as_str()),
                ("scope", "XboxLive.signin offline_access"),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(rejected("microsoft_device_code"));
        }
        let value: Value = response.json().await?;
        let expires_in = value["expires_in"].as_u64().unwrap_or(900).min(900);
        let interval = value["interval"].as_u64().unwrap_or(5).clamp(5, 60);
        let id = uuid::Uuid::new_v4();
        let verification_uri =
            field(&value, "verification_uri", "microsoft_device_code")?.to_string();
        let url =
            reqwest::Url::parse(&verification_uri).map_err(|_| rejected("verification_uri"))?;
        if url.scheme() != "https"
            || !matches!(
                url.host_str(),
                Some("microsoft.com" | "www.microsoft.com" | "login.microsoftonline.com")
            )
        {
            return Err(rejected("verification_uri"));
        }
        let login = DeviceLogin {
            id,
            user_code: field(&value, "user_code", "microsoft_device_code")?.into(),
            verification_uri,
            expires_in,
            interval,
        };
        let mut inner = self.inner.lock().await;
        inner.pending.retain(|_, p| p.expires > Instant::now());
        // One interactive flow at a time; beginning another cancels the old one.
        inner.pending.clear();
        inner.pending.insert(
            id,
            Pending {
                client_id,
                device_code: field(&value, "device_code", "microsoft_device_code")?.into(),
                expires: Instant::now() + Duration::from_secs(expires_in),
                next_poll: Instant::now() + Duration::from_secs(interval),
                interval,
            },
        );
        Ok(login)
    }

    pub async fn poll(&self, id: uuid::Uuid) -> AppResult<LoginResult> {
        // Serialize polling, cancellation, and sign-out to prevent a late login
        // response from restoring an account after it has been removed.
        let mut inner = self.inner.lock().await;
        let pending = inner
            .pending
            .get_mut(&id)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound))?;
        if pending.expires <= Instant::now() {
            inner.pending.remove(&id);
            return Err(AppError::new(ErrorCode::Cancelled));
        }
        if pending.next_poll > Instant::now() {
            return Ok(LoginResult::Pending {
                interval: pending.interval,
            });
        }
        pending.next_poll = Instant::now() + Duration::from_secs(pending.interval);
        let response = crate::game::http_client()
            .post(format!("{AUTH}/token"))
            .form(&[
                ("client_id", pending.client_id.as_str()),
                ("device_code", pending.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?;
        let success = response.status().is_success();
        let value: Value = response.json().await?;
        if !success {
            match value["error"].as_str() {
                Some("authorization_pending") => {
                    return Ok(LoginResult::Pending {
                        interval: pending.interval,
                    })
                }
                Some("slow_down") => {
                    pending.interval += 5;
                    pending.next_poll = Instant::now() + Duration::from_secs(pending.interval);
                    return Ok(LoginResult::Pending {
                        interval: pending.interval,
                    });
                }
                _ => {
                    inner.pending.remove(&id);
                    return Err(rejected("microsoft_authorization"));
                }
            }
        }
        inner.pending.remove(&id);
        let session = minecraft_session(field(&value, "access_token", "microsoft_token")?).await?;
        let account = session.account.clone();
        if let Some(refresh_token) = value["refresh_token"].as_str().filter(|t| !t.is_empty()) {
            inner.remembered().insert(
                account.id.clone(),
                Remembered { name: account.name.clone(), refresh_token: refresh_token.into() },
            );
            inner.persist();
        }
        inner.accounts.insert(account.id.clone(), session);
        Ok(LoginResult::Complete { account })
    }

    pub async fn accounts(&self) -> Vec<Account> {
        let mut inner = self.inner.lock().await;
        inner.accounts.retain(|_, s| s.expires > Instant::now());
        let mut accounts: Vec<_> = inner.accounts.values().map(|s| s.account.clone()).collect();
        // Remembered sign-ins are listed too; their token is refreshed on play.
        let live: HashSet<String> = accounts.iter().map(|a| a.id.clone()).collect();
        let saved: Vec<Account> = inner
            .remembered()
            .iter()
            .filter(|(id, _)| !live.contains(*id))
            .map(|(id, r)| Account { id: id.clone(), name: r.name.clone() })
            .collect();
        accounts.extend(saved);
        accounts.sort_by(|a, b| a.name.cmp(&b.name));
        accounts
    }
    pub async fn skin(&self, id: &str) -> AppResult<Skin> {
        let token = self.credentials(id).await?.token;
        let response = crate::game::http_client()
            .get("https://api.minecraftservices.com/minecraft/profile")
            .bearer_auth(token)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(rejected("minecraft_java_profile"));
        }
        Ok(active_skin(&response.json().await?))
    }

    /// Upload a 64x64 (or legacy 64x32) PNG as the profile's skin.
    pub async fn set_skin(&self, id: &str, png: Vec<u8>, slim: bool) -> AppResult<Skin> {
        let size_ok = png.len() <= 24 * 1024
            && png.starts_with(b"\x89PNG\r\n\x1a\n")
            && png.len() > 24
            && u32::from_be_bytes([png[16], png[17], png[18], png[19]]) == 64
            && matches!(u32::from_be_bytes([png[20], png[21], png[22], png[23]]), 32 | 64);
        if !size_ok {
            return Err(AppError::new(ErrorCode::InvalidInput).with("field", "skin"));
        }
        let token = self.credentials(id).await?.token;
        let form = reqwest::multipart::Form::new()
            .text("variant", if slim { "slim" } else { "classic" })
            .part(
                "file",
                reqwest::multipart::Part::bytes(png)
                    .file_name("skin.png")
                    .mime_str("image/png")?,
            );
        let response = crate::game::http_client()
            .post("https://api.minecraftservices.com/minecraft/profile/skins")
            .bearer_auth(token)
            .multipart(form)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(rejected("minecraft_skin_upload"));
        }
        Ok(active_skin(&response.json().await?))
    }

    /// Show one of the account's capes, or none.
    pub async fn set_cape(&self, id: &str, cape: Option<String>) -> AppResult<Skin> {
        let token = self.credentials(id).await?.token;
        let client = crate::game::http_client();
        let url = "https://api.minecraftservices.com/minecraft/profile/capes/active";
        let request = match cape {
            Some(cape_id) => client.put(url).json(&json!({ "capeId": cape_id })),
            None => client.delete(url),
        };
        let response = request.bearer_auth(token).send().await?;
        let status = response.status();
        if !status.is_success() {
            // Rate limiting is the usual cause; say so instead of a bare error.
            let reason = if status.as_u16() == 429 {
                "Minecraft is limiting cape changes. Wait a minute and try again."
            } else {
                "Minecraft did not accept this cape."
            };
            return Err(rejected("minecraft_cape").with("status", &status.as_u16().to_string()).detail(reason));
        }
        Ok(active_skin(&response.json().await?))
    }

    pub async fn logout(&self, id: String) {
        let mut inner = self.inner.lock().await;
        inner.accounts.remove(&id);
        if inner.remembered().remove(&id).is_some() {
            inner.persist();
        }
    }
    pub async fn cancel(&self, id: uuid::Uuid) {
        self.inner.lock().await.pending.remove(&id);
    }
}

fn active_skin(profile: &Value) -> Skin {
    let skin = profile["skins"]
        .as_array()
        .and_then(|skins| skins.iter().find(|s| s["state"] == "ACTIVE"));
    Skin {
        url: skin
            .and_then(|s| s["url"].as_str())
            .map(|u| u.replacen("http://", "https://", 1)),
        variant: skin
            .and_then(|s| s["variant"].as_str())
            .unwrap_or("CLASSIC")
            .to_lowercase(),
        capes: profile["capes"]
            .as_array()
            .map(|capes| {
                capes
                    .iter()
                    .filter_map(|c| {
                        Some(Cape {
                            id: c["id"].as_str()?.into(),
                            alias: c["alias"].as_str().unwrap_or("Cape").into(),
                            url: c["url"].as_str()?.replacen("http://", "https://", 1),
                            active: c["state"] == "ACTIVE",
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// A new Minecraft session from a remembered refresh token. Microsoft rotates
/// refresh tokens, so the next one comes back to be stored in its place.
async fn refresh(refresh_token: &str) -> AppResult<(Session, String)> {
    let response = crate::game::http_client()
        .post(format!("{AUTH}/token"))
        .form(&[
            ("client_id", client_id()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(rejected("microsoft_refresh"));
    }
    let value: Value = response.json().await?;
    let session = minecraft_session(field(&value, "access_token", "microsoft_refresh")?).await?;
    let next = value["refresh_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .unwrap_or(refresh_token);
    Ok((session, next.to_string()))
}

/// Microsoft access token -> Xbox Live -> XSTS -> Minecraft, verified by a
/// Java profile lookup.
async fn minecraft_session(access: &str) -> AppResult<Session> {
    let client = crate::game::http_client();
    let xbox = post(&client, "https://user.auth.xboxlive.com/user/authenticate", json!({
        "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={access}") },
        "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT"
    }), "xbox_live").await?;
    let xsts = post(&client, "https://xsts.auth.xboxlive.com/xsts/authorize", json!({
        "Properties": { "SandboxId": "RETAIL", "UserTokens": [field(&xbox, "Token", "xbox_live")?] },
        "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT"
    }), "xsts").await?;
    let uhs = xsts["DisplayClaims"]["xui"][0]["uhs"]
        .as_str()
        .ok_or_else(|| rejected("xsts"))?;
    let minecraft = post(
        &client,
        "https://api.minecraftservices.com/authentication/login_with_xbox",
        json!({
            "identityToken": format!("XBL3.0 x={uhs};{}", field(&xsts, "Token", "xsts")?)
        }),
        "minecraft_login",
    )
    .await?;
    let token = field(&minecraft, "access_token", "minecraft_login")?;
    // A successful Java profile lookup verifies access to a real profile.
    let response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(token)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(rejected("minecraft_java_profile"));
    }
    let profile: Value = response.json().await?;
    Ok(Session {
        account: Account {
            id: field(&profile, "id", "minecraft_java_profile")?.into(),
            name: field(&profile, "name", "minecraft_java_profile")?.into(),
        },
        _token: token.into(),
        expires: Instant::now()
            + Duration::from_secs(minecraft["expires_in"].as_u64().unwrap_or(3600).min(86400)),
    })
}

async fn post(client: &reqwest::Client, url: &str, body: Value, stage: &str) -> AppResult<Value> {
    let response = client.post(url).json(&body).send().await?;
    if !response.status().is_success() {
        return Err(rejected(stage));
    }
    Ok(response.json().await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_poll_obeys_server_interval_and_cancel_discards_code() {
        let state = AuthState::default();
        let id = uuid::Uuid::new_v4();
        state.inner.lock().await.pending.insert(
            id,
            Pending {
                client_id: "client".into(),
                device_code: "secret-device-code".into(),
                expires: Instant::now() + Duration::from_secs(60),
                next_poll: Instant::now() + Duration::from_secs(30),
                interval: 30,
            },
        );
        assert!(matches!(
            state.poll(id).await.unwrap(),
            LoginResult::Pending { interval: 30 }
        ));
        state.cancel(id).await;
        assert!(state.poll(id).await.is_err());
    }

    #[tokio::test]
    async fn expired_flow_is_removed_without_network_access() {
        let state = AuthState::default();
        let id = uuid::Uuid::new_v4();
        state.inner.lock().await.pending.insert(
            id,
            Pending {
                client_id: "client".into(),
                device_code: "secret".into(),
                expires: Instant::now(),
                next_poll: Instant::now(),
                interval: 5,
            },
        );
        assert!(state.poll(id).await.is_err());
        assert!(state.inner.lock().await.pending.is_empty());
    }

    #[tokio::test]
    async fn account_listing_drops_expired_sessions_and_never_serializes_tokens() {
        let state = AuthState::default();
        let account = Account {
            id: "uuid".into(),
            name: "Player".into(),
        };
        state.inner.lock().await.accounts.insert(
            account.id.clone(),
            Session {
                account: account.clone(),
                _token: "private-token".into(),
                expires: Instant::now() + Duration::from_secs(30),
            },
        );
        let listed = serde_json::to_string(&state.accounts().await).unwrap();
        assert!(listed.contains("Player"));
        assert!(!listed.contains("private-token"));
        state.logout(account.id.clone()).await;
        assert!(state.accounts().await.is_empty());
        state.inner.lock().await.accounts.insert(
            account.id.clone(),
            Session {
                account,
                _token: "expired".into(),
                expires: Instant::now(),
            },
        );
        assert!(state.accounts().await.is_empty());
    }
}
