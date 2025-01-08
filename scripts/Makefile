run_bitcoind:
	./bitcoind -chain=regtest -mempoolfullrbf -txindex -datadir=./regtest-data -server -rpcuser=dev -rpcpassword=dev

run_ord:
	./ord --regtest --bitcoin-rpc-username dev --bitcoin-rpc-password dev --bitcoin-data-dir ./regtest-data --data-dir ./ord-data  --index-runes server --http-port 18080

gen_blocks:
	./bitcoin-cli -chain=regtest -rpcuser=dev -rpcpassword=dev generatetoaddress 10 bcrt1pj96k758twyhkmt9vyqpv4q9cayjsqn4j94qzrcs7jrp9uqcva6rq9s3nh2

create_wallet:
	./ord --regtest --bitcoin-rpc-username dev --bitcoin-rpc-password dev --bitcoin-data-dir ./regtest-data  --index-runes wallet --server-url http://127.0.0.1:18080 create
	./ord --regtest --bitcoin-rpc-username dev --bitcoin-rpc-password dev --bitcoin-data-dir ./regtest-data  --index-runes wallet --server-url http://127.0.0.1:18080 receive

create_runes:
	./ord --regtest --bitcoin-rpc-username dev --bitcoin-rpc-password dev --bitcoin-data-dir ./regtest-data  --index-runes wallet --server-url http://127.0.0.1:18080 batch --batch ./batch.yaml --fee-rate 200
