# Developer shortcuts. CI uses the same invocations (see .github/workflows).

# Files excluded from the coverage gate: the entrypoint, the HTTP interface and
# port trait definitions are integration-test territory, not unit-tested.
COV_IGNORE := (main\.rs|interface/|ports/)
# Quality ratchets: set just below the current values so normal fluctuation does
# not turn CI red. Raise them as tests are added; never lower without discussion.
# Minimum line coverage of the business logic (currently ~86%).
COV_MIN := 80
# Minimum mutation score, killed / viable mutants (currently ~69%).
MUTATION_MIN := 65

.PHONY: fmt lint test coverage coverage-html mutants check

fmt:
	cargo fmt --all

lint:
	cargo fmt --all --check
	cargo clippy --all-targets -- -D warnings

test:
	cargo test --all

# Full local gate: format, lint, tests, coverage threshold.
check: lint test coverage

# Line coverage of the business logic; fails below COV_MIN.
coverage:
	cargo llvm-cov --ignore-filename-regex '$(COV_IGNORE)' --fail-under-lines $(COV_MIN) --summary-only

# Open an HTML coverage report in the browser.
coverage-html:
	cargo llvm-cov --ignore-filename-regex '$(COV_IGNORE)' --html --open

# Mutation testing of the business logic (scope in .cargo/mutants.toml);
# fails below MUTATION_MIN.
mutants:
	MUTATION_MIN_SCORE=$(MUTATION_MIN) bash scripts/mutation-score.sh
