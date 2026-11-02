SIM ?= iPhone 17
PROJECT := apps/ios/MLKeyboard.xcodeproj
CARGO := PATH="$$HOME/.cargo/bin:$$PATH" cargo

.PHONY: all engine project build test test-engine test-swift

all: engine project build

engine:
	./scripts/build-xcframework.sh

project:
	cd apps/ios && xcodegen generate

build: project
	xcodebuild -project $(PROJECT) -scheme KeyboardApp \
	  -destination 'platform=iOS Simulator,name=$(SIM)' CODE_SIGNING_ALLOWED=NO build -quiet

test: test-engine test-swift

test-engine:
	cd engine && $(CARGO) test && $(CARGO) clippy --all-targets -- -D warnings

test-swift: engine
	swift test --package-path apps/ios/Packages/KeyboardCore
	swift test --package-path apps/ios/Packages/MLEngine
