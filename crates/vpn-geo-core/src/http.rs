use std::time::Duration;

use reqwest::{Client, ClientBuilder};

use crate::{Error, Result, VERSION};

pub fn client(timeout: Duration, direct: bool) -> Result<Client> {
    let mut builder = ClientBuilder::new()
        .timeout(timeout)
        .connect_timeout(timeout.min(Duration::from_secs(5)))
        .pool_max_idle_per_host(0)
        .user_agent(format!("vpn-geo/{VERSION}"))
        .redirect(reqwest::redirect::Policy::limited(3));
    if direct {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|error| Error::http(format!("create HTTP client: {error}")))
}
