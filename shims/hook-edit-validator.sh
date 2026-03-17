#!/bin/bash
# Shell shim for humanize hook edit-validator
# This shim execs the Rust binary with the correct subcommand
exec "${CLAUDE_PLUGIN_ROOT:-/home/cupnfish/.claude/plugins/cache/humania/humanize/1.15.0}/bin/humanize" hook edit-validator
