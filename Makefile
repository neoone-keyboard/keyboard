SIM ?= iPhone 17
# Physical iPhone UDID for `make device` (list with: xcrun devicectl list devices)
DEVICE ?= <your-device-id>
PROJECT := apps/ios/MLKeyboard.xcodeproj
CARGO := PATH="$$HOME/.cargo/bin:$$PATH" cargo
XCFRAMEWORK := apps/ios/Packages/MLEngine/MLEngineFFI.xcframework
ENGINE_SOURCES := $(shell find engine -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' | grep -v target)

.PHONY: all tools engine project build device test test-engine test-data test-training test-swift lexicons bench

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

device: tools project $(XCFRAMEWORK)
	xcodebuild -project $(PROJECT) -scheme KeyboardApp -configuration Debug \
	  -destination 'id=$(DEVICE)' -derivedDataPath build/DerivedData -allowProvisioningUpdates build -quiet
	xcrun devicectl device install app --device $(DEVICE) build/DerivedData/Build/Products/Debug-iphoneos/KeyboardApp.app
	xcrun devicectl device process launch --device $(DEVICE) me.jishnu.keyboard

test: test-engine test-data test-training test-swift

test-engine:
	cd engine && $(CARGO) test && $(CARGO) clippy --all-targets -- -D warnings
	./scripts/shipped-crates.sh --check

test-data:
	uv run --quiet --project data/pipeline pytest -q data/pipeline

# The eval harness, through the engine's Python bindings (engine/py, never shipped).
test-training:
	cd training && uv run --quiet pytest -q

# Rebuilds the committed lexicon bundle from data/manifests (downloads about 3 GB).
lexicons:
	./data/build-lexicons.sh

# Suggestion latency on the real bundle, release build: p99 must stay under 8 ms.
bench:
	cd engine && $(CARGO) test --release -p mlengine-core --test bundle -- --include-ignored --nocapture latency

test-swift: $(XCFRAMEWORK)
	swift test --package-path apps/ios/Packages/KeyboardCore
	swift test --package-path apps/ios/Packages/MLEngine
