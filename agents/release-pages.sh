#!/bin/sh
# Publish the GitHub Release page for each version tag (vX.Y.Z) that has none, oldest first; the
# newest is marked latest. **A Myrme version is released when its page is published** — formicaria's
# `myrme-bump` and its release only take a version that has one (`docs/context/decisions.md`,
# 2026-10-19). CI runs this on a tag only after the gate and the integration test pass on it.
# A page carries notes only: formicaria builds Myrme from the tag. An existing page is never edited.
# Needs `gh` and GH_TOKEN (the CI token); tags fetched with their messages (`git fetch --tags`).
set -eu
tags=$(git tag -l 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V)
newest=$(echo "$tags" | tail -1)
for t in $tags; do
    gh release view "$t" >/dev/null 2>&1 && continue
    v=${t#v}
    {
        git for-each-ref --format='%(contents)' "refs/tags/$t"
        row=$(grep -E "^\| \*{0,2}$v\*{0,2} \|" docs/context/releases.md || true)
        if [ -n "$row" ]; then
            printf '\n| Myrme | Contract | Linked by formicaria | What changed |\n|---|---|---|---|\n%s\n' "$row"
        fi
        printf '\nformicaria builds Myrme from this tag; there is nothing to install on its own.\n'
    } > release-notes.md
    latest=false; [ "$t" = "$newest" ] && latest=true
    gh release create "$t" --verify-tag --title "Myrme $v" --notes-file release-notes.md --latest=$latest
    echo "release-pages: published $t"
done
rm -f release-notes.md
