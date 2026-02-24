use anyhow::{Context, Result};
use web_time::Duration;

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
        }
    }
}

pub fn create_client() -> Result<reqwest::Client> {
    builder().build().context("Failed to create reqwest client")
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
            let danger_accept_invalid_certs = matches!(std::env::var("DANGER_ACCEPT_INVALID_CERTS").as_deref(), Ok("1"));
            let mut builder = reqwest::Client::builder()
                .danger_accept_invalid_certs(danger_accept_invalid_certs)
                .http2_keep_alive_interval(Some(Duration::from_secs(10)))
                // See comment in builder() above for why pooling is re-enabled.
                .pool_idle_timeout(Duration::from_secs(10));

            // Apply connect timeout if specified
            // Note: 0 means infinite timeout (no timeout)
            // Defaults were already applied during client creation
            if let Some(ms) = http_config.connect_timeout_ms {
                if ms > 0 {
                    builder = builder.connect_timeout(Duration::from_millis(ms));
                }
                // If ms == 0, don't set connect_timeout (infinite timeout)
            }

            // Note: request_timeout is applied per-request, not on client
            // We'll apply it when building individual requests

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
