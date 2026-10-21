//! Device authorization. Secrets stay in Rust memory for this app session;
//! neither device codes nor bearer/refresh tokens cross IPC or go to disk.
use crate::error::{AppError, AppResult, ErrorCode};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const AUTH: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";

#[derive(Default)]
pub struct AuthState {
    inner: Mutex<Inner>,
}
#[derive(Default)]
struct Inner {
    pending: HashMap<uuid::Uuid, Pending>,
    accounts: HashMap<String, Session>,
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

pub fn configured_client_id() -> Option<String> {
    std::env::var("NEBULA_MICROSOFT_CLIENT_ID")
        .ok()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
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
    pub async fn begin(&self, client_id: Option<String>) -> AppResult<DeviceLogin> {
        let client_id = client_id
            .filter(|id| !id.trim().is_empty())
            .or_else(configured_client_id)
            .ok_or_else(|| AppError::new(ErrorCode::InvalidInput).with("field", "client_id"))?;
        if uuid::Uuid::parse_str(&client_id).is_err() {
            return Err(AppError::new(ErrorCode::InvalidInput));
        }
        let response = crate::game::http_client()
            .post(format!("{AUTH}/devicecode"))
            .form(&[
                ("client_id", client_id.as_str()),
                ("scope", "XboxLive.signin"),
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
        let access = field(&value, "access_token", "microsoft_token")?;
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
        let account = Account {
            id: field(&profile, "id", "minecraft_java_profile")?.into(),
            name: field(&profile, "name", "minecraft_java_profile")?.into(),
        };
        inner.accounts.insert(
            account.id.clone(),
            Session {
                account: account.clone(),
                _token: token.into(),
                expires: Instant::now()
                    + Duration::from_secs(
                        minecraft["expires_in"].as_u64().unwrap_or(3600).min(86400),
                    ),
            },
        );
        Ok(LoginResult::Complete { account })
    }

    pub async fn accounts(&self) -> Vec<Account> {
        let mut inner = self.inner.lock().await;
        inner.accounts.retain(|_, s| s.expires > Instant::now());
        let mut accounts: Vec<_> = inner.accounts.values().map(|s| s.account.clone()).collect();
        accounts.sort_by(|a, b| a.name.cmp(&b.name));
        accounts
    }
    pub async fn logout(&self, id: String) {
        self.inner.lock().await.accounts.remove(&id);
    }
    pub async fn cancel(&self, id: uuid::Uuid) {
        self.inner.lock().await.pending.remove(&id);
    }
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
