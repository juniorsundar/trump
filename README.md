# TRUMP - Transparent Remote Utility, Multiple Protocols

TRUMP is a CLI-based remote management tool designed to provide a seamless interface for interacting with remote servers. While designed with a multi-protocol architecture in mind, the current implementation provides a robust SSH client with advanced local-remote integration features.

## Requirements

To build or install TRUMP, ensure your environment meets the following requirements:

*   **Rust Toolchain**: `cargo` and `rustc` (latest stable recommended).
*   **Build Tools**: A C compiler (GCC, Clang, or MSVC) is required.
*   **Perl**: Required to compile the vendored OpenSSL dependency.
    *   *Linux*: usually pre-installed or via `perl` package.
    *   *Windows*: Strawberry Perl or ActivePerl.

## Installation

### Binary Download (Linux x86_64)
For a quick installation without compiling, you can download the statically linked binary:

```bash
# Download latest release
curl -L -o trump https://github.com/juniorsundar/trump/releases/latest/download/trump-x86_64-linux-musl
chmod +x trump

# You can move it to this location or another directory in your $PATH
sudo mv trump /usr/local/bin/
```

### Via Cargo
You can install directly from the git repository:
```bash
cargo install --git https://github.com/juniorsundar/trump
```

### Via Nix
This project supports Nix flakes. You can run it directly or install it to your profile:

```bash
# Run directly
nix run github:juniorsundar/trump

# Install to profile
nix profile install github:juniorsundar/trump
```

### From Source
1.  Clone the repository.
2.  Build using cargo:
    ```bash
    cargo build --release
    ```
3.  The binary will be located at `./target/release/trump`.

## Usage

Start an interactive session by connecting to a target:

```bash
trump ssh user@hostname
# or specify a port
trump ssh user@hostname:2222
```

### REPL Commands
Once connected, you enter the TRUMP shell. This shell allows you to interact with the remote server while leveraging local tools.

*   **`list [flags]`**: List contents of the current remote directory (aliases to remote `ls`).
*   **`cd <path>`**: Change the remote working directory.
*   **`cwd`**: Display the current remote working directory.
*   **`cat <file>`**: output the contents of a remote file to stdout.
*   **`edit <remote_file> [local_dest]`**: Downloads the remote file to a temporary location (or specified path), opens it in your local `$EDITOR`, and uploads changes back to the server upon save and exit.
*   **`copy <remote_path> [local_dest]`**: Recursively copies a remote file or directory to your local machine.
*   **`! <command>`**: Execute a raw shell command on the remote server (e.g., `! git status` or `! docker ps`).
