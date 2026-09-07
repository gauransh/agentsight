// SPDX-License-Identifier: MIT
// Copyright (c) 2026 eunomia-bpf org.

pub mod assets;
#[cfg(unix)]
pub(crate) mod bridge;
#[cfg(not(unix))]
#[path = "bridge_unsupported.rs"]
pub(crate) mod bridge;
pub(crate) mod capability;
pub(crate) mod relay_client;
pub(crate) mod session_runtime;
pub mod web;

pub use web::{NodeMetadata, WebServer};
