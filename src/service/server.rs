use std::sync::Arc;

use futures::FutureExt;
use tokio::net::TcpListener;

use crate::config::Config;
use crate::proto::Client;
use crate::proxy;
use crate::server::{self, Server};
use crate::service;
use crate::status;
use crate::util::error::{quit_error, ErrorHints};

/// Start lazymc.
pub async fn service(config: Arc<Config>) -> Result<(), ()> {
    // Load server state
    let server = Arc::new(Server::default());

    // Listen for new connections
    // TODO: do not drop error here
    let listener = TcpListener::bind(config.public.address)
        .await
        .map_err(|err| {
            quit_error(
                anyhow!(err).context("Failed to start proxy server"),
                ErrorHints::default(),
            );
        })?;

    info!(
        target: "lazymc",
        "Proxying public {} to server {}",
        config.public.address, config.server.address,
    );

    // Spawn server monitor and signal handler services
    tokio::spawn(service::monitor::service(config.clone(), server.clone()));
    tokio::spawn(service::signal::service(config.clone(), server.clone()));

    // Initiate server start
    if config.server.wake_on_start {
        Server::start(config.clone(), server.clone());
    }

    // Proxy all incomming connections
    while let Ok((inbound, _)) = listener.accept().await {
        let client = Client::default();

        let online = server.state() == server::State::Started;
        if !online {
            // When server is not online, spawn a status server
            let transfer =
                status::serve(client, inbound, config.clone(), server.clone()).map(|r| {
                    if let Err(err) = r {
                        warn!(target: "lazymc", "Failed to serve status: {:?}", err);
                    }
                });

            tokio::spawn(transfer);
        } else {
            // When server is online, proxy all
            let transfer = proxy::proxy(inbound, config.server.address).map(|r| {
                if let Err(err) = r {
                    warn!(target: "lazymc", "Failed to proxy: {}", err);
                }
            });

            tokio::spawn(transfer);
        }
    }

    Ok(())
}
