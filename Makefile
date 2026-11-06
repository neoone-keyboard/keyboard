SIM ?= iPhone 17
# Physical iPhone UDID for `make device` (list with: xcrun devicectl list devices)
DEVICE ?= <your-device-id>
PROJECT := apps/ios/MLKeyboard.xcodeproj
CARGO := PATH="$$HOME/.cargo/bin:$$PATH" cargo
XCFRAMEWORK := apps/ios/Packages/MLEngine/MLEngineFFI.xcframework
# Stamped into the keyboard extension for its diagnostics report.
GIT_COMMIT := $(shell git describe --always --dirty 2>/dev/null)
ENGINE_SOURCES := $(shell find engine -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' | grep -v target) data/chat-forms.tsv

.PHONY: golden golden-save golden-check pack all tools engine project build device test test-engine test-data test-training test-swift lexicons emoji lm-data models bench eval eval-swipe spike-assets pack-lexicons pack-models

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
	for pack in packs/*.neopack; do python3 -I scripts/pack.py $$pack --check || exit 1; done
	uv run --quiet --project data/pipeline pytest -q data/pipeline

# The eval harness, through the engine's Python bindings (engine/py, never shipped), and the
# transliterator model tests, which need torch from the `train` group.
test-training:
	cd training && uv run --quiet --group train pytest -q

# Transliteration accuracy and English no-harm on data/eval, against data/eval/baselines.json,
# with the bundled transliterator and the bundled LM (when present). Fails on a regression
# past the recorded baseline. Needs only committed files (the lexicons, the models and the eval
# sets), not the training data.
TRANSLIT_MODEL := packs/ml.neopack/translit.mlkbnn
LM_MODEL := packs/ml.neopack/lm.mlkbnn
eval:
	cd training && uv run --quiet python -m mlkb.eval --translit $(CURDIR)/$(TRANSLIT_MODEL) \
	  $(if $(wildcard $(LM_MODEL)),--lm $(CURDIR)/$(LM_MODEL))

# Swipe accuracy on synthetic paths, checked against data/eval/baselines-swipe.json.
eval-swipe:
	cd engine && cargo test -p mlengine-core --release --test swipe_eval swipe_eval -- --ignored --nocapture

# Every suggestion over the eval inputs (engine/core/tests/golden_dump.rs), for refactors that
# must not change output. golden-save records it in data/golden/m6p1.tsv.gz; golden-check
# fails on any difference from that record.
GOLDEN := data/golden/m6p1.tsv.gz
golden:
	cd engine && $(CARGO) test -q --release -p mlengine-core --test golden_dump -- --ignored
golden-save: golden
	mkdir -p data/golden && gzip -9nc build/golden/now.tsv > $(GOLDEN)
golden-check: golden
	@gzip -dc $(GOLDEN) | diff - build/golden/now.tsv > build/golden/diff.txt \
	  && echo "golden: identical" \
	  || { echo "golden: $$(grep -c '^>' build/golden/diff.txt) lines differ (build/golden/diff.txt)"; head -20 build/golden/diff.txt; exit 1; }

# The Malayalam pack as a .neopack file (a zip of packs/ml.neopack), for import or download.
pack:
	python3 -I scripts/pack.py packs/ml.neopack --zip build/packs/ml.neopack

# Rebuilds the committed lexicon bundle from data/manifests (downloads about 3 GB).
lexicons:
	./data/build-lexicons.sh

# A pack whose romanisation is data (packs/<lang>.neopack/romanisation.json), not Malayalam.
# Two kinds of pack language, told apart by their config:
#  - training/mlkb/langs/$(PACK_LANG).json + data/lang/$(PACK_LANG).conf (hi): lexicons by
#    lexbuild --lang from the conf's sources; pairs by mlkb.data.pairs (Dakshina and Aksharantar
#    extracts, see data/manifests); held-out accuracy by mlkb.translit.accuracy into
#    data/eval/$(PACK_LANG)/translit-accuracy.json.
#  - data/langs/$(PACK_LANG)/lang.json (as): lexicons by lexbuild-native and the romanisation
#    tables there; pairs by mlkb.langs.pairs (Aksharantar); held-out accuracy printed by
#    mlkb.langs.eval_translit.
# pack-lexicons builds the lexicons (`lexicons` first if en.fst changed); pack-models the
# transliterator: training pairs, training (about 1.5 hours; skipped when
# training/build/$(PACK_LANG)/translit.pt exists), int8 export, held-out accuracy on
# data/eval/$(PACK_LANG)/translit-human.tsv, and the copy into the pack. No LM yet: the LM
# pipeline is Malayalam's only.  make pack-lexicons pack-models PACK_LANG=as
PACK_LANG ?= hi
PACK_DIR := packs/$(PACK_LANG).neopack
PACK_BUILD := build/$(PACK_LANG)
PACK_KIND := $(if $(wildcard training/mlkb/langs/$(PACK_LANG).json),data,langs)
pack-lexicons:
	./data/build-pack-lexicons.sh $(PACK_LANG)

pack-models:
	@if [ ! -f training/$(PACK_BUILD)/translit.pt ]; then \
	  cd training && uv run --group train python -m mlkb.$(PACK_KIND).pairs --lang $(PACK_LANG) --out $(PACK_BUILD)/pairs && \
	  uv run --group train python -m mlkb.translit.train --lang $(PACK_LANG) --pairs $(PACK_BUILD)/pairs \
	    --out $(PACK_BUILD)/translit.pt; \
	fi
	cd training && uv run --group train python -m mlkb.translit.export --lang $(PACK_LANG) \
	  --checkpoint $(PACK_BUILD)/translit.pt --out $(PACK_BUILD)/translit.mlkbnn \
	  --reference $(PACK_BUILD)/translit-reference.json
ifeq ($(PACK_KIND),data)
	cd training && uv run --group train python -m mlkb.translit.accuracy --lang $(PACK_LANG) \
	  --model $(PACK_BUILD)/translit.mlkbnn --out $(CURDIR)/data/eval/$(PACK_LANG)/translit-accuracy.json
else
	cd training && uv run --group train python -m mlkb.langs.eval_translit --lang $(PACK_LANG) \
	  --model $(PACK_BUILD)/translit.mlkbnn
endif
	cp training/$(PACK_BUILD)/translit.mlkbnn $(PACK_DIR)/translit.mlkbnn
	python3 -I scripts/pack.py $(PACK_DIR) --write

# Rebuilds the committed emoji files (the panel and the suggestions) from Unicode's
# emoji-test.txt and the CLDR annotations in data/manifests (downloads about 1.5 MB).
emoji:
	./data/build-emoji.sh

# The LM's training text, vocabulary and token shards in training/build/lm (design §5; needs
# data/fetch.sh and the lexicon text in data/raw/text, about 2 hours).
lm-data:
	cd training && uv run --group train python -m mlkb.lm.sources && \
	  uv run --group train python -m mlkb.lm.corpus --out build/lm/text && \
	  uv run --group train python -m mlkb.lm.tokenizer --text build/lm/text --out build/lm && \
	  uv run --group train python -m mlkb.lm.shards --text build/lm/text --vocab build/lm/pieces.tsv \
	    --out build/lm/shards

# Rebuilds the bundled models. The transliterator: training pairs (needs the Dakshina and
# Aksharantar extracts in data/raw, see data/manifests), training (several hours; skipped when
# training/build/translit.pt exists), int8 export, then the copy into the keyboard bundle. Then
# the LM: training (`make lm-data` first; 4-5 hours, skipped when training/build/lm.pt exists),
# export, and the memorisation check (design §10), which must pass before the copy.
models:
	@if [ ! -f training/build/translit.pt ]; then \
	  cd training && uv run --group train python -m mlkb.data.pairs && \
	  uv run --group train python -m mlkb.translit.train; \
	fi
	cd training && uv run --group train python -m mlkb.translit.export \
	  --checkpoint build/translit.pt --out build/translit.mlkbnn --reference build/translit-reference.json
	mkdir -p $(dir $(TRANSLIT_MODEL))
	cp training/build/translit.mlkbnn $(TRANSLIT_MODEL)
	python3 -I scripts/pack.py packs/ml.neopack --write
	@if [ ! -f training/build/lm.pt ]; then \
	  cd training && uv run --group train python -m mlkb.lm.train --shards build/lm/shards \
	    --vocab build/lm/pieces.tsv --out build/lm.pt; \
	fi
	cd training && uv run --group train python -m mlkb.lm.export --checkpoint build/lm.pt \
	  --vocab build/lm/pieces.tsv --shards build/lm/shards --out build/lm.mlkbnn \
	  --reference build/lm-reference.json
	cd training && uv run --group train python -m mlkb.lm.memorise --model build/lm.mlkbnn \
	  --shards build/lm/shards --out $(CURDIR)/data/eval/memorisation.json
	cp training/build/lm.mlkbnn $(LM_MODEL)
	python3 -I scripts/pack.py packs/ml.neopack --write

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
