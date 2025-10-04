//! Constants used throughout the application.

use std::time::Duration;

/// RPC connection timeout for Neovim instances
pub const NEOVIM_RPC_TIMEOUT: Duration = Duration::from_secs(2);
