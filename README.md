# Rust Chess Engine

A UCI-compatible chess engine for standard chess.

## Build

Install Rust, then build the release binary:

```sh
cargo build --release
```

## Use

Add that binary as a UCI engine in a compatible chess GUI or analysis board.

You can also run it from a terminal and send UCI commands:

```sh
target/release/rust-chess-engine
```

```text
uci
isready
position startpos moves e2e4 e7e5
go depth 5
quit
```

The engine accepts standard UCI positions, including `startpos` and FEN, and supports fixed-depth, fixed-time, and clock-based searches.
