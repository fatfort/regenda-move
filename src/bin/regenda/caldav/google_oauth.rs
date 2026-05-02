use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const GOOGLE_DEVICE_CODE_URL: &str = "https://oauth2.googleapis.com/device/code";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
// Read+write scope. The CRUD branch needs to insert/patch/delete via the
// Calendar v3 API, which `calendar.events` (read+write on events of accessible
// calendars) authorises. Switching from `calendar.readonly` invalidates every
// previously stored refresh_token: Google won't broaden a refresh_token's
// scope, so the next get_access_token call will fail to refresh and the
// existing Ok(None) → OAuthScene path re-prompts the user to re-auth all
// Google sources. Mention this in the deploy commit message.
const GOOGLE_CALDAV_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events";

const TOKEN_DIR: &str = "/home/root/.config/reGenda";

/// Short timeouts so offline devices fall back to the cache quickly.
const OAUTH_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const OAUTH_TOTAL_TIMEOUT: Duration = Duration::from_secs(8);

fn oauth_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .connect_timeout(OAUTH_CONNECT_TIMEOUT)
        .timeout(OAUTH_TOTAL_TIMEOUT)
        .build()
        .expect("failed to build OAuth HTTP client")
}

/// Response from the device authorization request.
#[derive(Deserialize, Clone, Debug)]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Response from the token exchange.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

/// Stored token (persisted to disk).
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: String,
    pub client_id: String,
    pub client_secret: String,
}

/// Error response from token polling.
#[derive(Deserialize, Debug)]
struct TokenErrorResponse {
    error: String,
}

/// Initiate the device authorization flow.
/// Returns the device auth response containing the user_code and verification_url
/// that must be shown to the user.
pub fn start_device_auth(client_id: &str) -> Result<DeviceAuthResponse> {
    let client = oauth_client();

    let resp = client
        .post(GOOGLE_DEVICE_CODE_URL)
        .form(&[
            ("client_id", client_id),
            ("scope", GOOGLE_CALDAV_SCOPE),
        ])
        .send()
        .context("Failed to start device authorization")?;

    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        bail!("Device auth request failed: {}", body);
    }

    resp.json::<DeviceAuthResponse>()
        .context("Failed to parse device auth response")
}

/// Poll for the token after user has authorized.
/// Returns Ok(Some(token)) when authorized, Ok(None) when still pending,
/// or Err on fatal errors.
pub fn poll_for_token(
    client_id: &str,
    client_secret: &str,
    device_code: &str,
) -> Result<Option<TokenResponse>> {
    let client = oauth_client();

    let resp = match client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            // Treat transient network errors as "still pending" so WiFi blips
            // during device-auth polling don't abort the whole flow.
            log::warn!("Transient poll error, retrying: {e}");
            return Ok(None);
        }
    };

    let status = resp.status();
    let body = match resp.text() {
        Ok(b) => b,
        Err(e) => {
            log::warn!("Transient poll read error, retrying: {e}");
            return Ok(None);
        }
    };

    if status.is_success() {
        let token: TokenResponse =
            serde_json::from_str(&body).context("Failed to parse token response")?;
        return Ok(Some(token));
    }

    // Check if it's a pending/slow_down error (keep polling) or fatal
    if let Ok(error_resp) = serde_json::from_str::<TokenErrorResponse>(&body) {
        match error_resp.error.as_str() {
            "authorization_pending" | "slow_down" => return Ok(None),
            "expired_token" => bail!("Authorization expired. Please try again."),
            "access_denied" => bail!("Access denied by user."),
            other => bail!("Token error: {}", other),
        }
    }

    bail!("Unexpected token response ({}): {}", status, body);
}

/// Outcome of an attempted refresh. We need to distinguish "the server told
/// us our refresh_token is bad" (the user must re-authorize) from "the request
/// didn't reach the server at all / something blew up in transit" (transient,
/// the existing token is still presumably good — caller should fail this
/// fetch but NOT trigger an OAuth re-prompt).
pub enum RefreshOutcome {
    Token(String),
    /// Google returned `invalid_grant` (or another permanent rejection of the
    /// refresh_token). Re-auth is required.
    InvalidGrant,
    /// Transient: timeout, DNS failure, 5xx, malformed response, etc. The
    /// refresh_token is presumably still valid; retry next fetch cycle.
    Transient(anyhow::Error),
}

/// Refresh an expired access token using the refresh token.
pub fn refresh_access_token(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> RefreshOutcome {
    let client = oauth_client();

    let resp = match client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
    {
        Ok(r) => r,
        Err(e) => return RefreshOutcome::Transient(anyhow::Error::new(e)),
    };

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        // Inspect the OAuth error code per RFC 6749 §5.2. invalid_grant means
        // the refresh_token is no longer valid (revoked, expired, scope
        // narrowed) — the user must re-auth. Any other 4xx/5xx is treated as
        // transient (rate limits, server errors, scope-mismatch quirks).
        if body.contains("\"invalid_grant\"") || body.contains("'invalid_grant'") {
            return RefreshOutcome::InvalidGrant;
        }
        return RefreshOutcome::Transient(anyhow::anyhow!(
            "Token refresh returned {}: {}",
            status,
            body
        ));
    }

    match resp.json::<TokenResponse>() {
        Ok(token) => RefreshOutcome::Token(token.access_token),
        Err(e) => RefreshOutcome::Transient(anyhow::Error::new(e)),
    }
}

/// Get the token file path for a given server name.
fn token_path(server_name: &str) -> PathBuf {
    PathBuf::from(TOKEN_DIR).join(format!("token_{}.json", server_name))
}

/// Load a stored token from disk.
pub fn load_stored_token(server_name: &str) -> Option<StoredToken> {
    let path = token_path(server_name);
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Save a token to disk.
pub fn save_stored_token(server_name: &str, token: &StoredToken) -> Result<()> {
    let path = token_path(server_name);

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let data = serde_json::to_string_pretty(token)
        .context("Failed to serialize token")?;
    std::fs::write(&path, data)
        .with_context(|| format!("Failed to write token to {:?}", path))?;

    Ok(())
}

/// Get a valid access token for a Google source.
/// First tries to use a stored refresh token. If none exists, returns None
/// to indicate that device auth flow is needed.
pub fn get_access_token(
    server_name: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<Option<String>> {
    if let Some(stored) = load_stored_token(server_name) {
        // We intentionally do NOT delete the stored token on any failure — a
        // transient network blip must not force re-auth on every subsequent
        // run. Google invalidates the refresh_token server-side if it's truly
        // bad; we detect that via the explicit `invalid_grant` response code
        // and only then route to the OAuth re-prompt flow.
        match refresh_access_token(client_id, client_secret, &stored.refresh_token) {
            RefreshOutcome::Token(access_token) => {
                let updated = StoredToken {
                    access_token: access_token.clone(),
                    ..stored
                };
                save_stored_token(server_name, &updated).ok();
                Ok(Some(access_token))
            }
            RefreshOutcome::InvalidGrant => {
                log::warn!(
                    "refresh_token for {} rejected as invalid_grant — needs re-auth",
                    server_name
                );
                Ok(None)
            }
            RefreshOutcome::Transient(e) => {
                // Surface as a hard error on this fetch only — fetch_all will
                // log the source as failed and (with our cache fix) fall back
                // to last-known-good entries from the on-disk cache. The
                // refresh_token stays put and will be retried next cycle.
                log::warn!(
                    "transient refresh failure for {}: {:?}; will retry",
                    server_name,
                    e
                );
                Err(e.context(format!("transient refresh failure for {}", server_name)))
            }
        }
    } else {
        Ok(None)
    }
}
