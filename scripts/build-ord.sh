#!/usr/bin/env bash

set -e

cd ../../ord
cargo build

cp -f ./target/debug/ord ../runes-dex/scripts/
