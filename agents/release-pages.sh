#!/bin/sh
# Publish (or bring up to date) the GitHub Release page of every version tag vX.Y.Z, oldest first;
# the newest is marked latest. **A Myrme version is released when its page is published** —
# formicaria's `myrme-bump` and its release only take a version that has one
# (`docs/context/decisions.md`, 2026-10-19). CI runs this on a tag only after the gate and the
# integration test pass on it, and on `main` to keep earlier pages in step with their notes.
#
# A page is `docs/releases/<tag>.md` (its first line, `# …`, is the title), followed by a section
# written here: the row of `docs/context/releases.md` and links to the documentation **at that tag**.
# A tag with no notes file fails the run: no version is released without its notes.
# Needs `gh` and GH_TOKEN (the CI token), and the tags fetched with `git fetch --tags`.
set -eu
REPO=https://github.com/formicaria-org/myrme
# **One page per tag** (2026-10-24). Two runs creating the same page at once left two pages for
# v0.9.0, and editing that tag's page then failed every later run, so v0.10.0 was never published.
# The oldest page of a tag is kept; the others, identical copies, are removed. Tags are not touched.
gh api --paginate "repos/formicaria-org/myrme/releases?per_page=100" --jq '.[] | "\(.tag_name) \(.id)"' \
    | sort -k1,1 -k2,2n | awk 'seen[$1]++ { print $2 }' \
    | while read -r id; do
        gh api -X DELETE "repos/formicaria-org/myrme/releases/$id"
        echo "release-pages: removed a second page for one tag (release $id)"
    done
tags=$(git tag -l 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V)
newest=$(echo "$tags" | tail -1)
missing=""
for t in $tags; do
    notes="docs/releases/$t.md"
    if [ ! -f "$notes" ]; then missing="$missing $t"; continue; fi
    v=${t#v}
    title=$(head -1 "$notes" | sed 's/^# *//')
    {
        tail -n +2 "$notes" | sed '1{/^$/d}'
        printf '\n## Compatibility\n\n'
        row=$(grep -E "^\| \*{0,2}$v\*{0,2} \|" docs/context/releases.md || true)
        if [ -n "$row" ]; then
            printf '| Myrme | Contract | Linked by formicaria | Summary |\n|---|---|---|---|\n%s\n\n' "$row"
        fi
        printf 'formicaria builds Myrme from this tag and checks its contract version at compile time; '
        printf 'there is nothing to install on its own. An app links any release of its contract major '
        printf 'whose minor is at least the one it needs.\n\n'
        printf '## Documentation (at %s)\n\n' "$t"
        printf -- '- [README](%s/blob/%s/README.md)\n' "$REPO" "$t"
        if git cat-file -e "$t:docs/manual/assistant.md" 2>/dev/null; then
            printf -- '- [The assistant chapter of the user manual](%s/blob/%s/docs/manual/assistant.md), rendered in [formicaria'"'"'s manual](https://formicaria-org.github.io/formicaria/manual/user/assistant.html)\n' "$REPO" "$t"
        fi
        printf -- '- [The contract with formicaria](%s/blob/%s/docs/context/contract.md)\n' "$REPO" "$t"
        printf -- '- [Every release and the formicaria that links it](%s/blob/%s/docs/context/releases.md)\n' "$REPO" "$t"
        printf -- '- [The methods and their evaluations](%s/tree/%s/methods)\n' "$REPO" "$t"
    } > release-notes.md
    latest=false; [ "$t" = "$newest" ] && latest=true
    if gh release view "$t" >/dev/null 2>&1; then
        # Edit only when the notes or title differ, so a run on `main` changes nothing it need not.
        if [ "$(gh release view "$t" --json body --jq .body)" != "$(cat release-notes.md)" ] \
            || [ "$(gh release view "$t" --json name --jq .name)" != "$title" ]; then
            gh release edit "$t" --title "$title" --notes-file release-notes.md
            echo "release-pages: updated $t"
        fi
    else
        gh release create "$t" --verify-tag --title "$title" --notes-file release-notes.md --latest=$latest
        echo "release-pages: published $t"
    fi
done
rm -f release-notes.md
if [ -n "$missing" ]; then
    echo "release-pages: no notes in docs/releases/ for:$missing — not released." >&2
    exit 1
fi
