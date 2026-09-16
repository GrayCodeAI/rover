GO ?= go
PYTHON ?= python3
export GOTOOLCHAIN ?= local
export CGO_ENABLED := 1

.PHONY: build test race vet fmt fmt-check check demo fuzz install clean
build:
	mkdir -p bin
	$(GO) build -trimpath -o bin/rover ./cmd/rover

test:
	$(GO) test -count=1 ./...

race:
	$(GO) test -race -count=1 ./...

vet:
	$(GO) vet ./...

fmt:
	gofmt -w cmd internal

fmt-check:
	@test -z "$$(gofmt -l cmd internal)" || (gofmt -l cmd internal; exit 1)

check: fmt-check vet test

demo: build
	$(PYTHON) scripts/demo.py --binary bin/rover

fuzz:
	$(GO) test ./internal/config -run='^$$' -fuzz=FuzzDecode -fuzztime=3s -parallel=2
	$(GO) test ./internal/source -run='^$$' -fuzz=FuzzSafeName -fuzztime=3s -parallel=2
	$(GO) test ./internal/assurance -run='^$$' -fuzz=FuzzResultParser -fuzztime=3s -parallel=2

# Explicit user-local installation; intentionally no sudo or network installer.
install: build
	@test ! -e "$(HOME)/.local/bin/rover" || (echo "Refusing to overwrite existing rover. Use ./bin/rover or remove it explicitly."; exit 1)
	install -d "$(HOME)/.local/bin"
	install -m 0755 bin/rover "$(HOME)/.local/bin/rover"

# Only this source checkout's build output. Never deletes user state/worktrees.
clean:
	rm -f bin/rover

.PHONY: demo-extended sdk-test
demo-extended: build
	python3 scripts/demo_extended.py --binary bin/rover
sdk-test:
	python3 -m unittest discover -s sdk/python -p 'test_*.py'
	node --test sdk/typescript/test/*.test.ts
	go test ./sdk/go -count=1 -v
