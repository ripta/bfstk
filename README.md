bfstk, a brainfuck~ish interpreter

Implementation notes:

- the data pointer starts on cell zero; and
- cells extend (virtually) unlimited to both the negative and positive directions.

To run:

```
cargo run -- examples/hello_world.b
```

or compile first, then run:

```
cargo build --release
./target/release/bfstk examples/hello_word.b
```

Run with `-r` to show a summary report of the state after execution.
