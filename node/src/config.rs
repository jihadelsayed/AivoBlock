//! Configuration for one independently operated AivoBlock node.
//!
//! Environment variables are optional during early development. Node
//! identities, persistent data directories, and listening ports must be
//! unique when running multiple nodes on one machine.

use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeConfig {
    pub chain_id: u32,
    pub node_name: String,
    pub data_dir: PathBuf,
    pub p2p_listen_addr: SocketAddr,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            chain_id: 1, // Development chain only; not a mainnet identifier.
            node_name: "aivoblock-dev".to_owned(),
            data_dir: PathBuf::from("./data"),
            p2p_listen_addr: SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                30333,
            ),
        }
    }
}

impl NodeConfig {
    /// Load node configuration, returning actionable errors for invalid input.
    pub fn from_env() -> Result<Self, String> {
        let defaults = Self::default();

        let chain_id = match env::var("AIVOBLOCK_CHAIN_ID") {
            Ok(raw) => raw.parse::<u32>()
                .map_err(|_| "AIVOBLOCK_CHAIN_ID must be a positive u32".to_owned())?,
            Err(env::VarError::NotPresent) => defaults.chain_id,
            Err(err) => return Err(format!("AIVOBLOCK_CHAIN_ID: {err}")),
        };

        let node_name = env::var("AIVOBLOCK_NODE_NAME")
            .unwrap_or_else(|_| defaults.node_name.clone());

        let data_dir = match env::var("AIVOBLOCK_DATA_DIR") {
            Ok(raw) => PathBuf::from(raw),
            Err(env::VarError::NotPresent) => defaults.data_dir,
            Err(err) => return Err(format!("AIVOBLOCK_DATA_DIR: {err}")),
        };

        let p2p_listen_addr = match env::var("AIVOBLOCK_P2P_LISTEN") {
            Ok(raw) => raw.parse::<SocketAddr>()
                .map_err(|_| "AIVOBLOCK_P2P_LISTEN must be IP:PORT".to_owned())?,
            Err(env::VarError::NotPresent) => defaults.p2p_listen_addr,
            Err(err) => return Err(format!("AIVOBLOCK_P2P_LISTEN: {err}")),
        };

        let config = Self {
            chain_id,
            node_name,
            data_dir,
            p2p_listen_addr,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.chain_id == 0 {
            return Err("Chain ID must be nonzero".to_owned());
        }
        if self.node_name.trim().is_empty() {
            return Err("Node name must not be empty".to_owned());
        }
        if self.data_dir.as_os_str().is_empty() {
            return Err("Data directory must not be empty".to_owned());
        }
        if self.p2p_listen_addr.port() == 0 {
            return Err("P2P port must be nonzero".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(NodeConfig::default().validate().is_ok());
    }

    #[test]
    fn rejects_zero_chain_id() {
        let config = NodeConfig { chain_id: 0, ..NodeConfig::default() };
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_empty_name() {
        let config = NodeConfig { node_name: "  ".into(), ..NodeConfig::default() };
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_port_zero() {
        let config = NodeConfig {
            p2p_listen_addr: "127.0.0.1:0".parse().unwrap(),
            ..NodeConfig::default()
        };
        assert!(config.validate().is_err());
    }
}
