SIM ?= iPhone 17
# Physical iPhone UDID for `make device` (list with: xcrun devicectl list devices)
DEVICE ?= <your-device-id>
PROJECT := apps/ios/MLKeyboard.xcodeproj
CARGO := PATH="$$HOME/.cargo/bin:$$PATH" cargo
XCFRAMEWORK := apps/ios/Packages/MLEngine/MLEngineFFI.xcframework
# Stamped into the keyboard extension for its diagnostics report.
GIT_COMMIT := $(shell git describe --always --dirty 2>/dev/null)
# data/langs/{ml,hi,as}/lang.json are compiled into the engine (engine/core/src/script/mod.rs),
# and packs/ml.neopack/romanisation.json for its fromLatin (engine/core/src/translit.rs).
ENGINE_SOURCES := $(shell find engine -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' | grep -v target) \
  data/langs/ml/lang.json data/langs/hi/lang.json data/langs/as/lang.json packs/ml.neopack/romanisation.json

.PHONY: golden golden-save golden-check golden-more golden-packs golden-save-packs golden-check-packs testflight pack site-packs site-deploy all tools engine project build device twopacks test test-engine coverage-engine test-data test-pack test-training test-swift test-app lexicons emoji lm-data lm models bench bench-hi bench-pack-lm eval eval-swipe spike-assets pack-lexicons pack-models icon

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

# A Release archive (no bundled pack, no spike models) exported and uploaded to App Store
# Connect for TestFlight, signed automatically with the Xcode-logged-in account. Each upload
# needs a build number App Store Connect hasn't seen: `make testflight BUILD=2` (the default is
# CURRENT_PROJECT_VERSION in apps/ios/project.yml; raise that there when releasing a version).
# Without an Apple account in Xcode > Settings > Accounts, pass an App Store Connect API key
# (Users and Access > Integrations; App Manager or Admin role):
#   make testflight ASC_KEY_PATH=~/keys/AuthKey_XXXX.p8 ASC_KEY_ID=XXXX ASC_ISSUER_ID=uuid
BUILD ?=
ASC_KEY_PATH ?=
ASC_KEY_ID ?=
ASC_ISSUER_ID ?=
ASC_AUTH := $(if $(ASC_KEY_PATH),-authenticationKeyPath $(ASC_KEY_PATH) -authenticationKeyID $(ASC_KEY_ID) -authenticationKeyIssuerID $(ASC_ISSUER_ID))
ARCHIVE := build/NeoOne.xcarchive
testflight: tools project $(XCFRAMEWORK)
	rm -rf $(ARCHIVE) build/export
	xcodebuild -project $(PROJECT) -scheme KeyboardApp -configuration Release \
	  -destination 'generic/platform=iOS' -archivePath $(ARCHIVE) -allowProvisioningUpdates $(ASC_AUTH) \
	  ML_GIT_COMMIT=$(GIT_COMMIT) $(if $(BUILD),CURRENT_PROJECT_VERSION=$(BUILD)) archive -quiet
	xcodebuild -exportArchive -archivePath $(ARCHIVE) -exportOptionsPlist apps/ios/ExportOptions.plist \
	  -exportPath build/export -allowProvisioningUpdates $(ASC_AUTH)

# Debug-only (M6 phase 2, task 12): the device app with packs/hi.neopack and as.neopack beside
# Malayalam and active ["ml","hi"] (English off), built into its own DerivedData so the normal
# build is untouched. Builds only; install it by hand:
#   xcrun devicectl device install app --device $(DEVICE) build/DerivedData-twopacks/Build/Products/Debug-iphoneos/KeyboardApp.app
twopacks: tools project $(XCFRAMEWORK)
	xcodebuild -project $(PROJECT) -scheme KeyboardApp -configuration Debug \
	  -destination 'id=$(DEVICE)' -derivedDataPath build/DerivedData-twopacks -allowProvisioningUpdates \
	  ML_GIT_COMMIT=$(GIT_COMMIT) ML_DEBUG_PACKS="hi as" ML_ACTIVE_LANGUAGES=ml,hi build -quiet

test: test-engine test-data test-training test-swift eval golden-check-packs

test-engine:
	cd engine && $(CARGO) test && $(CARGO) clippy --all-targets -- -D warnings
	./scripts/shipped-crates.sh --check

# Line coverage of the engine's test suite (the `cargo test` of test-engine, so the release-only
# and ignored tests of test-data, eval and bench don't count): a table of every source file, the
# least covered first, and the total. Measures only; nothing fails on a number. Needs
# cargo-llvm-cov, once:
#   cd engine && rustup component add llvm-tools-preview && cargo install cargo-llvm-cov
# The tests run several times slower instrumented (about 10 minutes, far longer on a busy
# machine), which can fail a test that times itself (swipe's a_swipe_is_quick): a failing test
# is reported in the output and the table is still printed (--ignore-run-fail). test-engine
# is the gate, not this.
coverage-engine:
	@cd engine && $(CARGO) llvm-cov --version >/dev/null 2>&1 || { echo "cargo-llvm-cov not found. Install it with: cd engine && rustup component add llvm-tools-preview && cargo install cargo-llvm-cov"; exit 1; }
	mkdir -p build/coverage
	cd engine && $(CARGO) llvm-cov --workspace --ignore-run-fail --json --summary-only --output-path ../build/coverage/engine.json
	@python3 -I scripts/coverage-table.py build/coverage/engine.json engine

# The data checks: every pack's checksums, script rules and layout (scripts/pack.py --check),
# the scripts' own tests, each pack's lexicons against its tables and script
# (engine/core/tests/pack_check.rs reads every manifest in packs/), the older per-pack tests,
# the Malayalam, Hindi and Assamese script rules on every word of their lexicons against what
# they gave as code (script_rules_recorded.rs), and the data pipeline. Last, that the published site lists every pack in packs/: that one fails
# for a pack nobody has published yet (`make site-packs` does), so it runs after the rest.
# A contributor's target is test-pack, below.
PACK_CHECKS := python3 -I -m unittest discover -s scripts/tests && \
	(cd engine && $(CARGO) test -q --release -p mlengine-core --test romanisation_tables -- --ignored) && \
	(cd engine && $(CARGO) test -q --release -p mlengine-core --test script_normal_form -- --ignored) && \
	(cd engine && $(CARGO) test -q --release -p mlengine-core --test script_rules_recorded -- --ignored every_lexicon_word) && \
	(cd engine && $(CARGO) test -q --release -p lexbuild --test english_next -- --ignored) && \
	uv run --quiet --project data/pipeline pytest -q data/pipeline
test-data:
	for pack in packs/*.neopack engine/core/tests/fixtures/gu.neopack; do python3 -I scripts/pack.py $$pack --check || exit 1; done
	cd engine && NEOPACK= $(CARGO) test -q --release -p mlengine-core --test pack_check -- --ignored --nocapture
	$(PACK_CHECKS)
	python3 -I scripts/publish-packs.py --check

# Every check of one pack, for whoever made it (docs/make-a-language.md, "Contribute your
# pack"): its checksums, script rules and layout; that the engine opens it and loads every file
# its manifest lists; its lexicon words in normal form and its romanised keys against its
# tables (engine/core/tests/pack_check.rs, for the pack named); then the checks test-data runs
# for every pack; and, once the pack's suggestion bars are recorded (golden-save-packs, below),
# that they are still the recorded ones. It leaves out the site index, which only lists
# published packs.
#   make test-pack PACK_LANG=ta
test-pack:
	python3 -I scripts/pack.py packs/$(PACK_LANG).neopack --check
	cd engine && NEOPACK=$(PACK_LANG) $(CARGO) test -q --release -p mlengine-core --test pack_check -- --ignored --nocapture
	$(PACK_CHECKS)
	@if [ -f data/golden/packs/$(PACK_LANG).tsv.gz ]; then $(MAKE) --no-print-directory golden-check-packs GOLDEN_PACKS=$(PACK_LANG); fi
	@echo "test-pack: packs/$(PACK_LANG).neopack passes"

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

# Swipe accuracy on synthetic paths, checked against data/eval/baselines-swipe.json: English
# and Malayalam (swipe_eval), then each pack of SWIPE_PACKS (swipe_eval_packs; hi, as and ta
# unless set) over its held-out words. A pack with no baseline there is printed, not gated.
#   make eval-swipe SWIPE_PACKS=xx
eval-swipe:
	cd engine && cargo test -p mlengine-core --release --test swipe_eval swipe_eval -- --ignored --nocapture --test-threads=1

# Every suggestion over the eval inputs (engine/core/tests/golden_dump.rs), for refactors that
# must not change output: with the default languages (data/golden/m6p1.tsv.gz) and with English
# switched off (data/golden/ml-only.tsv.gz). golden-save records both; golden-check fails on any
# difference from either record.
GOLDEN := data/golden/m6p1.tsv.gz
GOLDEN_ML_ONLY := data/golden/ml-only.tsv.gz
golden:
	cd engine && $(CARGO) test -q --release -p mlengine-core --test golden_dump -- --ignored golden_dump
# The same inputs asked for 3, 10 and 20 suggestions: the longer lists only add to the end
# (about 3.5 minutes).
golden-more:
	cd engine && $(CARGO) test -q --release -p mlengine-core --test golden_dump -- --ignored more_suggestions --nocapture
golden-save: golden
	mkdir -p data/golden && gzip -9nc build/golden/now.tsv > $(GOLDEN)
	gzip -9nc build/golden/ml-only.tsv > $(GOLDEN_ML_ONLY)
golden-check: golden
	@gzip -dc $(GOLDEN) | diff - build/golden/now.tsv > build/golden/diff.txt \
	  && echo "golden: identical" \
	  || { echo "golden: $$(grep -c '^>' build/golden/diff.txt) lines differ (build/golden/diff.txt)"; head -20 build/golden/diff.txt; exit 1; }
	@gzip -dc $(GOLDEN_ML_ONLY) | diff - build/golden/ml-only.tsv > build/golden/diff-ml-only.txt \
	  && echo "golden ml-only: identical" \
	  || { echo "golden ml-only: $$(grep -c '^>' build/golden/diff-ml-only.txt) lines differ (build/golden/diff-ml-only.txt)"; head -20 build/golden/diff-ml-only.txt; exit 1; }

# The same for the other packs (engine/core/tests/golden_pack.rs): the bar for every prefix of
# inputs from the pack's own eval data (data/eval/<id>/), on an engine with English and that
# pack, recorded in data/golden/packs/<id>.tsv.gz. GOLDEN_PACKS names the packs: by default
# every pack with a recording. golden-save-packs records them (again after a pack's files
# change on purpose: new lexicons, a new model), golden-check-packs fails on any difference
# and prints "golden <id>: identical" for each pack that has none.
#   make golden-save-packs GOLDEN_PACKS=ta
GOLDEN_PACKS_DIR := data/golden/packs
GOLDEN_PACKS ?= $(sort $(patsubst $(GOLDEN_PACKS_DIR)/%.tsv.gz,%,$(wildcard $(GOLDEN_PACKS_DIR)/*.tsv.gz)))
golden-packs:
	@test -n "$(GOLDEN_PACKS)" || { echo "no pack to record: make golden-save-packs GOLDEN_PACKS=<id>"; exit 1; }
	cd engine && GOLDEN_PACKS="$(GOLDEN_PACKS)" $(CARGO) test -q --release -p mlengine-core --test golden_pack -- --ignored golden_dump_packs
golden-save-packs: golden-packs
	mkdir -p $(GOLDEN_PACKS_DIR)
	for id in $(GOLDEN_PACKS); do gzip -9nc build/golden/packs/$$id.tsv > $(GOLDEN_PACKS_DIR)/$$id.tsv.gz || exit 1; done
golden-check-packs: golden-packs
	@status=0; for id in $(GOLDEN_PACKS); do \
	  if [ ! -f $(GOLDEN_PACKS_DIR)/$$id.tsv.gz ]; then \
	    echo "golden $$id: not recorded (make golden-save-packs GOLDEN_PACKS=$$id)"; status=1; \
	  elif gzip -dc $(GOLDEN_PACKS_DIR)/$$id.tsv.gz | diff - build/golden/packs/$$id.tsv > build/golden/packs/diff-$$id.txt; then \
	    echo "golden $$id: identical"; \
	  else \
	    echo "golden $$id: $$(grep -c '^>' build/golden/packs/diff-$$id.txt) lines differ (build/golden/packs/diff-$$id.txt)"; \
	    head -20 build/golden/packs/diff-$$id.txt; status=1; \
	  fi; done; exit $$status

# The Malayalam pack as a .neopack file (a zip of packs/ml.neopack), for import or download.
pack:
	python3 -I scripts/pack.py packs/ml.neopack --zip build/packs/ml.neopack

# The download site (site/, a Cloudflare Worker with static assets; hosting/wrangler.jsonc) and its
# pack index, from site-src/ (site.json holds the domain, the R2 bucket and the download URL) and
# packs/: zips every pack into build/site-packs/<id>-<version>.neopack, writes site/packs/index.json
# (with the older versions site-src/kept-packs.json keeps listed for apps on older engines),
# the pages and apps/ios/PackIndex.xcconfig (the app's index URL), and prints the upload commands.
# site-deploy uploads the pack files the site doesn't serve yet to R2 and runs wrangler deploy
# (wrangler logged in to the account that holds the jishnu.me zone).
site-packs:
	python3 -I scripts/publish-packs.py

site-deploy:
	python3 -I scripts/publish-packs.py --deploy

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
# data/eval/$(PACK_LANG)/translit-human.tsv, the copy into the pack, and the same held-out words
# typed into the engine with and without the model (mlkb.eval.pack). The pack's LM, where
# training/mlkb/lm/lang.py has one (hi), is `make lm-data lm LM_LANG=hi`.
#   make pack-lexicons pack-models PACK_LANG=as
# TRANSLIT_STEPS=6000 trains a short run (the schedule's length; warmup and evaluation scale with
# it), for a first try; TRANSLIT_ARGS passes anything else to mlkb.translit.train.
#   make pack-models PACK_LANG=mr TRANSLIT_STEPS=6000
PACK_LANG ?= hi
TRANSLIT_STEPS ?=
TRANSLIT_ARGS ?=
TRANSLIT_STEP_ARGS := $(if $(TRANSLIT_STEPS),--steps $(TRANSLIT_STEPS) --warmup $(shell expr $(TRANSLIT_STEPS) / 12) --eval-every $(shell expr $(TRANSLIT_STEPS) / 6))
PACK_DIR := packs/$(PACK_LANG).neopack
PACK_BUILD := build/$(PACK_LANG)
PACK_KIND := $(if $(wildcard training/mlkb/langs/$(PACK_LANG).json),data,langs)
pack-lexicons:
	./data/build-pack-lexicons.sh $(PACK_LANG)

pack-models:
	@if [ ! -f training/$(PACK_BUILD)/translit.pt ]; then \
	  cd training && uv run --group train python -m mlkb.$(PACK_KIND).pairs --lang $(PACK_LANG) --out $(PACK_BUILD)/pairs && \
	  uv run --group train python -m mlkb.translit.train --lang $(PACK_LANG) --pairs $(PACK_BUILD)/pairs \
	    --out $(PACK_BUILD)/translit.pt $(TRANSLIT_STEP_ARGS) $(TRANSLIT_ARGS); \
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
	cd training && uv run --quiet python -m mlkb.eval.pack --lang $(PACK_LANG)

# Rebuilds the committed emoji files (the panel and the suggestions) from Unicode's
# emoji-test.txt and the CLDR annotations in data/manifests (downloads about 13 MB): English,
# and every pack language that has its words in data/langs/<id>/. A language's first build
# names it by pack id (its CLDR locale is in data/manifests/cldr-annotations.json, "packs"):
#   make emoji EMOJI_PACKS="as ta"
emoji:
	./data/build-emoji.sh $(EMOJI_PACKS)

# Which language the LM pipeline pairs with English (training/mlkb/lm/lang.py): ml, the default,
# builds in training/build/lm and build/lm.pt as it always has; hi in training/build/lm-hi and
# build/lm-hi.pt. (Not LANG, which is the locale.)  make lm-data lm LM_LANG=hi
# as and ta (packs of data/langs/, text from data/build-<id>-text.sh) train on what their
# manifests credit: of English, English Wikipedia only, and no Dakshina. Their eval sets are
# `python -m mlkb.eval.next_sets --lang <id>` (ta before lm-data, as after it) and the record
# without an LM, `python -m mlkb.eval --lang <id> --translit … --record-nolm`.
# `make bench-pack-lm LM_LANG=<id>` is their latency gate.
LM_LANG ?= ml
LM_BUILD := build/$(if $(filter ml,$(LM_LANG)),lm,lm-$(LM_LANG))
LM_EVAL := data/eval$(if $(filter ml,$(LM_LANG)),,/$(LM_LANG))

# The LM's training text, vocabulary and token shards in training/$(LM_BUILD) (design §5; needs
# data/fetch.sh and the lexicon text in data/raw/text, about 2 hours: ml.txt from
# data/build-lexicons.sh, hi.txt from data/build-pack-lexicons.sh hi). Also writes the native
# next-word eval sets (next-ml.tune.txt and next-ml.heldout.txt; data/eval/hi/next-hi.*).
lm-data:
	cd training && uv run --group train python -m mlkb.lm.sources --lang $(LM_LANG) && \
	  uv run --group train python -m mlkb.lm.corpus --lang $(LM_LANG) --out $(LM_BUILD)/text && \
	  uv run --group train python -m mlkb.lm.tokenizer --lang $(LM_LANG) --text $(LM_BUILD)/text --out $(LM_BUILD) && \
	  uv run --group train python -m mlkb.lm.shards --text $(LM_BUILD)/text --vocab $(LM_BUILD)/pieces.tsv \
	    --out $(LM_BUILD)/shards

# A pack's LM other than Malayalam's (`make models` builds that one): training (`make lm-data
# LM_LANG=hi` first; 4-5 hours, skipped when training/$(LM_BUILD).pt exists), export, the
# memorisation check (design §10) into $(LM_EVAL)/memorisation.json, and the eval's LM ship gates
# against the pack without an LM ($(LM_EVAL)/baselines-nolm.json, `python -m mlkb.eval --lang hi
# --translit … --record-nolm`). Each must pass before the copy into packs/$(LM_LANG).neopack.
lm:
	@if [ "$(LM_LANG)" = ml ]; then echo "Malayalam's LM is built by make models"; exit 1; fi
	@if [ ! -f training/$(LM_BUILD).pt ]; then \
	  cd training && uv run --group train python -m mlkb.lm.train --lang $(LM_LANG); \
	fi
	cd training && uv run --group train python -m mlkb.lm.export --lang $(LM_LANG) \
	  --reference $(LM_BUILD)-reference.json
	cd training && uv run --group train python -m mlkb.lm.memorise --lang $(LM_LANG) \
	  --out $(CURDIR)/$(LM_EVAL)/memorisation.json
	cd training && uv run --quiet python -m mlkb.eval --lang $(LM_LANG) --quiet --gate lm \
	  --translit $(CURDIR)/packs/$(LM_LANG).neopack/translit.mlkbnn --lm $(CURDIR)/training/$(LM_BUILD).mlkbnn
	cp training/$(LM_BUILD).mlkbnn packs/$(LM_LANG).neopack/lm.mlkbnn
	python3 -I scripts/pack.py packs/$(LM_LANG).neopack --write

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
# and the sliding context cache must predict within 2% of fresh windows. The suggest gate also
# runs with every table of the learned-word store full, and with 2,000 of the user's own words
# (native words picked from the bar, `own::OwnWords`); bench-hi with 2,000 of them too.
bench:
	cd engine && $(CARGO) test --release -p mlengine-core --test bundle -- --include-ignored --nocapture latency
	cd engine && $(CARGO) test --release -p mlengine-core --test nn_reference -- --include-ignored --nocapture nn_latency
	cd engine && $(CARGO) test --release -p mlengine-core --test lm_reference -- --include-ignored --nocapture --test-threads=1 lm_latency ring_matches

# The same for the Hindi pack and its LM (engine/core/tests/pack_lm.rs: the bundled
# packs/hi.neopack/lm.mlkbnn, else training/build/lm-hi.mlkbnn): suggest p99 under 8 ms without
# and with the LM, the LM commit p99 under 1 ms and the context ring within 2% of fresh windows.
bench-hi:
	cd engine && $(CARGO) test --release -p mlengine-core --test pack_lm -- --include-ignored --nocapture
	cd engine && MLENGINE_LM_LANG=hi $(CARGO) test --release -p mlengine-core --test lm_reference -- --include-ignored --nocapture --test-threads=1 lm_latency ring_matches

# The same for any pack with an LM (engine/core/tests/pack_lm_any.rs: the pack's lm.mlkbnn, else
# training/build/lm-$(LM_LANG).mlkbnn), typing words of the pack's held-out set: suggest p99
# under 8 ms without and with the LM, the LM commit p99 under 1 ms and the context ring within
# 2% of fresh windows.  make bench-pack-lm LM_LANG=as
bench-pack-lm:
	cd engine && NEOPACK_LM=$(LM_LANG) $(CARGO) test --release -p mlengine-core --test pack_lm_any -- --include-ignored --nocapture --test-threads=1
	cd engine && MLENGINE_LM_LANG=$(LM_LANG) $(CARGO) test --release -p mlengine-core --test lm_reference -- --include-ignored --nocapture --test-threads=1 lm_latency ring_matches

test-swift: $(XCFRAMEWORK)
	swift test --package-path apps/ios/Packages/KeyboardCore
	swift test --package-path apps/ios/Packages/MLEngine

# The app's unit tests (apps/ios/KeyboardAppTests), run inside the app on the simulator: the
# Languages screen's import with real pack files, and the keyboard's suggestion bar as a
# view. Signed to run locally, so the simulator gives the app its App Group; never a device.
test-app: tools project $(XCFRAMEWORK)
	xcodebuild -project $(PROJECT) -scheme KeyboardApp -destination 'platform=iOS Simulator,name=$(SIM)' \
	  ML_GIT_COMMIT=$(GIT_COMMIT) test -only-testing:KeyboardAppTests -quiet

# The app icon and its in-app copy (apps/ios/KeyboardApp/Assets.xcassets), from
# tools/icon/render.swift. Downloads the Space Grotesk font into build/icon/ to draw with.
icon:
	./tools/icon/render.sh

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
