use anyhow::{Context, Result};
use std::sync::OnceLock;
use web_time::Duration;

/// Returns a shared `reqwest::Client` with default settings.
///
/// When `ClientRegistry` creates a new `LLMProvider` per request, each provider
/// previously got its own `reqwest::Client` with its own TCP connection pool.
/// Under high concurrency (100+ requests/s), this causes ephemeral port
/// exhaustion because each client opens fresh connections that enter TIME_WAIT.
///
/// By sharing a single client, all providers reuse the same connection pool.
fn default_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        log::warn!("[reqwest-pool] initializing shared default reqwest::Client");
        builder()
            .build()
            .expect("Failed to create default reqwest client")
    })
}

fn builder() -> reqwest::ClientBuilder {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            reqwest::Client::builder()
        } else {
            let danger_accept_invalid_certs = matches!(std::env::var("DANGER_ACCEPT_INVALID_CERTS").as_deref(), Ok("1"));
            reqwest::Client::builder()
                // NB: we can NOT set a total request timeout here: our users
                // regularly have requests that take multiple minutes, due to how
                // long LLMs take
                .connect_timeout(Duration::from_secs(10))
                .danger_accept_invalid_certs(danger_accept_invalid_certs)
                .http2_keep_alive_interval(Some(Duration::from_secs(10)))
                // Re-enable connection pooling with a short idle timeout.
                //
                // Previously pooling was disabled (pool_max_idle_per_host(0),
                // pool_idle_timeout(1ns)) to work around hyper stalling bugs in
                // Python's asyncio context.  However, disabling pooling means
                // every request opens a new TCP connection which enters TIME_WAIT
                // on close (~60s on Linux).  Under high concurrency this exhausts
                // ephemeral ports, causing EADDRNOTAVAIL errors.
                //
                // A 10s idle timeout keeps connections alive long enough for
                // reuse during bursts while still cleaning up aggressively.
                //
                // Original references (for context):
                // https://github.com/seanmonstar/reqwest/issues/600
                // https://github.com/hyperium/hyper/issues/2312
                .pool_idle_timeout(Duration::from_secs(10))
                // Log connection establishment/reuse for diagnostics.
                .connection_verbose(true)
        }
    }
}

pub fn create_client() -> Result<reqwest::Client> {
    Ok(default_client().clone())
}

pub fn create_http_client(
    http_config: &internal_llm_client::HttpConfig,
) -> Result<reqwest::Client> {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            // WASM doesn't support timeouts, use default builder
            reqwest::Client::builder()
                .build()
                .context("Failed to create reqwest client")
        } else {
            // Return the shared global client when the config matches defaults.
            // This is critical for the ClientRegistry path where a new
            // LLMProvider is created per request — without sharing, each
            // provider gets its own connection pool and we exhaust ephemeral
            // ports under sustained load.
            //
            // The shared client uses a 10s connect timeout (see builder()).
            // ensure_http_config() in helpers.rs always sets
            // connect_timeout_ms = Some(10_000) as the default, so we must
            // match on that value — not just None.
            let uses_default_connect_timeout = matches!(
                http_config.connect_timeout_ms,
                None | Some(10_000)
            );

            if uses_default_connect_timeout {
                log::debug!(
                    "[reqwest-pool] returning shared client (connect_timeout_ms={:?})",
                    http_config.connect_timeout_ms
                );
                return Ok(default_client().clone());
            }

            log::debug!(
                "[reqwest-pool] creating NEW client (connect_timeout_ms={:?})",
                http_config.connect_timeout_ms
            );

            let danger_accept_invalid_certs = matches!(std::env::var("DANGER_ACCEPT_INVALID_CERTS").as_deref(), Ok("1"));
            let mut builder = reqwest::Client::builder()
                .danger_accept_invalid_certs(danger_accept_invalid_certs)
                .http2_keep_alive_interval(Some(Duration::from_secs(10)))
                // See comment in builder() above for why pooling is re-enabled.
                .pool_idle_timeout(Duration::from_secs(10));

            // Apply custom connect timeout
            // Note: 0 means infinite timeout (no timeout)
            if let Some(ms) = http_config.connect_timeout_ms {
                if ms > 0 {
                    builder = builder.connect_timeout(Duration::from_millis(ms));
                }
                // If ms == 0, don't set connect_timeout (infinite timeout)
            }

            builder.build().context("Failed to create reqwest client")
        }
    }
}

pub(crate) fn create_tracing_client() -> Result<reqwest::Client> {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let cb = builder();
        } else {
            let cb = builder()
                // Wait up to 30s to send traces to the backend
                .read_timeout(Duration::from_secs(30));

        }
    }

    cb.build().context("Failed to create reqwest client")
}
