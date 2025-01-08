use std::collections::HashMap;

use bitcoin::{
    absolute::LockTime,
    opcodes, script,
    script::Builder,
    secp256k1::{KeyPair, Message, Secp256k1, XOnlyPublicKey},
    sighash::{Prevouts, SighashCache, TapSighashType},
    taproot::{ControlBlock, LeafVersion, Signature, TapLeafHash, TaprootBuilder},
    Address, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness,
};
use ordinals::{Etching, Runestone};

use super::utxo::Utxo;

const PROTOCOL_ID: [u8; 3] = *b"ord";
pub const COMMITMENT_OUT_VALUE: u64 = 100_000;
pub const RUNES_OUT_VALUE: u64 = 600;

#[derive(Clone)]
pub struct CommitmentOut {
    vout: usize,
    control_block: ControlBlock,
    reveal_script: ScriptBuf,
    pub commit_tx_address: Address,
    // taproot_spend_info: TaprootSpendInfo,
    out: TxOut,
}

pub struct RunesTxBuilder {
    net: Network,
    commitment_pubkey: XOnlyPublicKey,
    change_address: Address,
    fee_rate: f64,
}

impl RunesTxBuilder {
    pub fn new(
        net: Network,
        commitment_pubkey: XOnlyPublicKey,
        change_address: Address,
        fee_rate: f64,
    ) -> Self {
        // let key_pair = etching_key_pair;
        // let (public_key, _parity) = XOnlyPublicKey::from_keypair(&key_pair);
        Self {
            net,
            commitment_pubkey,
            change_address,
            fee_rate,
        }
    }

    pub fn create_commitment_tx(
        &self,
        etching_outputs: Vec<Etching>,
        utxo: Vec<Utxo>,
        commitment_value: u64,
    ) -> (Transaction, HashMap<String, CommitmentOut>, Vec<TxOut>) {
        let mut tx = Transaction {
            version: 2,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![],
        };

        let mut in_value: u64 = 0;
        let out_amount = etching_outputs.len() as u64 * commitment_value;
        let mut used_utxos = Vec::new();
        for u in utxo {
            if in_value > out_amount {
                break;
            }

            in_value += u.value;

            tx.input.push(TxIn {
                previous_output: OutPoint {
                    txid: u.txid,
                    vout: u.vout,
                },
                script_sig: script::Builder::new().into_script(),
                witness: Witness::new(),
                sequence: Sequence::ZERO,
            });
            used_utxos.push(TxOut {
                script_pubkey: u.script_pubkey,
                value: u.value,
            });
        }

        let mut commitment_outs: HashMap<String, CommitmentOut> = HashMap::new();

        for (index, etching) in etching_outputs.iter().enumerate() {
            let rune_name = etching.rune.unwrap().to_string().clone();
            let out_data = self.craft_commitment_out(etching, index, commitment_value);

            tx.output.push(out_data.out.clone());
            commitment_outs.insert(rune_name, out_data);
        }

        let change_amount = in_value - out_amount;

        tx.output.push(TxOut {
            value: change_amount,
            script_pubkey: self.change_address.script_pubkey(),
        });

        const SIG_GROW_K: f64 = 1.85;
        let fee = ((self.fee_rate * tx.vsize() as f64) * SIG_GROW_K) as u64;

        let change_amount = in_value - out_amount - fee;
        tx.output.last_mut().unwrap().value = change_amount;

        (tx, commitment_outs, used_utxos)
    }

