#!/bin/bash
# Shell shim for humanize setup rlcr
# This shim execs the Rust binary from PATH with the correct subcommand
exec humanize setup rlcr "$@"
