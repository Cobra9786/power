#!/usr/bin/env bash

set -e
echo "Generating a block every 5s. Press [CTRL+C] to stop.."

while :
do
  echo "Generate a new block `date '+%d/%m/%Y %H:%M:%S'`"
  ./bitcoin-cli -chain=regtest -rpcuser=dev -rpcpassword=dev generatetoaddress 1 "${1}"
  sleep 5
done
