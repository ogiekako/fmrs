.PHONY: run
run:
	(cd app && npm run build) && cargo r -r server

.PHONY: bench
bench:
	mkdir -p prof && \
	cargo r -r bench && \
	(cd prof && go tool pprof -svg profile.pb)

.PHONY: bench
bench_slow:
	mkdir -p prof && \
	cargo r bench && \
	(cd prof && go tool pprof -svg profile.pb)
