# runes-dex

## Build

1. Setup `rust` language env.
2. Build service using.
3. Get the release binary.

```sh
cargo build --release

cp ./targer/release/runes-dex <work-dir>/runes-dex
```

## Configuration

To run `runes-dex` service your need to prepare only external dependencies 
- **Bitcoin RPC** - due to limitations of the rpc crate that we use, we can work only with `bitcoind` RPC with user/password authorization and as address must be set IP address, not a domain.
- **PostgreSQL** 
- **Redis**

Than fill config file. Example can be found at [config.toml](./config.toml).

#### DB

`runes-dex` uses PostgreSQL as a main databse.

#### BTC Regtest

To set up local dev env
1. Initialize and setup bitcoin regtest - can be found at [scripts directory](./scripts). Check the [get-bitcoind.sh](./scripts/get-bitcoind.sh) and [Makefile](./scripts/Makefile)

2. Generate keypair:

```sh
env RUST_LOG=info ./runes-dex gen-keypair

## OR 

env RUST_LOG=info cargo run -- gen-keypair

```

3. Start generation new blocks to your address. Check the [./generate-blocks.sh](./scripts/generate-blocks.sh).

4. Run app in the background to index btc utxos and runes data.

5. Now in the another shell run command to etch new rune:

```sh

env RUST_LOG=info ./runes-dex -c path/to/config.toml runes-etching --submit --submit-etch

# OR

env RUST_LOG=info cargo run -- -c path/to/config.toml runes-etching --submit --submit-etch
```


## Run

```sh

## Start whole application as one process 
env RUST_LOG=info ./runes-dex -c path/to/config.toml

## OR

## Start indexer and api-server separatedly

env RUST_LOG=info ./runes-dex -c path/to/config.toml api-server

# in the another shell
env RUST_LOG=info ./runes-dex -c path/to/config.toml indexer

```


 
## Profile perf

1. Install `perf`
2. Install `hotspot` - https://github.com/KDAB/hotspot/releases
3. Install `cargo-flamegraph` - https://github.com/flamegraph-rs/flamegraph

How to
- https://rust-lang.github.io/packed_simd/perf-guide/prof/linux.html
- https://nnethercote.github.io/perf-book/profiling.html
