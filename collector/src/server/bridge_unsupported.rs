// SPDX-License-Identifier: MIT
// Copyright (c) 2026 eunomia-bpf org.

//! Unsupported-platform boundary for the Unix-socket evidence bridge.
//!
//! Keep the portable collector callable without pretending that a requested
//! bridge was started. No handle can be constructed on this platform.

use crate::view::SharedMaterializedView;
use crate::view::live_top::SharedLiveView;
use agentsight_protocol::bridge::DisclosureMode;
use std::convert::Infallible;
use std::path::PathBuf;

pub(crate) struct BridgeServerConfig;

impl BridgeServerConfig {
    pub(crate) fn new(_socket_path: PathBuf) -> Self {
        Self
    }
}

pub(crate) struct BridgeServerHandle(Infallible);

impl BridgeServerHandle {
    pub(crate) fn shutdown(&self, _reason: &str) {
        match self.0 {}
    }
}

pub(crate) async fn start_bridge_server(
    _config: BridgeServerConfig,
    _view: SharedMaterializedView,
    _disclosure: DisclosureMode,
    _live_sessions: SharedLiveView,
) -> Result<BridgeServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    Err(crate::cmd_trace::UNSUPPORTED_BRIDGE_MESSAGE.into())
}
