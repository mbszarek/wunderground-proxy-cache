#![deny(warnings)]
#![warn(rust_2018_idioms)]

use std::{
    sync::Arc,
    time::Instant,
};

use axum::{extract::State, routing::get, Json, Router};
use constants::{API_KEY, CACHE_DURATION_SECS, PWS_ID};

use reqwest::Client;
use serde_json::Value;
use tokio::sync::RwLock;
mod constants;

#[derive(Debug, Clone)]
struct AppConfig {
    cache_duration_secs: u64,
    pws_id: String,
    api_key: String,
}

#[derive(Debug, Clone)]
struct CachedEntry {
    value: Value,
    fetched_at: Instant,
}

#[derive(Debug, Clone)]
struct AppState {
    config: AppConfig,
    client: Client,
    cached_entry: Arc<RwLock<Option<CachedEntry>>>,
}

fn load_config() -> AppConfig {
    let raw_cache_duration_secs =
        std::env::var(CACHE_DURATION_SECS).expect("CACHE_DURATION_SECS not defined");
    let cache_duration_secs: u64 = raw_cache_duration_secs
        .parse()
        .expect("CACHE_DURATION_SECS wrong value");

    let pws_id = std::env::var(PWS_ID).expect("PWS_ID not defined");
    let api_key = std::env::var(API_KEY).expect("API_KEY not defined");

    AppConfig {
        cache_duration_secs: cache_duration_secs,
        pws_id: pws_id,
        api_key: api_key,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    let config = load_config();

    let state = AppState {
        config: config,
        client: Client::new(),
        cached_entry: Arc::new(RwLock::new(None)),
    };

    let app = Router::new().route("/", get(root)).with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    axum::serve(listener, app).await?;

    Ok(())
}

async fn root(State(state): State<AppState>) -> Json<Value> {
    let cached_value = {
        let cached_entry = state.cached_entry.read().await;
        cached_entry
            .clone()
            .filter(|entry| entry.fetched_at.elapsed().as_secs() < state.config.cache_duration_secs)
    };

    match cached_value {
        None => {
            let json = fetch_json(&state).await.unwrap();
            let mut writeable_state = state.cached_entry.write().await;
            *writeable_state = Some(CachedEntry {
                value: json.clone(),
                fetched_at: Instant::now(),
            });
            Json(json)
        }
        Some(cached_value) => Json(cached_value.value),
    }
}

async fn fetch_json(state: &AppState) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let pws_id = state.config.pws_id.clone();
    let api_key = state.config.api_key.clone();

    let res = state.client.get(format!("https://api.weather.com/v2/pws/observations/current?stationId={pws_id}&format=json&units=e&apiKey={api_key}"))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip")
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/111.0.0.0 Safari/537.36")
        .send()
        .await?
        .json::<Value>()
        .await?;

    Ok(res)
}
