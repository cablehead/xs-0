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
{"id":"039KW46V39SC3LYMJSMHJJJRH","data":"hello world"}
```

IDs are [SCRU128](https://github.com/scru128/rust)s.

### Command-Line Interface

Here are the available commands for `xs`:

```
Usage: xs <PATH> <COMMAND>

Commands:
  put    Insert a payload into the store from stdin, with optional follow
  get    Fetch a specific payload by ID
  cat    Stream payloads from store with optional last_id and follow
  call   Send request to topic, wait for response, print output
  serve  Listen for requests, execute command, write output back
  help   Print this message or the help of the given subcommand(s)

Arguments:
  <PATH>

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## License

`xs` is released under the MIT License. See the `LICENSE` file for more details.

## Contributing

Contributions to `xs` are welcome!
