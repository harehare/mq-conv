# Install

## Install script

```sh
curl -fsSL https://raw.githubusercontent.com/harehare/mq-conv/main/bin/install.sh | bash
```

The installer downloads the latest release for your platform, verifies its SHA-256 checksum, installs it to `~/.mq/bin/` and adds that directory to your `PATH` (bash, zsh or fish). Restart your terminal afterwards.

## Cargo

```sh
cargo install mq-conv
cargo binstall mq-conv   # prebuilt release instead of compiling
```

## From source

```sh
git clone https://github.com/harehare/mq-conv.git
cd mq-conv
cargo build --release    # target/release/mq-conv
```

Prebuilt binaries for macOS, Linux and Windows are on the [releases page](https://github.com/harehare/mq-conv/releases).

## Verify

```sh
mq-conv --version
```
