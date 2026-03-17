#!/bin/bash
# Shell shim for humanize cancel rlcr
# This shim execs the Rust binary from PATH with the correct subcommand
exec humanize cancel rlcr "$@"
