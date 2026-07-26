#!/usr/bin/env bash
#
# Run cargo-mutants and gate on the mutation score (killed / viable mutants).
# cargo-mutants has no built-in percentage threshold, so we compute it here.
#
# Usage:  MUTATION_MIN_SCORE=25 scripts/mutation-score.sh [extra cargo-mutants args]
#
set -uo pipefail

MIN="${MUTATION_MIN_SCORE:-25}"
OUT="mutants.out"

# cargo-mutants exits non-zero when mutants survive (2) or time out (3); those
# are outcomes we score, not run failures. Don't let them abort the script.
cargo mutants "$@"
status=$?
case "$status" in
  0 | 2 | 3) ;;
  *)
    echo "cargo-mutants failed to run (exit ${status})"
    exit "${status}"
    ;;
esac

count() { [ -f "${OUT}/$1" ] && wc -l <"${OUT}/$1" || echo 0; }
caught=$(count caught.txt)
missed=$(count missed.txt)
timeout=$(count timeout.txt)
viable=$((caught + missed))

if [ "${viable}" -eq 0 ]; then
  echo "No viable mutants were produced — check the cargo-mutants run."
  exit 1
fi

score=$((caught * 100 / viable))
echo "Mutation score: ${score}% (${caught} killed / ${viable} viable; ${missed} survived, ${timeout} timed out)"

if [ "${score}" -lt "${MIN}" ]; then
  echo "::error::Mutation score ${score}% is below the ${MIN}% threshold"
  exit 1
fi
echo "Mutation score OK (>= ${MIN}%)"
