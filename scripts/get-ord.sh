#!/usr/bin/env bash

set -e

git clone https://github.com/ordinals/ord.git ord_src
cd ord_src
cargo build --release
cp ./target/release/ord ../ord
cd ..
rm -rf ord_src
