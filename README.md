# xs

This is a command-line tool (`xs`) and a Rust library (`xs_lib`) for recording
and retrieving sequential streams of payloads. It uses [Lightning Memory-Mapped
Database](http://www.lmdb.tech/doc/)
([LMDB](https://docs.rs/lmdb-rkv/0.14.0/lmdb/)) for efficient and reliable
local embedded storage.

## Installation

```
cargo install xs-lib
```

## Usage

`xs` is easy to use. Here's an example of recording and retrieving a sequential
stream of payloads:

```shell
# Record a payload
% echo "hello world" | xs ./s put

# Retrieve the payloads
% xs ./s cat
{"id":"039KW46V39SC3LYMJSMHJJJRH","data":"hi there"}
```

IDs are [SCRU128](https://github.com/scru128/rust)s.

### Command-Line Interface

Here are the available commands for `xs`:

```shell
xs 0.2.0

USAGE:
    xs <PATH> <SUBCOMMAND>

ARGS:
    <PATH>    Path to the LMDB environment

OPTIONS:
    -h, --help       Print help information
    -V, --version    Print version information

SUBCOMMANDS:
    call    Call a command with payload as argument
    cat     Retrieve and display all payloads
    get     Retrieve a specific payload
    put     Record a new payload
    serve   Serve payloads via an HTTP server
```

## License

`xs` is released under the MIT License. See the `LICENSE` file for more details.

## Contributing

Contributions to `xs` are welcome!
