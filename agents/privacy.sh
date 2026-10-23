#!/bin/sh
# **This repository is public: nothing in it may identify a person** — `pixi run privacy`, part of
# `pixi run ci`. The same guard formicaria's `ci/checks.sh` runs, for the same reason (CLAUDE.md,
# *Never write personal details*): research notes, tests and fixtures here had carried quoted mail,
# names from it, an institution and a test phone's retail model number (cleaned 2026-10-23).
#
# What fails:
# - a `/home/<login>` path — someone's real login (the documentation placeholders `/home/you`,
#   `/home/user` and friends are allowed by name);
# - an address at a personal mail provider;
# - a phone's retail model number, caught by its shape (two year digits, three more, then letters),
#   so no real one is spelled here — the chip class a benchmark rests on stays, a *unit* goes;
# - an account of the owner's own circumstances ("measured on the owner's phone"): write the role
#   instead, "a user's phone", and leave out where they are and who is in their mail.
#
# Real names in fixtures cannot be told apart from invented ones by a pattern: invent them, always.
set -u
cd "$(dirname "$0")/.."
fail=0
ids=$(git grep -nIE '/home/[a-z][a-z0-9_-]*|\b2[0-9]{4}[A-Z]{3,5}[A-Z0-9]?\b|[a-zA-Z0-9._%+-]+@(gmail|outlook|hotmail|yahoo|icloud)\.[a-z]+' \
      -- . ':!agents/privacy.sh' ':!*.lock' 2>/dev/null \
      | grep -vE '/home/(you|ada|alice|bob|user|username|me|someone|USER|x)([^a-z0-9_-]|$)')
if [ -n "$ids" ]; then
    echo "privacy: personal identifier(s) in tracked files:"
    echo "$ids" | head -20 | sed 's/^/    /'
    fail=1
fi
owner=$(git grep -nIiE "owner'?s (own |real |private |personal )?(phones?|laptops?|devices?|vaults?|mail|mailbox|gmail|inbox|conversations|desktop|notes?)\b" \
        -- . ':!agents/privacy.sh' ':!*.lock' 2>/dev/null)
if [ -n "$owner" ]; then
    echo "privacy: the owner's own circumstances described in tracked files:"
    echo "$owner" | head -20 | sed 's/^/    /'
    echo "    Write the role instead — \"a user's phone\", \"a user's vault\"."
    fail=1
fi
[ "$fail" = 0 ] && echo "privacy: ok"
exit "$fail"
