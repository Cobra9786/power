use bitcoin::{ScriptBuf, Txid};

#[derive(Clone, Debug)]
pub struct Utxo {
    /// The referenced transaction's txid.
    pub txid: Txid,
    /// The index of the referenced output in its transaction's vout.
    pub vout: u32,
    /// The value of the output, in satoshis.
    pub value: u64,
    /// The script which must be satisfied for the output to be spent.
    pub script_pubkey: ScriptBuf,
}
