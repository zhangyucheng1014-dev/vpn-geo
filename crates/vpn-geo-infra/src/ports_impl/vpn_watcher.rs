use async_trait::async_trait;
use futures_util::StreamExt;
use log::warn;
use vpn_geo_app::{AppError, AppResult, ports::VpnEventStream};
use zbus::{Connection, MessageStream};

use super::dbus_support;

#[derive(Default)]
pub struct VpnWatcher {
    connection: Option<Connection>,
    stream: Option<MessageStream>,
}

impl VpnWatcher {
    async fn initialize(&mut self) -> AppResult<()> {
        if self.connection.is_none() {
            let connection = dbus_support::connection().await?;
            self.stream = Some(dbus_support::signal_stream(&connection).await?);
            self.connection = Some(connection);
        }
        Ok(())
    }
}

#[async_trait]
impl VpnEventStream for VpnWatcher {
    async fn initial_vpn_active(&mut self) -> AppResult<bool> {
        self.initialize().await?;
        let connection = self
            .connection
            .as_ref()
            .ok_or_else(|| AppError::nm("D-Bus connection is missing"))?;
        Ok(dbus_support::snapshot(connection).await?.vpn_active)
    }

    async fn wait_event(&mut self) -> AppResult<()> {
        loop {
            self.initialize().await?;
            let stream = self
                .stream
                .as_mut()
                .ok_or_else(|| AppError::nm("D-Bus signal stream is missing"))?;
            match stream.next().await {
                Some(Ok(message)) if dbus_support::relevant_signal(&message) => return Ok(()),
                Some(Ok(_)) => continue,
                Some(Err(error)) => {
                    warn!("ignoring malformed D-Bus signal: {error}");
                }
                None => return Err(AppError::nm("system D-Bus event stream closed")),
            }
        }
    }

    async fn current_vpn_active(&mut self) -> AppResult<bool> {
        self.initialize().await?;
        let connection = self
            .connection
            .as_ref()
            .ok_or_else(|| AppError::nm("D-Bus connection is missing"))?;
        Ok(dbus_support::snapshot(connection).await?.vpn_active)
    }
}
