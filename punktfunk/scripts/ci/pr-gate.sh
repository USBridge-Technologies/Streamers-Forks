#!/bin/sh
# Decide whether a pull request runs the expensive CI lanes. Reads the Gitea pull-request JSON
# on stdin and prints `heavy=true` or `heavy=false`.
#
# Draft = cheap gates only. Ready for review, or labelled `ci:all`, = everything. The fleet
# cannot absorb a full run per push across many open pull requests, and Gitea does not cancel a
# superseded pull-request run (gitea#35933), so each push stacks another one on the last.
#
# Fails OPEN: unreadable input prints `heavy=true`. A gate that silently skips CI is worse than
# one that costs runner time, because only the second is visible.
#
# With `--label ci:<platform>` the draft rule does not apply and the build is label-only: a
# platform lane costs too much to run on every pull request that happens to be ready. That is what
# the `ci:android` / `ci:apple` / … labels already in this repo mean.
#
#     sh scripts/ci/pr-gate.sh < pr.json
#     sh scripts/ci/pr-gate.sh --label ci:android < pr.json
#     sh scripts/ci/pr-gate.sh --self-test

set -eu

ALL=ci:all
PLATFORM=

decide() {
    body=$1
    if [ -n "$PLATFORM" ]; then
        # Label-only. Unreadable input still fails open.
        case "$body" in
            '') printf 'heavy=true\n'; return 0 ;;
        esac
        case "$body" in
            *"\"name\":\"$ALL\""* | *"\"name\":\"$PLATFORM\""*) printf 'heavy=true\n' ;;
            *) printf 'heavy=false\n' ;;
        esac
        return 0
    fi
    # Gitea prints `"draft":false` with no spaces. Anchor on the comma/brace that follows so a
    # title containing the literal text cannot match.
    # `false` is tested first so a body carrying both readings resolves to "run everything".
    case "$body" in
        *'"draft":false,'* | *'"draft":false}'*) draft=0 ;;
        *'"draft":true,'* | *'"draft":true}'*) draft=1 ;;
        *) printf 'heavy=true\n'; return 0 ;;  # no draft field at all — not a PR body we know
    esac
    [ "$draft" -eq 0 ] && { printf 'heavy=true\n'; return 0; }
    # Draft. The label is the override; match it inside a `"name":"..."` pair so a branch or
    # title mentioning the label cannot turn the lanes on.
    case "$body" in
        *"\"name\":\"$ALL\""*) printf 'heavy=true\n' ;;
        *) printf 'heavy=false\n' ;;
    esac
}

self_test() {
    fails=0
    check() {
        got=$(decide "$2")
        if [ "$got" != "$3" ]; then
            echo "pr-gate self-test: $1: expected $3, got $got" >&2
            fails=$((fails + 1))
        fi
    }
    check ready        '{"number":1,"draft":false,"labels":[]}'                      heavy=true
    check draft        '{"number":1,"draft":true,"labels":[]}'                      heavy=false
    check draft-label  '{"number":1,"draft":true,"labels":[{"name":"ci:all"}]}'     heavy=true
    check draft-other  '{"number":1,"draft":true,"labels":[{"name":"ci:android"}]}'  heavy=false
    check garbage      'not json at all'                                            heavy=true
    check empty        ''                                                           heavy=true
    check draft-last   '{"number":1,"draft":true}'                                  heavy=false
    # A title quoting the field must not flip the decision.
    check title-spoof  '{"title":"fix \"draft\":false, really","draft":true,"labels":[]}' heavy=false
    # A branch named after the label must not turn the lanes on.
    check label-spoof  '{"head":{"ref":"ci:all"},"draft":true,"labels":[]}'         heavy=false
    # Ambiguity must resolve toward running everything, never toward skipping it.
    check both-values  '{"draft":true,"x":{"draft":false},"labels":[]}'              heavy=true

    # Platform mode: label-only, and a ready pull request alone is not enough.
    PLATFORM=ci:android
    check plat-none    '{"draft":false,"labels":[]}'                                 heavy=false
    check plat-match   '{"draft":false,"labels":[{"name":"ci:android"}]}'            heavy=true
    check plat-all     '{"draft":true,"labels":[{"name":"ci:all"}]}'                 heavy=true
    check plat-other   '{"draft":false,"labels":[{"name":"ci:apple"}]}'              heavy=false
    check plat-empty   ''                                                            heavy=true
    # `ci:android` must not be satisfied by a longer name that contains it.
    check plat-prefix  '{"draft":false,"labels":[{"name":"ci:android-tv"}]}'         heavy=false
    PLATFORM=

    [ "$fails" -eq 0 ] || return 1
    echo "pr-gate: self-test passed"
}

case "${1:-}" in
    --self-test)
        self_test
        exit $?
        ;;
    --label)
        [ -n "${2:-}" ] || { echo "pr-gate: --label needs a label name" >&2; exit 2; }
        PLATFORM=$2
        ;;
    '') ;;
    *)
        echo "usage: pr-gate.sh [--self-test | --label ci:<platform>]" >&2
        exit 2
        ;;
esac

decide "$(cat)"
