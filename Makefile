.PHONY: run
run:
	(cd app && npm run build) && cargo r -r server

.PHONY: bench
bench:
	mkdir -p prof && \
	cargo r -r bench && \
	(cd prof && go tool pprof -gif profile.pb)

.PHONY: bench
bench_slow:
	mkdir -p prof && \
	cargo r bench && \
	(cd prof && go tool pprof -gif profile.pb)

.PHONY: criterion
criterion:
	mkdir -p prof && \
	cargo criterion --bench bench -- --profile-time 5 && \
	(cd prof && go tool pprof -gif ../target/criterion/profile/black_advance/profile.pb)