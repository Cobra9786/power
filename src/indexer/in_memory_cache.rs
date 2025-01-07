use bitcoin::{Address, Network, ScriptBuf};
use std::collections::HashMap;
use std::str::FromStr;

use crate::db;

#[derive(Default)]
pub struct BtcIndexCache {
    pub btc_scripts: HashMap<ScriptBuf, String>,
    pub btc_balances: HashMap<String, i64>,
}

impl BtcIndexCache {
    pub fn init_btc_balances(&mut self, net: Network, watchlist: Vec<db::BtcBalance>) {
        for el in watchlist.iter() {
            let a = Address::from_str(&el.address)
                .unwrap()
                .require_network(net)
                .unwrap();

            self.btc_scripts
                .insert(a.script_pubkey(), el.address.clone());
            self.btc_balances.insert(el.address.clone(), el.balance);
        }
    }

    pub fn decrease_btc_balance(&mut self, address: &str, value: i64) -> i64 {
        let balance = self.btc_balances.entry(address.to_owned()).or_default();
        *balance -= value;
        *balance
    }

    pub fn increase_btc_balance_if_present(
        &mut self,
        script: &ScriptBuf,
        value: i64,
    ) -> Option<(String, i64)> {
        let Some(address) = self.btc_scripts.get(script).cloned() else {
            return None;
        };
        Some((address.clone(), self.increase_btc_balance(&address, value)))
    }

    pub fn increase_btc_balance(&mut self, address: &str, value: i64) -> i64 {
        let balance = self.btc_balances.entry(address.to_owned()).or_default();
        *balance += value;
        *balance
    }
}