    pub fn create_etching_tx(
        &self,
        etching: &Etching,
        commitment_utxo: CommitmentOut,
        txid: Txid,
        dest_address: Address,
    ) -> Transaction {
        let mut etching_tx = Transaction {
            version: 2,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid,
                    vout: commitment_utxo.vout as u32,
                },
                script_sig: Builder::new().into_script(),
                witness: Witness::new(),

                sequence: Sequence::from_height(Runestone::COMMIT_CONFIRMATIONS - 1),
            }],
            output: Vec::new(),
        };

        let dest_id = 1;

        let runestone = Runestone {
            edicts: Vec::new(),
            mint: None,
            etching: Some(*etching),
            pointer: Some(dest_id),
        };
        let rune_script = runestone.encipher();

        etching_tx.output.push(TxOut {
            script_pubkey: rune_script,
            value: 0,
        });

        // output with premined runes
        etching_tx.output.push(TxOut {
            script_pubkey: dest_address.script_pubkey(),
            value: RUNES_OUT_VALUE,
        });

        etching_tx
    }

    pub fn sign_etching_tx(
        &self,
        otx: &Transaction,
        key_pair: &KeyPair,
        commitment_utxo: CommitmentOut,
        commitment_input: usize,
    ) -> Transaction {
        let mut etching_tx = otx.clone();

        let mut sighash_cache = SighashCache::new(&mut etching_tx);
        let sighash = sighash_cache
            .taproot_script_spend_signature_hash(
                commitment_input,
                &Prevouts::All(&[commitment_utxo.out]),
                TapLeafHash::from_script(&commitment_utxo.reveal_script, LeafVersion::TapScript),
                TapSighashType::All,
            )
            .expect("signature hash should compute");

        let secp256k1 = Secp256k1::new();
        let sig = secp256k1.sign_schnorr(
            &Message::from_slice(sighash.as_ref())
                .expect("should be cryptographically secure hash"),
            key_pair,
        );

        let mut witness = Witness::new();
        witness.push(
            Signature {
                sig,
                hash_ty: TapSighashType::All,
            }
            .to_vec(),
        );

        witness.push(commitment_utxo.reveal_script);
        witness.push(&commitment_utxo.control_block.serialize());

        //*sighash_cache.witness_mut(commitment_input).unwrap() = witness;
        //sighash_cache.into_transaction().to_owned()

        etching_tx.input[commitment_input].witness = witness;

        etching_tx.clone()
    }

    fn craft_commitment_out(&self, etching: &Etching, index: usize, value: u64) -> CommitmentOut {
        let secp256k1 = Secp256k1::new();

        let mut builder = ScriptBuf::builder()
            .push_slice(self.commitment_pubkey.serialize())
            .push_opcode(opcodes::all::OP_CHECKSIG);
        builder = append_reveal_script_to_builder(builder, *etching);
        let reveal_script = builder.into_script();

        println!("COMMINTMENT REVEAL SCRIPT -> {}", reveal_script);

        let taproot_spend_info = TaprootBuilder::new()
            .add_leaf(0, reveal_script.clone())
            .expect("adding leaf should work")
            .finalize(&secp256k1, self.commitment_pubkey)
            .expect("finalizing taproot builder should work");

        let control_block = taproot_spend_info
            .control_block(&(reveal_script.clone(), LeafVersion::TapScript))
            .expect("should compute control block");

        let commit_tx_address = Address::p2tr_tweaked(taproot_spend_info.output_key(), self.net);

        CommitmentOut {
            vout: index,
            control_block,
            reveal_script,
            commit_tx_address: commit_tx_address.clone(),
            //taproot_spend_info,
            out: TxOut {
                script_pubkey: commit_tx_address.script_pubkey(),
                value,
            },
        }
    }
}

fn append_reveal_script_to_builder(mut builder: script::Builder, rune: Etching) -> script::Builder {
    let value = rune.rune.unwrap().commitment();
    let tag: [u8; 1] = [13_u8];

    builder = builder
        .push_opcode(opcodes::OP_FALSE)
        .push_opcode(opcodes::all::OP_IF)
        .push_slice(PROTOCOL_ID)
        .push_opcode(opcodes::OP_FALSE)
        .push_slice::<&script::PushBytes>(tag.as_slice().try_into().unwrap())
        .push_slice::<&script::PushBytes>(value.as_slice().try_into().unwrap())
        .push_opcode(opcodes::all::OP_ENDIF);

    builder
}
