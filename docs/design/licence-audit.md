# Licence audit: Malayalam and English sources

The sources considered for the Malayalam pack and the English base. Read on 2026-11-03.

The facts this audit relies on: the app is free on the App Store, we train every model from scratch, the trained weights ship in the app bundle, and raw corpora never ship. This audit records what each source's own licence text says and how we read it. It is not legal advice. The maintainer decided (2026-11-03) not to get legal review for a free app; instead we follow every condition each licence sets, on its strictest reading.

Verdicts:
- **use** means the terms fit training a model and shipping its weights, with attribution on the About screen.
- **eval only** means we may keep a held-out copy on the Mac to measure models. We never train on it, and it never ships.
- **reject** means don't use it at all.

Every source marked **use** or **eval only** has a manifest in `data/manifests/`. A manifest has a checksum only where this audit downloaded the file (three files, all under 50 MB). The others say `"status": "audited, not fetched"`, list their URLs under `candidate_files`, and keep `"files": []` so that `data/fetch.sh` skips them. To fetch one later, move the entry into `files` and add the checksum.

## 1. Results

| Source | Licence (as stated by the source) | URL read | Verdict | Reason |
|---|---|---|---|---|
| Dakshina v1.0 (Google), `ml` lexicons and romanised sentences | CC BY-SA 4.0 | [README, License section](https://github.com/google-research-datasets/dakshina/blob/master/README.md#license) (there is no LICENSE file) | **use** | Human romanisations, which is what we need to fit the rule probabilities. The licence is share-alike, so it falls under the same position as Wikipedia (§3). Train on the train split and hold dev and test out for eval. |
| Aksharantar (AI4Bharat), `mal` | Manual data CC BY, version not stated. Mined data and "existing sources" CC0 1.0. | [HF dataset card](https://huggingface.co/datasets/ai4bharat/Aksharantar/blob/main/README.md), [paper arXiv:2205.03018](https://arxiv.org/abs/2205.03018) §1 and §3, [dataset_info.csv](https://github.com/AI4Bharat/IndicXlit/blob/master/corpus_preprocessing/Collating_existing_dataset/dataset_info.csv) | **use (filtered)** | Keep the rows whose `source` is IndicCorp, Samanantar, Wikidata or AK-NEI/AK-NEF/AK-Uni/AK-Freq. Drop the `Dakshina Dataset` and `Existing sources` rows (see below). Hold the test split (manual, CC BY) out for eval. |
| IndicCorp v1 (AI4Bharat), `ml`, `en` | CC BY-NC-SA 4.0 | [indicnlp_corpus README, License](https://github.com/AI4Bharat/indicnlp_corpus#license) | **reject** | NonCommercial plus share-alike, on news text the publishers own. v2 replaces it. |
| IndicCorp v2 (AI4Bharat), `ml`, `en` | "will be released under a CC-0 license" | [HF card](https://huggingface.co/datasets/ai4bharat/IndicCorpV2), [IndicBERT README](https://github.com/AI4Bharat/IndicBERT#license), [paper arXiv:2212.05409](https://arxiv.org/abs/2212.05409) Ethics Statement | **eval only** | The text is crawled news articles. AI4Bharat can waive only its own rights in the compilation, not the publishers' copyright, so the text can't go into weights we ship. A held-out sample is still useful to measure the LM on Malayalam that isn't from Wikipedia. |
| SMC corpus `text/ml-wiki` | CC BY-SA 3.0 (repo README) | [smc/corpus README](https://gitlab.com/smc/corpus/-/blob/master/README.md) | **reject** | It is an older copy of Malayalam Wikipedia, which we already take straight from the dumps. |
| SMC corpus `text/sayahna` | SMC says CC BY-SA 4.0. Sayahna's own page says "The text of the original item is copyrighted to the author". Only "the text encoding and editorial notes" are CC BY-SA 4.0. | [SMC README](https://gitlab.com/smc/corpus/-/blob/master/text/sayahna/README.md), [books.sayahna.org](https://books.sayahna.org/) | **reject** | The licence doesn't cover the literary text. |
| SMC corpus `text/deshabhimani` (news, 2023-01 to 2024-04, 114 MB) | SMC says CC BY 4.0. Deshabhimani's terms say "Creative Commons Attribution Share Alike 4.0 India License [CC-BY-SA 4.0 IN] or later" for content "unless otherwise specified". Its footer says "All Rights Reserved". | [SMC README](https://gitlab.com/smc/corpus/-/blob/master/text/deshabhimani/README.md), [deshabhimani.com/terms-of-use](https://www.deshabhimani.com/terms-of-use) (read from a 2024 Wayback snapshot because the live site serves a Cloudflare challenge) | **eval only** | The site grants CC BY-SA, but the footer contradicts it, and news agency copy is probably "otherwise specified". Upgrade to **use** only after confirming the terms on the live site or with the publisher. |
| SMC corpus `text/CMOKerala` | README: "content is in public domain" | [SMC README](https://gitlab.com/smc/corpus/-/blob/master/text/CMOKerala/README.md) | **reject** | Nothing backs the public-domain claim. Indian government works carry government copyright (Copyright Act 1957, s.17(dd) and s.28), and the Chief Minister's Office publishes no licence. |
| SMC corpus `text/sarvavijnanakosam` | README: GNU FDL | [SMC README](https://gitlab.com/smc/corpus/-/blob/master/text/sarvavijnanakosam/README.md) | **reject** | The repo holds only the README, not the text. The source site didn't respond, so the GFDL claim is unverified. GFDL's copy-the-licence and invariant-section terms also fit model weights badly. |
| SMC corpus `text/oscar` | README only, pointing to OSCAR (Common Crawl) | [SMC README](https://gitlab.com/smc/corpus/-/blob/master/text/oscar/README.md) | **reject** | Web crawl, with no licence on the text itself. |
| SMC Malayalam Speech Corpus, `text/conversations.txt` (203 sentences) | CC BY-SA 4.0 (LICENSE file; the README says 3.0) | [smc/msc LICENSE at f959908](https://gitlab.com/smc/msc/-/blob/f95990846eff9910cff7e7f2ac45e320836324e3/LICENSE) | **eval only** | Everyday spoken Malayalam written for the project, and the only openly licensed conversational Malayalam we found. It's too small to help training and is worth more held out. |
| Project Gutenberg, English | Public domain in the US. The PG licence covers only the trademark: "If you strip the Project Gutenberg license and all references to Project Gutenberg from the text, you are left with a text unrestricted by U.S. intellectual property law." | [gutenberg.org/policy/license](https://www.gutenberg.org/policy/license.html), [permission](https://www.gutenberg.org/policy/permission.html), [robot access](https://www.gutenberg.org/policy/robot_access.html) | **use** | PG vouches only for US status ("Not all items that are public domain in the US are public domain in other countries"). Keep books whose authors all died before 1956, which puts them in the public domain under life+70 and India's life+60. Skip books whose header marks them as copyrighted, and strip the PG header and footer. The register is old, so give it a low mix weight. |
| English Wikinews | CC BY 4.0 after 2024-12-16, CC BY 2.5 from 2005-09-25, public domain before that. "This work may be attributed to 'Wikinews'." | [Wikinews:Copyright](https://en.wikinews.org/wiki/Wikinews:Copyright) | **use** | Attribution only, no share-alike. Modern prose. Wikinews forbids importing copyrighted text and Wikipedia text. Fetched: 47.5 MB, SHA-1 matches the dump's sha1sums. |
| Simple English Wikipedia | CC BY-SA 4.0 (site `rightsinfo`; the local Copyrights page still talks about the GFDL) | [siteinfo API](https://simple.wikipedia.org/w/api.php?action=query&meta=siteinfo&siprop=rightsinfo), [Wikimedia Terms of Use](https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use) | **use** | Same terms and position as English Wikipedia, with plainer sentences that are closer to what people type. |
| OpenWebText | CC0 for "the actual packaging". "We do not own any of the text from which these data has been extracted." | [HF card](https://huggingface.co/datasets/Skylion007/openwebtext) | **reject** | The text is web pages linked from Reddit, all of it third-party copyright. |
| Code-mixed Manglish: Chakravarthi et al. 2020, Malayalam-English sentiment (6,739 YouTube comments) | CC BY 4.0 on the Zenodo record | [zenodo.org/records/4015234](https://zenodo.org/records/4015234). The [GitHub repo](https://github.com/bharathichezhiyan/DravidianCodeMix-Dataset) has no licence. | **eval only** | Real romanised chat, but the curators don't own the commenters' text, so their CC BY can't license it. Fetched (0.5 MB, MD5 matches Zenodo). |
| Code-mixed: DravidianCodeMix 2020 (Zenodo 4750858) and other Manglish sets on GitHub/Zenodo | CC BY 4.0 on Zenodo, or no licence | [zenodo.org/records/4750858](https://zenodo.org/records/4750858) | **reject** for training | Same problem: YouTube comments the curators don't own. **No permissively licensed Manglish chat corpus exists that we could train on.** Use synthetic pairs for training. |
| Wikipedia and Wikisource (enwiki, mlwiki, mlwikisource), already used | CC BY-SA 4.0 | Existing manifests, [Terms of Use §7](https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use) | **use**, with the conditions in §3 | See §3. |

### Notes on Aksharantar's sub-sources

The card gives one licence per group: "Manually collected data: Released under CC-BY license. Mined dataset (from Samanantar and IndicCorp): Released under CC0 license. Existing sources: Released under CC0 license." Sub-sources with other terms:

- **Dakshina** is CC BY-SA 4.0 at Google. AI4Bharat can't relicense it as CC0. For Malayalam, 58,382 of the 61,583 existing-source training pairs come from Dakshina, according to `dataset_info.csv`. Take them from Google's release instead, under Dakshina's own licence.
- **BrahmiNet** contributes 3,201 Malayalam pairs. The paper says "Brahminet is mined from ILCI parallel corpus (Jha, 2010)". We didn't find a licence for BrahmiNet, and ILCI is distributed for research through TDIL. Drop these rows.
- **Wikidata** labels are CC0. Fine.
- **Samanantar and IndicCorp mined pairs**: the card says "We do not own any of the text from which this data has been extracted". The rows are single words. A word on its own isn't protectable expression, so we accept them. The residual risk is low. IndicCorp-mined pairs are English loanwords and their Malayalam spellings (`computer` to its Malayalam form), not chat romanisations.
- **Manual data** (the Karya annotators: AK-NEI, AK-NEF, AK-Uni, AK-Freq) is CC BY. The version isn't stated. A commented-out line in the card points to 4.0. Attribute it as CC BY.
- The card contradicts itself: it lists manual data as CC-BY, yet the CC0 statement waives rights to "manually collected data and existing sources". We use the stricter reading, CC BY.

The `source` column values have to be checked when `mal.zip` (131 MB) is fetched. This audit didn't download it.

## 2. Overlap and leakage

- Aksharantar was filtered against the Dakshina test and dev sets (IndicXlit `ablation_study/data_filteration*`). Dedupe again by native word when building splits, so that no Dakshina dev/test word appears in training.
- Dakshina's `native_script_wikipedia` text duplicates mlwiki. Don't use it.

## 3. CC BY-SA and shipping trained weights on the App Store

The question: we train on CC BY-SA text (Wikipedia, Wikisource, Dakshina, SMC words, Simple English Wikipedia) and ship the weights inside an App Store app. Does share-alike apply to the weights? Does the ban on technological measures conflict with App Store distribution? The lexicons (word frequencies and next-word pairs from Wikipedia) already raise the same question, so the answer covers what ships today too.

Not legal advice. Each argument below is a reading of the cited text, and no court has ruled on CC BY-SA and model weights.

### What the licence says

From the [CC BY-SA 4.0 legal code](https://creativecommons.org/licenses/by-sa/4.0/legalcode.en):

- §1(a): "Adapted Material means material subject to Copyright and Similar Rights that is derived from or based upon the Licensed Material and in which the Licensed Material is translated, altered, arranged, transformed, or otherwise modified in a manner requiring permission under the Copyright and Similar Rights held by the Licensor."
- §2(a)(5)(C), no downstream restrictions: "You may not offer or impose any additional or different terms or conditions on, or apply any Effective Technological Measures to, the Licensed Material if doing so restricts exercise of the Licensed Rights by any recipient of the Licensed Material."
- §3(b)(3), ShareAlike: "You may not offer or impose any additional or different terms or conditions on, or apply any Effective Technological Measures to, Adapted Material that restrict exercise of the rights granted under the Adapter's License You apply."

From the [CC FAQ](https://creativecommons.org/faq/), "Can I use effective technological measures (such as DRM) when I share CC-licensed material?": "No. ... For example, if you remix a CC-licensed song, and you wish to share it on a music site that places digital copy-restriction on all uploaded files, you may not do this without express permission from the licensor. However, if you upload that same file to your own site or any other site that does not apply DRM to the file, and a listener chooses to stream it through an app that applies DRM, you have not violated the license."

From CC's guidance [Using CC-Licensed Works for AI Training](https://creativecommons.org/using-cc-licensed-works-for-ai-training-2/):
- "CC licenses apply only when copyright permission is required. If exceptions or limitations apply, then the CC license terms don't apply."
- On the conservative path: "If AI models or outputs are based on ShareAlike content and they will be shared publicly, following the ShareAlike condition would require AI developers to use the same CC license as the original works."
- On attribution: "For AI model training, attribution could be a simple link to the source of the dataset used to train the model."

From [Apple's Standard EULA](https://www.apple.com/legal/internet-services/itunes/dev/stdeula/), §a: "You may not transfer, redistribute or sublicense the Licensed Application ... You may not copy ..., reverse-engineer, ... modify, or create derivative works of the Licensed Application, ... or any part thereof (except as and only to the extent that any foregoing restriction is prohibited by applicable law or to the extent as may be permitted by the licensing terms governing use of any open-sourced components included with the Licensed Application)."

### The case that there is a problem

1. **The weights may be Adapted Material.** If the weights are "derived from or based upon" the text in a way that needs permission, §3(b) applies. The weights would then have to be licensed CC BY-SA, with no extra terms and no technological measures. CC's own conservative guidance reads it this way.
2. **The App Store adds terms and DRM.** Apple's EULA forbids redistributing, copying and modifying "any part" of the app, and App Store builds carry FairPlay encryption. Both look like what §2(a)(5)(C) and §3(b)(3) prohibit. The CC FAQ's music-site example describes a store that adds copy restriction, which is our situation.
3. **There is precedent with copyleft.** In January 2011 Apple pulled VLC for iOS after one of its GPL copyright holders complained that the App Store's terms conflicted with the GPL ([Engadget, 2011-01-08](https://www.engadget.com/2011-01-08-vlc-app-removed-from-app-store.html)). GPL isn't CC BY-SA, but the conflict between a copyleft licence and App Store terms has the same shape, and one rights holder's complaint was enough.
4. **Memorisation.** A language model can reproduce training text word for word. CC notes that whether memorisation happened can only be shown from outputs. If the LM can emit long Wikipedia passages, it is much harder to argue the weights don't contain the work.
5. **Jurisdiction.** India has no text-and-data-mining exception. ANI v OpenAI in the Delhi High Court was pending when this was written; check its current status. We can't count on an exception covering training everywhere we distribute.

### The case that there isn't

1. **Training may need no permission.** CC says its conditions apply only where copyright requires permission. In the US, two 2025 district court rulings found training to be fair use on their records: Bartz v. Anthropic (N.D. Cal., June 2025, as to lawfully acquired books) and Kadrey v. Meta (N.D. Cal., June 2025). The EU's DSM Directive Art. 3 and 4 permit text and data mining, Art. 4 subject to a rights holder's machine-readable opt-out. These rulings are first-instance decisions and are summarised from memory. Check them before relying on them.
2. **Weights may not be Adapted Material.** The definition requires the Licensed Material to be "modified in a manner requiring permission". The transliterator learns character mappings from word pairs. Word lists and frequencies are facts, not expression. A ~10M parameter LM trained on hundreds of millions of words can't store the corpus, and if it doesn't reproduce the text, arguably nothing protectable is in the weights.
3. **Apple's EULA defers to open licences, in part.** Its no-copying, no-modification clause yields to "the licensing terms governing use of any open-sourced components". The ban on redistribution has no such carve-out. A custom EULA can add one.
4. **The weights can be made available without DRM.** If the same files are published elsewhere under CC BY-SA 4.0, every recipient can exercise the licensed rights whatever the App Store does, which is the situation of the second half of the FAQ example. As we understand it, FairPlay encrypts the executable, not resource files such as model weights. Verify that on a built `.ipa`. CC BY-SA 4.0 has no explicit parallel-distribution exception, so this reduces the risk without removing it.
5. **Industry practice.** Many widely used models trained on Wikipedia ship under non-share-alike licences. Google's BERT, for example, is Apache 2.0. No rights holder is known to have challenged them. That is practice, not law.

### Recommended position

Keep training on CC BY-SA sources. Behave as if the weights and lexicons derived from them were Adapted Material, because doing so costs little and covers the strict reading:

1. **Attribution.** The About screen names each source, its licence and a link (lines in §4), and says that the models and word lists were built from that text and modified from it. This meets the BY condition and CC's suggested form for training data.
2. **Share-alike, without DRM.** Publish every shipped file derived from a CC BY-SA source (`*.fst` lexicons, transliterator and LM weights) under CC BY-SA 4.0 at a public URL with no DRM, for example a GitHub release per app version, and link it from the About screen. CC BY-SA 3.0 inputs (SMC words) may be adapted under 4.0, since 3.0 §4(b) allows "a later version of this License with the same License Elements", and Deshabhimani's terms say "4.0 ... or later".
3. **Ship weights as separate resource files**, not compiled into the executable.
4. **Custom EULA.** In App Store Connect, use a custom EULA, or an addendum to Apple's, stating that the data files listed on the About screen are licensed under CC BY-SA 4.0 and that nothing in the EULA restricts them.
5. **Memorisation check before the LM ships.** Add an eval that prompts the LM with Wikipedia prefixes and fails if it reproduces long verbatim spans, say 50 or more tokens. The exact threshold is for the eval task to set.
6. **Fallback.** If publishing the weights is unacceptable, train the shipped models only on CC BY, CC0 and public-domain sources (Wikinews, Gutenberg, filtered Aksharantar, synthetic pairs). That works for English and the transliterator, but Malayalam would lose Wikipedia, which is most of its LM text.
7. **No legal review** (the maintainer's decision, 2026-11-03). Points 1–5 are the strict reading of every licence, and all of them are done before the first App Store submission.

Publishing the weights is a product decision as well as a legal one: anyone could then reuse our models under CC BY-SA. It is the one point here that needs the maintainer's decision.

## 4. About screen attribution lines

These cover only sources marked **use**. Eval-only sources never ship and need no credit in the app.

> This keyboard's word lists and prediction models were built from, and modified from, the following text. The built files are available under CC BY-SA 4.0 at <release URL>.
>
> - Wikipedia (English, Simple English and Malayalam) and Malayalam Wikisource, by their contributors. CC BY-SA 4.0. https://creativecommons.org/licenses/by-sa/4.0/ (wikipedia.org, ml.wikisource.org)
> - English Wikinews, by Wikinews contributors. CC BY 2.5 and CC BY 4.0. https://en.wikinews.org/
> - Dakshina dataset, Google Research (Roark et al., 2020). CC BY-SA 4.0. https://github.com/google-research-datasets/dakshina
> - Aksharantar, AI4Bharat (Madhani et al., 2022). CC BY and CC0. Includes Wikidata labels (CC0). https://huggingface.co/datasets/ai4bharat/Aksharantar
> - Malayalam word lists from the Malayalam Corpus by Swathanthra Malayalam Computing. CC BY-SA 3.0. https://gitlab.com/smc/corpus
> - Public-domain texts from Project Gutenberg. https://www.gutenberg.org/
> - SCOWL word lists, Copyright 2000-2018 by Kevin Atkinson. (Full notice as in `data/manifests/scowl.json`.)

Project Gutenberg's permission page says "No permission is needed to credit Project Gutenberg as the source of something you use. This applies even for commercial use." The PG trademark licence doesn't apply once the headers are stripped.

## 5. Open items

- Confirm the Deshabhimani terms on the live site, or with the publisher, before any upgrade from eval only.
- ~~Check Aksharantar's `source` values in `mal.zip` at fetch time and apply the filter in §1.~~ Done 2026-11-03: `mal_train.json` has a per-row `source` (IndicCorp, Samanantar, Wikidata, Dakshina, Existing; no AK-* rows, which are only in the test split). The filter is applied in `training/mlkb/data/human.py`; counts are in `data/manifests/aksharantar.json`.
- Verify that FairPlay leaves resource files unencrypted on a built `.ipa`.
- Check the current status of the cases cited in §3 and of any Indian TDM legislation.
- ~~Decide whether to publish the derived files (§3, point 2).~~ Decided by the maintainer 2026-11-03: yes. The app is free, and publishing the lexicons and model weights under CC BY-SA 4.0 is acceptable. Still to do before the first App Store release: the public release URL and the custom EULA (§3, point 4). No legal review: we follow each licence's conditions on the strict reading instead.
