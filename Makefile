SIM ?= iPhone 17
PROJECT := apps/ios/MLKeyboard.xcodeproj
CARGO := PATH="$$HOME/.cargo/bin:$$PATH" cargo
XCFRAMEWORK := apps/ios/Packages/MLEngine/MLEngineFFI.xcframework
ENGINE_SOURCES := $(shell find engine -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' | grep -v target)

.PHONY: all tools engine project build test test-engine test-swift

all: build

tools:
	@command -v xcodegen >/dev/null || { echo "xcodegen not found. Install it with: brew install xcodegen"; exit 1; }
	@test -x "$$HOME/.cargo/bin/rustup" || { echo "rustup not found. Install it from https://rustup.rs"; exit 1; }

engine: tools $(XCFRAMEWORK)

$(XCFRAMEWORK): $(ENGINE_SOURCES) scripts/build-xcframework.sh | tools
	./scripts/build-xcframework.sh

project: | tools
	cd apps/ios && xcodegen generate

build: tools project $(XCFRAMEWORK)
	xcodebuild -project $(PROJECT) -scheme KeyboardApp \
	  -destination 'platform=iOS Simulator,name=$(SIM)' CODE_SIGNING_ALLOWED=NO build -quiet

test: test-engine test-swift

test-engine:
	cd engine && $(CARGO) test && $(CARGO) clippy --all-targets -- -D warnings

test-swift: $(XCFRAMEWORK)
	swift test --package-path apps/ios/Packages/KeyboardCore
	swift test --package-path apps/ios/Packages/MLEngine
