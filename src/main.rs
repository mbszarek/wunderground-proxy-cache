#![deny(warnings)]
#![warn(rust_2018_idioms)]

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use constants::{API_KEY, CACHE_DURATION_SECS, CURRENT, FORECAST, PWS_ID, USER_AGENT};

use reqwest::Client;
use serde::Deserialize;
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
    cached_entries: Arc<RwLock<HashMap<String, CachedEntry>>>,
}

#[derive(Deserialize)]
struct ForecastQueryParams {
    geocode: String,
    language: String,
}

type Result<A> = std::result::Result<A, Box<dyn std::error::Error + Send + Sync>>;

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
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = load_config();

    let state = AppState {
        config: config,
        client: Client::new(),
        cached_entries: Arc::new(RwLock::new(HashMap::with_capacity(2))),
    };

    let app = Router::new()
        .route("/current", get(current))
        .route("/forecast", get(forecast))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    axum::serve(listener, app).await?;

    Ok(())
}

async fn current(State(state): State<AppState>) -> Json<Value> {
    let cached_value = {
        let cached_entry = state.cached_entries.read().await;
        cached_entry
            .get(CURRENT)
            .cloned()
            .filter(|entry| entry.fetched_at.elapsed().as_secs() < state.config.cache_duration_secs)
    };

    match cached_value {
        None => {
            let json = fetch_current_json(&state).await.unwrap();
            let mut writeable_state = state.cached_entries.write().await;
            writeable_state.insert(
                CURRENT.to_string(),
                CachedEntry {
                    value: json.clone(),
                    fetched_at: Instant::now(),
                },
            );
            Json(json)
        }
        Some(cached_value) => Json(cached_value.value),
    }
}

async fn forecast(State(state): State<AppState>, query: Query<ForecastQueryParams>) -> Json<Value> {
    let geocode = &query.geocode;
    let language = &query.language;
    let cache_key = format!("{FORECAST}_{geocode}_{language}");
    let cached_value = {
        let cached_entry = state.cached_entries.read().await;
        cached_entry
            .get(&cache_key)
            .cloned()
            .filter(|entry| entry.fetched_at.elapsed().as_secs() < state.config.cache_duration_secs)
    };

    match cached_value {
        None => {
            let json = fetch_forecast_json(&geocode, &language, &state)
                .await
                .unwrap();
            let mut writeable_state = state.cached_entries.write().await;
            writeable_state.insert(
                cache_key,
                CachedEntry {
                    value: json.clone(),
                    fetched_at: Instant::now(),
                },
            );
            Json(json)
        }
        Some(cached_value) => Json(cached_value.value),
    }
}

async fn fetch_current_json(state: &AppState) -> Result<Value> {
    let pws_id = state.config.pws_id.clone();
    let api_key = state.config.api_key.clone();

    fetch_json(state, format!("https://api.weather.com/v2/pws/observations/current?stationId={pws_id}&format=json&units=m&apiKey={api_key}&numericPrecision=decimal")).await
}

async fn fetch_forecast_json(geocode: &str, language: &str, state: &AppState) -> Result<Value> {
    let api_key = state.config.api_key.clone();

    fetch_json(state, format!("https://api.weather.com/v3/wx/forecast/daily/5day?geocode={geocode}&format=json&units=m&apiKey={api_key}&language={language}")).await
}

async fn fetch_json(state: &AppState, url: String) -> Result<Value> {
    let res = state
        .client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "gzip")
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(Duration::from_secs(5))
        .send()
        .await?
        .json::<Value>()
        .await?;

    Ok(res)
}
