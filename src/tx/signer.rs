use std::{borrow::Borrow, str::FromStr};

use bitcoin::{
    ecdsa::Signature,
    key::{KeyPair, TapTweak, UntweakedPublicKey},
    script::{Builder, PushBytes},
    secp256k1::{All, Message, Secp256k1, SecretKey, XOnlyPublicKey},
    sighash::{EcdsaSighashType, Prevouts, SighashCache, TapSighashType},
    taproot, Address, Network, PrivateKey, Transaction, TxOut, Witness,
};

#[derive(Clone)]
pub enum AddressMode {
    Legacy(bool),
    Witness,
    Taproot,
}

impl AddressMode {
    pub fn new_from_str(v: &str) -> Self {
        match v {
            "legacy_compressed" => Self::Legacy(true),
            "legacy_uncompressed" => Self::Legacy(false),
            "witnes" => Self::Witness,
            "taproot" => Self::Taproot,
            _ => Self::Witness,
        }
    }
}

#[derive(Clone)]
pub struct PKSigner {
    secp: Secp256k1<All>,
    private_key: PrivateKey,
    address_mode: AddressMode,
    pub net: Network,
    pub kp: KeyPair,
    pub address: Address,
}

impl miniscript::bitcoin::psbt::GetKey for PKSigner {
    type Error = miniscript::bitcoin::psbt::GetKeyError;
    fn get_key<C: miniscript::bitcoin::secp256k1::Signing>(
        &self,
        key_request: miniscript::bitcoin::psbt::KeyRequest,
        secp: &miniscript::bitcoin::secp256k1::Secp256k1<C>,
    ) -> Result<Option<miniscript::bitcoin::PrivateKey>, Self::Error> {
        debug!("GET_KEY: {:?}", key_request);
        match key_request {
            miniscript::bitcoin::psbt::KeyRequest::Bip32(_) => Ok(None),
            miniscript::bitcoin::psbt::KeyRequest::Pubkey(pk) => {
                let privat_key_b = self.private_key.to_string();
                let privat_key = miniscript::bitcoin::PrivateKey::from_str(&privat_key_b).unwrap();
                debug!("GET_KEY: {} == {}", pk, privat_key.public_key(secp));

                if privat_key.public_key(secp).eq(&pk) {
                    Ok(Some(privat_key))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }
}

impl PKSigner {
    pub fn new_from_secret(net: Network, secret: &str, mode: AddressMode) -> anyhow::Result<Self> {
        let secp = Secp256k1::new();
        let data = hex::decode(secret)?;
        let recovered_secret = SecretKey::from_slice(&data)?;
        let kp = KeyPair::from_secret_key(&secp, &recovered_secret);
        let pk: PrivateKey;

        let address = match mode {
            AddressMode::Legacy(compressed) => {
                pk = if compressed {
                    PrivateKey::new(recovered_secret, net)
                } else {
                    PrivateKey::new_uncompressed(recovered_secret, net)
                };

                Address::p2pkh(&pk.public_key(&secp), net)
            }
            AddressMode::Witness => {
                pk = PrivateKey::new(recovered_secret, net);
                Address::p2shwpkh(&pk.public_key(&secp), net).unwrap()
            }
            AddressMode::Taproot => {
                pk = PrivateKey::new(recovered_secret, net);

                let (untw_public_key, _) = UntweakedPublicKey::from_keypair(&kp);
                Address::p2tr(&secp, untw_public_key, None, net)
            }
        };

        Ok(Self {
            secp,
            net,
            address_mode: mode,
            private_key: pk,
            kp,
            address,
        })
    }

    pub fn xonly_pubkey(&self) -> XOnlyPublicKey {
        let (pubkey, _) = XOnlyPublicKey::from_keypair(&self.kp);
        pubkey
    }

    pub fn partial_sign(
        &self,
        otx: &Transaction,
        parent_utxos: Vec<(bool, TxOut)>,
    ) -> anyhow::Result<Vec<Option<taproot::Signature>>> {
        if let AddressMode::Legacy(_) = self.address_mode {
            anyhow::bail!("Legacy signature mode is unsupported for partial signing!");
        }

        if let AddressMode::Witness = self.address_mode {
            anyhow::bail!("Witness signature mode is unsupported for partial signing!");
        }

        let mut tx = otx.clone();
        let sighash_type = TapSighashType::All;
        let mut sighasher = SighashCache::new(&mut tx);
        let mut parents = Vec::new();
        for (_, u) in parent_utxos.iter() {
            parents.push(u.clone());
        }

        let prevouts = Prevouts::All(&parents);
        let mut result = Vec::new();
        for (id, input) in otx.input.iter().enumerate() {
            if !parent_utxos[id].0 {
                result.push(None);
                continue;
            }

            info!(
                "sign utxo: {} input={:?}   {:?}",
                id, input, parent_utxos[id]
            );

            let sighash =
                sighasher.taproot_key_spend_signature_hash(id, &prevouts, sighash_type)?;

            // Sign the sighash using the secp256k1 library (exported by rust-bitcoin).
            let tweaked = self.kp.tap_tweak(&self.secp, None);
            let msg = Message::from(sighash);
            let signature = self.secp.sign_schnorr(&msg, &tweaked.to_inner());

            // Update the witness stack.
            let signature = taproot::Signature {
                sig: signature,
                hash_ty: sighash_type,
            };
            result.push(Some(signature));
        }

        Ok(result)
    }

    pub fn sign_tx(
        &self,
        otx: &Transaction,
        parent_utxos: Vec<TxOut>,
    ) -> anyhow::Result<Transaction> {
        if let AddressMode::Legacy(_) = self.address_mode {
            return self.legacy_sign_tx(otx, parent_utxos);
        }

        if let AddressMode::Witness = self.address_mode {
            anyhow::bail!("Witness signature mode is unsupported for signing!");
        }

        let mut tx = otx.clone();
        let sighash_type = TapSighashType::All;
        let prevouts = Prevouts::All(&parent_utxos);

        let mut sighasher = SighashCache::new(&mut tx);
        for (id, _input) in otx.input.iter().enumerate() {
            let sighash = sighasher
                .taproot_key_spend_signature_hash(id, &prevouts, sighash_type)
                .expect("failed to construct sighash");

            // Sign the sighash using the secp256k1 library (exported by rust-bitcoin).
            let tweaked = self.kp.tap_tweak(&self.secp, None);
            let msg = Message::from(sighash);
            let signature = self.secp.sign_schnorr(&msg, &tweaked.to_inner());

            // Update the witness stack.
            let signature = taproot::Signature {
                sig: signature,
                hash_ty: sighash_type,
            };

            let mut witness = Witness::new();
            witness.push(&signature.to_vec());

            *sighasher.witness_mut(id).unwrap() = witness;
            // *sighasher.witness_mut(id).unwrap() = Witness::p2tr_key_spend(&signature); // TODO in newest ver of lib :(
        }

        // Get the signed transaction.
        let tx = sighasher.into_transaction();
        Ok(tx.to_owned())
    }

    pub fn legacy_sign_tx(
        &self,
        otx: &Transaction,
        parent_utxos: Vec<TxOut>,
    ) -> anyhow::Result<Transaction> {
        let secp = Secp256k1::new();
        let sighash_type = EcdsaSighashType::All;
        let mut tx = otx.clone();
        let public_key = self.private_key.public_key(&secp).to_bytes();

        for (input_index, _input) in otx.input.iter().enumerate() {
            let sb = {
                let sighash_cache = SighashCache::new(tx.borrow());
                let sighash = sighash_cache.legacy_signature_hash(
                    input_index,
                    &parent_utxos[input_index].script_pubkey,
                    sighash_type as u32,
                )?;

                let signature = secp.sign_ecdsa(
                    &Message::from_slice(sighash.as_ref())?,
                    &self.private_key.inner,
                );

                Signature {
                    sig: signature,
                    hash_ty: sighash_type,
                }
                .to_vec()
            };

            let payload: &PushBytes = sb.as_slice().try_into().unwrap();
            let pk_payload: &PushBytes = public_key.as_slice().try_into().unwrap();

            tx.input[input_index].script_sig = Builder::new()
                .push_slice(payload)
                .push_slice(pk_payload)
                .into_script();
            tx.input[input_index].witness.clear();
        }

        Ok(tx)
    }
}
