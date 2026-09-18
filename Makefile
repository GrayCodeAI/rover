GO ?= go
PYTHON ?= python3
export GOTOOLCHAIN ?= local
export CGO_ENABLED := 1

.PHONY: build test race vet fmt fmt-check check demo fuzz install clean cross-build sbom vulncheck
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

# Cross-compile for supported platforms. Linux uses CGO with system SQLite
# (host build). Darwin uses CGO_ENABLED=0 (terminal/pty are now pure-Go).
# Windows is not yet supported: the codebase uses syscall.O_NOFOLLOW which
# does not exist on Windows. See docs/acceptance/implementation-map.json.
cross-build:
	$(GO) build -trimpath -o bin/rover-linux-amd64 ./cmd/rover
	CGO_ENABLED=0 GOOS=darwin GOARCH=amd64 $(GO) build -trimpath -o bin/rover-darwin-amd64 ./cmd/rover
	CGO_ENABLED=0 GOOS=darwin GOARCH=arm64 $(GO) build -trimpath -o bin/rover-darwin-arm64 ./cmd/rover

# Generate a Software Bill of Materials (SBOM) listing all module dependencies.
# Uses go's built-in module introspection — no external tools required.
# CycloneDX and SPDX consumers can map the JSON output in CI.
sbom:
	mkdir -p bin
	$(GO) list -m -json all > bin/rover-sbom.json
	$(GO) version -m bin/rover > bin/rover-buildinfo.txt 2>/dev/null || true

# Run govulncheck to scan for known vulnerabilities in dependencies.
vulncheck:
	$(GO) run golang.org/x/vuln/cmd/govulncheck@latest ./...

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
