.PHONY: run
run:
	(cd app && npm run build) && cargo r -r server

.PHONY: bench
bench:
	cargo build -r && \
	cat ./problems/forest-06-10_97.sfen | time ./target/release/fmrs solve > /dev/null
