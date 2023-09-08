use anyhow::Result;
use etcd_client::Client;
use serde::{Deserialize, Serialize};

use crate::struct_to_json_string;

pub const NODE_ID_PREFIX: &'static str = "nodeid_";

// #[derive(Debug, Serialize, Deserialize, Clone)]
// pub struct NodeInfo {
//     pub id: u64,
//     pub grpc_addr: String,
//     pub http_addr: String,
//     pub attribute: String,
// }

// #[allow(clippy::derive_partial_eq_without_eq)]
// #[derive(Clone, PartialEq, ::prost::Message)]
// pub struct NodeInfo {
//     #[prost(uint64, tag = "1")]
//     pub id: u64,
//     #[prost(string, tag = "2")]
//     pub rpc_addr: ::prost::alloc::string::String,
//     #[prost(string, tag = "3")]
//     pub http_addr: ::prost::alloc::string::String,
//     #[prost(uint32, tag = "4")]
//     pub status: u32,
// }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeInfo {
    pub id: u64,
    pub rpc_addr: String,
    pub http_addr: String,
    pub status: u32,
}
pub async fn register_node(etcd_endpoints: Vec<&str>, node_info: &NodeInfo) -> Result<()> {
    let mut client = Client::connect(etcd_endpoints, None).await?;

    let json_str = struct_to_json_string::<NodeInfo>(&node_info)?;
    let key = NODE_ID_PREFIX.to_string() + &node_info.id.to_string();
    // put kv
    client.put(key.clone(), json_str, None).await?;
    // get kv
    let resp = client.get(key, None).await?;
    if let Some(kv) = resp.kvs().first() {
        println!(
            "Get kv: {{{}: {}}}",
            kv.key_str().unwrap(),
            kv.value_str().unwrap()
        );
    }
    Ok(())
}
