use bitcoin::{hashes::Hash, Txid};
use ordinals::{Etching, Runestone, SpacedRune, Terms};
use std::str::FromStr;

use super::models::Rune;

pub fn reserved_rune() -> Rune {
    let sp = SpacedRune::from_str("UNCOMMON•GOODS").unwrap();
    let etching = Etching {
        divisibility: Some(0),
        symbol: Some('⧉'),
        turbo: true,
        rune: Some(sp.rune),
        spacers: Some(sp.spacers),
        premine: Some(0),
        terms: Some(Terms {
            amount: Some(1),
            cap: Some(340282366920938463463374607431768211455),
            height: (Some(840000), Some(1050000)),
            offset: (None, None),
        }),
    };
    let runestone = Runestone {
        etching: Some(etching),
        mint: None,
        pointer: None,
        edicts: Vec::new(),
    };
    let max_supply = etching.supply().unwrap_or_default();
    let premine = etching.premine.unwrap_or_default();

    Rune {
        id: 0,
        rune: sp.rune.to_string(),
        display_name: sp.to_string(),
        symbol: etching.symbol.unwrap_or('¤').to_string(),
        block: 1,
        tx_id: 0,
        mints: 0,
        premine: "0".to_string(),
        burned: "0".to_string(),
        max_supply: max_supply.to_string(),
        minted: premine.to_string(),
        in_circulation: premine.to_string(),
        divisibility: etching.divisibility.unwrap_or_default() as i32,
        turbo: etching.turbo,
        timestamp: 0,
        etching_tx: Txid::all_zeros().to_string(),
        commitment_tx: Txid::all_zeros().to_string(),
        raw_data: runestone.encipher().into_bytes(),
    }
}
