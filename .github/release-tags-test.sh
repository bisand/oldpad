#!/bin/sh
# Checks release-tags.sh. Runs in CI with the other tests.
set -u
script=$(dirname "$0")/release-tags.sh
failed=0

# check <tag> <prerelease> <known releases> <expected output, or "refused">
check() {
    # shellcheck disable=SC2086 # one known release per word
    got=$(printf '%s\n' $3 | sh "$script" "$1" "$2" 2>/dev/null | tr '\n' ' ' | sed 's/ $//')
    [ -n "$got" ] || got=refused
    if [ "$got" != "$4" ]; then
        echo "FAILED: $1 prerelease=$2 known=[$3]: got [$got], wanted [$4]"
        failed=1
    fi
}

check v1.2.3 false "" "1.2.3 1.2.3 1.2 1 latest"
check v1.2.3 false "v1.2.3" "1.2.3 1.2.3 1.2 1 latest"
check v1.2.4 false "v1.2.3 v2.0.0" "1.2.4 1.2.4 1.2 1"
check v1.2.4 false "v1.2.3 v1.3.0 v2.0.0" "1.2.4 1.2.4 1.2"
check v1.2.4 false "v1.2.5" "1.2.4 1.2.4"
check v1.10.0 false "v1.9.0" "1.10.0 1.10.0 1.10 1 latest"
check v10.0.0 false "v9.9.9" "10.0.0 10.0.0 10.0 10 latest"
check v1.2.10 false "v1.2.9" "1.2.10 1.2.10 1.2 1 latest"
check v1.2.3 false "v1.20.0" "1.2.3 1.2.3 1.2"
check v0.1.0 false "" "0.1.0 0.1.0 0.1 0 latest"
check v2.0.0-rc.1 true "v1.0.0" "2.0.0-rc.1 2.0.0-rc.1"
check v1.0.0-alpha-1.2 true "" "1.0.0-alpha-1.2 1.0.0-alpha-1.2"
check v1.0.0 false "not-a-version v0.9.0 v9.0.0-rc.1" "1.0.0 1.0.0 1.0 1 latest"

check v1.2.3 true "" refused
check v1.2.3-rc.1 false "" refused
for bad in 1.2.3 v1.2 v1.2.3.4 v01.2.3 v1.02.3 v1.2.03 v1.2.3- v1.2.3-rc..1 v1.2.3-01 'v1.2.3;id' 'v1.2.3 x' latest main; do
    check "$bad" false "" refused
    check "$bad" true "" refused
done

[ "$failed" = 0 ] && echo "release tags: ok"
exit "$failed"
