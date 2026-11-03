SIM ?= iPhone 17
# Physical iPhone UDID for `make device` (list with: xcrun devicectl list devices)
DEVICE ?= <your-device-id>
PROJECT := apps/ios/MLKeyboard.xcodeproj
CARGO := PATH="$$HOME/.cargo/bin:$$PATH" cargo
XCFRAMEWORK := apps/ios/Packages/MLEngine/MLEngineFFI.xcframework
# Stamped into the keyboard extension for its diagnostics report.
GIT_COMMIT := $(shell git describe --always --dirty 2>/dev/null)
ENGINE_SOURCES := $(shell find engine -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' | grep -v target) data/chat-forms.tsv

.PHONY: all tools engine project build device test test-engine test-data test-training test-swift lexicons models bench eval spike-assets

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
	  -destination 'platform=iOS Simulator,name=$(SIM)' CODE_SIGNING_ALLOWED=NO ML_GIT_COMMIT=$(GIT_COMMIT) build -quiet

device: tools project $(XCFRAMEWORK)
	xcodebuild -project $(PROJECT) -scheme KeyboardApp -configuration Debug \
	  -destination 'id=$(DEVICE)' -derivedDataPath build/DerivedData -allowProvisioningUpdates ML_GIT_COMMIT=$(GIT_COMMIT) build -quiet
	xcrun devicectl device install app --device $(DEVICE) build/DerivedData/Build/Products/Debug-iphoneos/KeyboardApp.app
	xcrun devicectl device process launch --device $(DEVICE) me.jishnu.keyboard

test: test-engine test-data test-training test-swift eval

test-engine:
	cd engine && $(CARGO) test && $(CARGO) clippy --all-targets -- -D warnings
	./scripts/shipped-crates.sh --check

test-data:
	uv run --quiet --project data/pipeline pytest -q data/pipeline

# The eval harness, through the engine's Python bindings (engine/py, never shipped), and the
# transliterator model tests, which need torch from the `train` group.
test-training:
	cd training && uv run --quiet --group train pytest -q

# Transliteration accuracy and English no-harm on data/eval, against data/eval/baselines.json,
# with the bundled transliterator. Fails on a regression past the recorded baseline. Needs only
# committed files (the lexicons, the model and the eval sets), not the training data.
TRANSLIT_MODEL := apps/ios/KeyboardExtension/Models/translit.mlkbnn
eval:
	cd training && uv run --quiet python -m mlkb.eval --translit $(CURDIR)/$(TRANSLIT_MODEL)

# Rebuilds the committed lexicon bundle from data/manifests (downloads about 3 GB).
lexicons:
	./data/build-lexicons.sh

# Rebuilds the bundled transliterator: training pairs (needs the Dakshina and Aksharantar
# extracts in data/raw, see data/manifests), training (several hours; skipped when
# training/build/translit.pt exists), int8 export, then the copy into the keyboard bundle.
models:
	@if [ ! -f training/build/translit.pt ]; then \
	  cd training && uv run --group train python -m mlkb.data.pairs && \
	  uv run --group train python -m mlkb.translit.train; \
	fi
	cd training && uv run --group train python -m mlkb.translit.export \
	  --checkpoint build/translit.pt --out build/translit.mlkbnn --reference build/translit-reference.json
	mkdir -p $(dir $(TRANSLIT_MODEL))
	cp training/build/translit.mlkbnn $(TRANSLIT_MODEL)

# Suggestion latency on the real bundle, release build: p99 must stay under 8 ms. Then the
# int8 transformer's encode + 10 steps x beam 4 (a random full-size model and the
# bundled one). The suggest gate runs the bundled transliterator, as the keyboard does. Then
# the LM (training/build/lm.mlkbnn, skipped when missing): commit p99 must stay under 1 ms,
# and the sliding context cache must predict within 2% of fresh windows.
bench:
	cd engine && $(CARGO) test --release -p mlengine-core --test bundle -- --include-ignored --nocapture latency
	cd engine && $(CARGO) test --release -p mlengine-core --test nn_reference -- --include-ignored --nocapture nn_latency
	cd engine && $(CARGO) test --release -p mlengine-core --test lm_reference -- --include-ignored --nocapture --test-threads=1 lm_latency ring_matches

test-swift: $(XCFRAMEWORK)
	swift test --package-path apps/ios/Packages/KeyboardCore
	swift test --package-path apps/ios/Packages/MLEngine

# Runtime spike (M3 design §5): a seeded random full-size transliterator as .mlkbnn and as an
# int8 Core ML model, copied into the app (not the keyboard) for the debug "Runtime check"
# screen. Git-ignored; run before `make device` for the spike. coremltools 9 has no native
# libraries for Python 3.14, so this uses its own Python 3.13 environment.
SPIKE_ASSETS := apps/ios/KeyboardApp/SpikeAssets
spike-assets:
	cd training && UV_PROJECT_ENVIRONMENT=build/spike-venv uv run --quiet --python 3.13 \
	  --group spike --group train python -W ignore -m mlkb.translit.coreml_export --out-dir build/spike
	rm -rf $(SPIKE_ASSETS) && mkdir -p $(SPIKE_ASSETS)
	cp training/build/spike/translit-spike.mlkbnn $(SPIKE_ASSETS)/
	cp -R training/build/spike/translit-spike.mlmodelc $(SPIKE_ASSETS)/
