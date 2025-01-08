#!/usr/bin/env bash

set -e

VERSION=27.0
TARGET_ARCH=x86_64-linux-gnu
BTC_RELEASE=${VERSION}-${TARGET_ARCH}
curl -O "https://bitcoincore.org/bin/bitcoin-core-27.0/bitcoin-${BTC_RELEASE}.tar.gz"

tar xzvf bitcoin-${BTC_RELEASE}.tar.gz
cp bitcoin-${VERSION}/bin/{bitcoind,bitcoin-cli} ./

rm -rf *.tar.gz
rm -rf ./bitcoin-${VERSION}
