#!/bin/sh
# **Myrme has no clock of its own** — `pixi run clock`, part of `pixi run ci` (`decisions.md`,
# 2026-10-24 later, *Myrme follows formicaria's clock*). Which day it is and which zone a time is
# on are formicaria's answers (`/api/clock`, read through `VaultAccess::clock`); this process's own
# zone can be another device's, or lag a change of zone by minutes, and a meeting read on it moves
# by hours. So no source here may ask the system for its zone or its local time.
#
# What fails, in `crates/*/src` (tests included — a test names its clock):
# - `TimeZone::system`, `Zoned::now` (jiff, the system zone);
# - `now_local`, `UtcOffset::current_local_offset`, `current_local_offset` (time);
# - `chrono::Local`, `Local::now` (chrono).
# `Timestamp::now` and `OffsetDateTime::now_utc` are instants, and stay allowed.
set -u
cd "$(dirname "$0")/.."
hits=$(git grep -nIE 'TimeZone::system|Zoned::now|now_local|current_local_offset|chrono::Local|Local::now|try_system' \
       -- 'crates/*/src/*' 2>/dev/null)
if [ -n "$hits" ]; then
    echo "clock: this process's own zone asked for — use formicaria's clock (VaultAccess::clock):"
    echo "$hits" | head -20 | sed 's/^/    /'
    exit 1
fi
echo "clock: no local clock read"
