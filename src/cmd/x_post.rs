use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, Mac};
use rand::Rng;
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

const MEDIA_UPLOAD_URL: &str = "https://upload.twitter.com/1.1/media/upload.json";
const TWEET_URL: &str = "https://api.twitter.com/2/tweets";

struct XCredentials {
    consumer_key: String,
    consumer_secret: String,
    access_token: String,
    access_token_secret: String,
}

fn load_credentials() -> Result<XCredentials, Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok(); // Load .env if present, ignore if not

    let consumer_key = std::env::var("consumer_key").map_err(|_| {
        "Missing consumer_key in .env"
    })?;
    let consumer_secret = std::env::var("secret_key").map_err(|_| {
        "Missing secret_key in .env"
    })?;

    let access_token = std::env::var("access_token").map_err(|_| {
        "Missing access_token in .env. Generate one in the X Developer Console \
         under \"Keys and Tokens\" > \"Access Token and Secret\", then add:\n  \
         access_token=...\n  access_token_secret=..."
    })?;
    let access_token_secret = std::env::var("access_token_secret").map_err(|_| {
        "Missing access_token_secret in .env. Generate one in the X Developer Console \
         under \"Keys and Tokens\" > \"Access Token and Secret\", then add:\n  \
         access_token=...\n  access_token_secret=..."
    })?;

    Ok(XCredentials {
        consumer_key,
        consumer_secret,
        access_token,
        access_token_secret,
    })
}

/// Percent-encode a string per RFC 5849.
fn percent_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

/// Generate a random nonce.
fn generate_nonce() -> String {
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| {
            let idx = rng.gen_range(0..36);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect()
}

/// Build the OAuth 1.0a Authorization header.
fn build_oauth_header(
    creds: &XCredentials,
    method: &str,
    url: &str,
    extra_params: &BTreeMap<String, String>,
) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let nonce = generate_nonce();

    let mut params = BTreeMap::new();
    params.insert("oauth_consumer_key".to_string(), creds.consumer_key.clone());
    params.insert("oauth_nonce".to_string(), nonce.clone());
    params.insert(
        "oauth_signature_method".to_string(),
        "HMAC-SHA1".to_string(),
    );
    params.insert("oauth_timestamp".to_string(), timestamp.clone());
    params.insert("oauth_token".to_string(), creds.access_token.clone());
    params.insert("oauth_version".to_string(), "1.0".to_string());

    // Merge extra params for signature base
    for (k, v) in extra_params {
        params.insert(k.clone(), v.clone());
    }

    // Build parameter string
    let param_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // Build signature base string
    let base = format!(
        "{}&{}&{}",
        method.to_uppercase(),
        percent_encode(url),
        percent_encode(&param_string)
    );

    // Build signing key
    let signing_key = format!(
        "{}&{}",
        percent_encode(&creds.consumer_secret),
        percent_encode(&creds.access_token_secret)
    );

    // HMAC-SHA1 signature
    let mut mac = HmacSha1::new_from_slice(signing_key.as_bytes()).unwrap();
    mac.update(base.as_bytes());
    let signature = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    // Build header
    format!(
        "OAuth oauth_consumer_key=\"{}\", \
         oauth_nonce=\"{}\", \
         oauth_signature=\"{}\", \
         oauth_signature_method=\"HMAC-SHA1\", \
         oauth_timestamp=\"{}\", \
         oauth_token=\"{}\", \
         oauth_version=\"1.0\"",
        percent_encode(&creds.consumer_key),
        percent_encode(&nonce),
        percent_encode(&signature),
        percent_encode(&timestamp),
        percent_encode(&creds.access_token),
    )
}

/// Upload an image to X and return the media_id string.
async fn upload_media(
    creds: &XCredentials,
    image_path: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let image_bytes = std::fs::read(image_path)?;
    let media_data = base64::engine::general_purpose::STANDARD.encode(&image_bytes);

    let mut params = BTreeMap::new();
    params.insert("media_data".to_string(), media_data.clone());

    let auth_header = build_oauth_header(creds, "POST", MEDIA_UPLOAD_URL, &params);

    let client = reqwest::Client::new();
    let resp = client
        .post(MEDIA_UPLOAD_URL)
        .header("Authorization", &auth_header)
        .form(&[("media_data", &media_data)])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Media upload failed ({}): {}", status, body).into());
    }

    let json: serde_json::Value = resp.json().await?;
    let media_id = json["media_id_string"]
        .as_str()
        .ok_or("No media_id_string in upload response")?
        .to_string();

    Ok(media_id)
}

/// Create a tweet with an attached media image. Returns the tweet URL.
async fn create_tweet(
    creds: &XCredentials,
    text: &str,
    media_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // For v2 tweets, OAuth params don't include the JSON body in signature
    let auth_header = build_oauth_header(creds, "POST", TWEET_URL, &BTreeMap::new());

    let body = serde_json::json!({
        "text": text,
        "media": {
            "media_ids": [media_id]
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(TWEET_URL)
        .header("Authorization", &auth_header)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Tweet creation failed ({}): {}", status, body).into());
    }

    let json: serde_json::Value = resp.json().await?;
    let tweet_id = json["data"]["id"]
        .as_str()
        .unwrap_or("unknown");

    Ok(format!("https://x.com/i/status/{}", tweet_id))
}

/// Generate and post a graphic to X. Returns the tweet URL.
pub async fn post_graphic(
    image_path: &Path,
    tweet_text: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let creds = load_credentials()?;
    println!("  Uploading image to X...");
    let media_id = upload_media(&creds, image_path).await?;
    println!("  Creating tweet...");
    let tweet_url = create_tweet(&creds, tweet_text, &media_id).await?;
    Ok(tweet_url)
}
