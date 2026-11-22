#!/bin/sh
# Which image tags a release gets. Usage: release-tags.sh <git tag> <true|false: prerelease>
# Published stable release tags are read from stdin. Prints the version, then one tag per line.
# A floating tag (1.2, 1, latest) moves only when this is the newest release it covers,
# so a patch to an old line cannot pull users back.
set -eu

stable='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
id='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
pre="^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-$id(\.$id)*$"

tag=$1
prerelease=$2
known=$(grep -E "$stable" || true)

if printf '%s\n' "$tag" | grep -Eq "$stable"; then
    if [ "$prerelease" = true ]; then
        echo "A prerelease needs a suffix, like $tag-rc.1" >&2
        exit 1
    fi
elif printf '%s\n' "$tag" | grep -Eq "$pre"; then
    if [ "$prerelease" != true ]; then
        echo "$tag has a prerelease suffix: mark the GitHub release as a prerelease" >&2
        exit 1
    fi
else
    echo "\"$tag\" is not a version: use vMAJOR.MINOR.PATCH, like v1.2.3 or v1.2.3-rc.1" >&2
    exit 1
fi

version=${tag#v}
echo "$version"
echo "$version"
[ "$prerelease" = true ] && exit 0

major=${version%%.*}
minor=${version%.*}

# The newest of the known releases and this one, among those starting with $1.
newest() {
    printf '%s\n%s\n' "$known" "$tag" | grep -E "$stable" | sed 's/^v//' | grep -E "^$1" | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1
}

[ "$(newest "$(echo "$minor" | sed 's/\./\\./')\\.")" = "$version" ] && echo "$minor"
[ "$(newest "$major\\.")" = "$version" ] && echo "$major"
[ "$(newest '')" = "$version" ] && echo latest
exit 0
