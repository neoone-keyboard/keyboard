//! Every ranking weight in one place. Scores are natural-log probabilities
//! plus bonuses and penalties; higher is better. The golden tests in
//! `tests/golden.rs` pin the behaviour these numbers produce.

/// Added to every English word found in the lexicon. It sets how English
/// candidates weigh against Manglish ones.
pub const KNOWN_BONUS: f32 = 2.0;
/// Score of a typed word in no lexicon. Anything plausible beats it.
pub const UNKNOWN: f32 = -18.0;
/// Penalty per half-edit (`fuzzy::CHEAP_EDIT`) of a correction.
pub const EDIT_PENALTY: f32 = 1.6;
/// Added per half-edit past one full edit (`fuzzy::EDIT`), so that slips pile up: three
/// neighbouring-key slips in a five-letter word (`whsts` → where) rarely beat one (`whsts`
/// → whats). On the eval sets, 1.0–2.5 all helped: chat top-1 0.795 → 0.804, typos
/// 0.832 → 0.856, human 0.568 → 0.575 at 2.0; English wrong rate unchanged.
pub const EXTRA_SLIP_PENALTY: f32 = 2.0;
/// Penalty for a completion, plus a little for every letter it adds.
pub const COMPLETION_PENALTY: f32 = 1.0;
pub const COMPLETION_PER_CHAR: f32 = 0.3;
/// Score of a learned word offered as a completion, before the per-letter penalty.
pub const LEARNED_COMPLETION: f32 = -4.0;
/// A candidate must beat the typed word by this much to take the middle slot.
/// A typed word that is in the English lexicon is never replaced at all.
pub const MARGIN: f32 = 2.0;
/// Nothing scoring below this replaces the typed word, however unknown it is:
/// a rare word is a guess, not a correction.
pub const MIN_REPLACE: f32 = -13.0;

/// Contractions typed without the apostrophe. Leaving it out is the commonest slip and
/// no letter is wrong, so a word that is the typed letters with apostrophes put back
/// (`whats` → what's, `im` → I'm) is the English correction whenever one exists: it
/// beats corrections that change letters (`thats` → that at −6.2 against that's at
/// −8.7; `ive` → ice; `theyre` → there), and is not held back as a likely-unfinished
/// prefix (`PREFIX_VS_CORRECTION`: `im` begins image, important …).
///
/// The bundle has many of these bare forms as words (`whats` −14.6, `cant` −12.0,
/// `im` −9.6), counted from text that dropped the apostrophe, so a typed word in the
/// lexicon is still replaced by its contraction when the contraction scores higher
/// (`im` −9.6 against I'm −8.1: by 1.5, short of `MARGIN`), except for the words in
/// `CONTRACTION_LOOKALIKES`. Measured on the bundle, the gap separates most real
/// words: its −3.4, well −2.8, were −2.6, shell −4.2, hell −3.1, ill −1.4, id −0.4,
/// against whats +5.0, cant +3.3, im +1.5. Not `lets` (+1.0) or `wont` (+3.3):
/// their counts are inflated by stripped let's and won't, so frequency cannot tell
/// them apart; they are listed. A lookalike that stays gets its contraction in the
/// first side slot (`its` · it's).
pub const CONTRACTION_LOOKALIKES: [&str; 11] = [
    "its", "lets", "id", "ill", "well", "were", "wed", "shed", "shell", "hell", "wont",
];

/// Typed words this long get the long-word search (`fuzzy::WeightedEdit`): one vowel for
/// another costs a cheap edit (`definately` → definitely, −13.3 → −11.7, now clear of
/// `MIN_REPLACE`; `prefarance` → preference, −15.6 → −12.4), and a near match may run up
/// to `fuzzy::MAX_TAIL` letters past the typed ones (`apostrap` → apostrophe), a
/// candidate for the middle under `COMPLETION_DOMINANT`. Not where a Malayalam word fits
/// the typed letters: Manglish spells vowels loosely, and with the long-word search there
/// too chat top-1 fell 0.7950 → 0.7919, top-3 0.9161 → 0.9068 and human top-1
/// 0.5665 → 0.5610 (`toorist` and `fitnass` tied with tourist and fitness). Measured on
/// `data/eval/english-typos.tsv`: at 6, `belive` became believed, `adison` Arizona and
/// `bariam` variant, and chat top-3 fell to 0.9130; 7 changed nothing but the wanted rows.
pub const LONG_WORD: usize = 7;
/// A long typed word that is in no lexicon takes the near completion into the middle
/// when that word holds at least this share of the count of every word beginning with
/// the letters it matched (`preferenc` → preference, not preferences), and those letters
/// are no word themselves (`recieve` is receive, not received). It is scored as the word
/// typed with the matched part's edits: the letters after are free, as the typing was
/// right so far. So it competes with corrections and Malayalam as usual, and still needs
/// `MARGIN` and `MIN_REPLACE`.
pub const COMPLETION_DOMINANT: f64 = 0.5;
/// Most edits (half-edits, `fuzzy::CHEAP_EDIT`) in the matched part of a near completion.
/// At the long-word budget (two full edits), `eleedee` became elected and `sameraa` several.
pub const NEAR_COMPLETION_COST: u8 = crate::fuzzy::EDIT;
/// A typed word of at least this many letters that begins some English word is
/// corrected only by a single cheap slip (`fuzzy::CHEAP_EDIT`), not two: `apos`
/// (apostle, apostrophe) stays rather than becoming also (−7.2, a slip and a swap),
/// and where no Malayalam word fits the letters the commonest word they begin takes
/// the first side slot. Filling both side slots with completions, or doing it beside a
/// Malayalam word, cost 11 top-3 rows (`choru`, `evide`, `ok sheri` lost their
/// Malayalam). In this bundle apostle is commoner than apostrophe, so `apos` offers it.
///
/// `MIN_REPLACE` stays one floor for every length. A floor lowered 0.5 per letter
/// past six, for English only, fixed `noticable`, `restaraunt` and `desicion` in the
/// typo set but put English in the middle for ten Manglish words in the human set
/// (`evideya` → evidence, `sibariann` → Siberian, `roonaldoo` → Ronald); 1.0 also made
/// `definatly` defiantly. Lowered for Malayalam as well, it lost typo rows to loanwords.
pub const PREFIX_KEEPS_LETTERS: usize = 4;

/// The English words that start with the typed letters (`ent` → enter,
/// entire, entry …) compete with any replacement as one candidate, scored by
/// their summed log-probability plus a bonus. Space never commits an
/// unfinished word, and a common prefix is more likely unfinished than mistyped.
///
/// Tuned on the real bundle (Task 17). Against an English correction the sum
/// counts as an English word (`KNOWN_BONUS`): `thi` stays (this, think …: −3.6)
/// rather than becoming `the` (−3.75); `wor` and `bec` likewise. `teh` (−9.9)
/// still becomes `the`.
pub const PREFIX_VS_CORRECTION: f32 = KNOWN_BONUS;
/// Against a Manglish word with the typed skeleton, the sum gets less: the
/// skeleton match is itself evidence the word is finished. `ent` stays (−6.6
/// against എന്റെ at −7.3); `ente` (−7.5 against −6.5) and `ende` (−8.3) convert.
pub const PREFIX_VS_MANGLISH: f32 = 0.5;

/// Manglish: when nothing points to English (no English word before it), a
/// Malayalam word the typed letters spell out (up to vowel length) gets this
/// much over an English *correction*: a correction needs a typo, a spelled
/// word needs none. `nale` → നാളെ (−9.7 + 4.5) rather than `name` (−6.9), clear of `AMBIGUOUS`; `teh`
/// spells no Malayalam word and still becomes `the`. Not counted against the
/// English prefix sum above, or `thi` would become തി.
pub const MANGLISH_SPELLED_OVER_CORRECTION: f32 = 4.5;
/// When the best English replacement and the best Malayalam one score within
/// this of each other, neither takes the middle: the typed word stays, and
/// both go in the side slots (`I feel pani` → pani, with pain and പണി). Half
/// of `MARGIN`: on the small golden lexicon `pain` is 15 times as common as
/// പണി and leads by 1.6 after an English word, which must still convert.
pub const AMBIGUOUS: f32 = 1.0;

/// Manglish: bonus when the typed skeleton is the word's whole skeleton.
pub const MANGLISH_EXACT_BONUS: f32 = 1.5;
/// Manglish: penalty per skeleton letter the word has beyond what was typed.
pub const MANGLISH_MISSING_PER_CHAR: f32 = 0.4;
/// Manglish: penalty per edit between the typed letters and the word's romanisation.
pub const MANGLISH_DISTANCE: f32 = 0.8;
/// Manglish suffix splitting (`suffix.rs`): a word read as stem + suffixes
/// scores as its stem, less this per suffix: the joined form is rarer than
/// the stem, and the reading is a guess. `sukhamano` → സുഖം (−9.8) + ആണോ.
pub const MANGLISH_SPLIT_PER_SUFFIX: f32 = 1.0;
/// Stems rarer than this are not split from: a rare stem plus a suffix is
/// more often nonsense than a word.
pub const MANGLISH_SPLIT_STEM_MIN: f32 = -12.5;
/// Manglish with the transliterator (`nn::Translit`, held to the lexicon and the
/// suffix splits): each lexicon spelling it returns scores like a skeleton match
/// (bias, fit bonus), its word's log probability (a split's: its stem's, as in
/// `MANGLISH_SPLIT_PER_SUFFIX`) plus this times the model's score (log
/// probability per generated token, eos included) in place of the
/// `MANGLISH_DISTANCE` penalty: the model, not edit distance, judges how well
/// the typed letters fit. A word the skeleton lookup also found keeps the
/// better of the two scores.
///
/// Swept with the eval harness (Task 10) on chat_golden / translit-human /
/// English no-harm; with the floor at −1.0, chat top-1 was 0.733 at 0.125–0.25,
/// 0.730 at 0.5, 0.724 at 1–2, 0.718 at 4 and 0.714 at 6–8. A small weight
/// wins: lexicon frequency should decide between spellings the model finds
/// plausible (എന്തു over എന്ത്, as in the corpus), and the model's real gain
/// is reach (തൃശ്ശൂർ, ശരി, മനസ്സിലായി) and dropping the distance penalty.
/// Per-token score tuned better than the total log probability (0.724 at best).
///
/// Where English is likely, the model's spellings keep the `MANGLISH_DISTANCE`
/// penalty as well: after an English word (`CONTEXT_AFTER_ENGLISH`), and for a
/// typed word with a capital (a name in English text: `Anjali`, `to Wayanad`).
/// There the model only widens what the skeleton lookup reaches; without this,
/// English wrong-replacements rose from 0.0064 to 0.0075, and `I kazhicho`
/// became കഴിച്ചോ. A sentence start the keyboard capitalised keeps the penalty too.
pub const MANGLISH_MODEL_WEIGHT: f32 = 0.25;
/// Model spellings scoring below this per token are dropped: the constrained
/// beam fills its width with whatever lexicon words it can reach, and the
/// tail is often junk (ജില്ല at −3.4 for `jishnu`). −0.5 cut good words
/// (chat top-1 0.721); −1.25 and below let junk in (0.730).
pub const MANGLISH_MODEL_FLOOR: f32 = -1.0;
/// A lexicon reading this far per token behind the model's best lexicon reading trails it: it
/// is offered, but scored no higher than just under that reading (`MANGLISH_MODEL_TRAILS_STEP`
/// lower for each), however common its word. The constrained beam fills its width with what
/// the lexicon lets it reach, and `MANGLISH_MODEL_WEIGHT` leaves the choice among its readings
/// to frequency: a common word the letters only begin (पाठशाला, −0.91, for `pathshalayen`) or a
/// commoner inflection (देनेवाले, −0.84, for `denewaali`) led the word the model read at −0.1.
/// Readings closer than this are still ranked by frequency (गलगण्ड over गलगंड for `galgand`).
///
/// Measured on the held-out transliteration sets (`mlkb.langs.eval_pack`: the first native
/// suggestion is the target), before → at 0.3: Tamil 44.0% → 47.8%, Assamese 51.5% → 54.5%,
/// Hindi 56.2% → 57.9%; at 0.5: 47.4%, 54.1%, 57.3%; at 0.7: 46.4%, 52.9%, 57.0%. Dropping the
/// trailing readings instead scored the same first suggestion and lost words from the longer
/// bar. Capping what the skeleton lookup found of the same words as well moved nothing.
///
/// Not for Malayalam, whose ranking is held as recorded (`make golden-check`): there 0.3
/// lifts translit-human top-1 0.5745 → 0.6205 and costs chat_golden two rows
/// (0.8043 → 0.7981), a trade for its own sweep.
pub const MANGLISH_MODEL_TRAILS: f32 = 0.3;
pub const MANGLISH_MODEL_TRAILS_STEP: f32 = 0.01;
/// A model spelling scoring at least this per token is a confident reading:
/// it may take the middle with its unfitted score (log probability, bias,
/// model term) up to `MANGLISH_MODEL_CONFIDENT_RELIEF` below `MIN_REPLACE`.
/// `MIN_REPLACE` keeps a rare word from replacing on a guess; a model this
/// sure of a lexicon word is the evidence the floor asks for. Not where the
/// model keeps `MANGLISH_DISTANCE` (after an English word, or a capital), and
/// never for a typed English word, so English stays as typed.
///
/// User-reported on device: `avishyam` and `avshyam` mean ആവിശ്യം, log
/// probability −12.53, so −13.5 at `CONTEXT_AT_START`: just under the floor,
/// though the model reads them at −0.08 and −0.24 per token.
/// Swept with the eval harness on the 315 old chat_golden rows / translit-human
/// (relief 1.5): −0.15 gave 0.768 / 0.534, −0.3 0.775 / 0.557, −0.5
/// 0.781 / 0.567, −0.75 0.775 / 0.572, −1.0 (the floor) 0.775 / 0.570; before,
/// 0.733 / 0.322. English wrong-replacements stayed at 0.0064 throughout. The
/// human set's single words sit at the sentence start, where the floor bit hardest.
///
/// User-reported on device: `nanni` (നന്ദി) put നണ്ണി (−12.45; the model's runner-up at
/// −0.44, after നന്നി at −0.11) in the middle through this relief. Granting it only to
/// the model's first lexicon spelling cost chat top-1 0.782 → 0.773 (ഇപ്പോ, വേണ്ടേ,
/// പുട്ട് lost) and left `bank` in the middle, so the relief stays; `nanni` is a chat
/// form (the pack's `chat-forms.tsv`) instead.
pub const MANGLISH_MODEL_CONFIDENT: f32 = -0.5;
/// At most `MANGLISH_EXACT_BONUS`, so a candidate let in by the relief still
/// clears `MIN_REPLACE` once the fit bonus is added (the final check in
/// `suggest_latin`). 1.0 helped less (chat 0.765, human 0.497 at −0.5).
pub const MANGLISH_MODEL_CONFIDENT_RELIEF: f32 = 1.5;
const _: () = assert!(MANGLISH_MODEL_CONFIDENT_RELIEF <= MANGLISH_EXACT_BONUS);
/// Manglish: a chat spelling from the pack's `chat-forms.tsv` (`nanni` → നന്ദി, `pwoli` → പൊളി)
/// may take the middle this far below `MIN_REPLACE`, like a confident model reading:
/// a hand-checked row is evidence the word is meant. Not where English is likely.
/// പൊളി (−12.9, −13.9 at the start) needs it; നന്ദി and എന്റെ clear the floor anyway.
pub const MANGLISH_CHAT_FORM_RELIEF: f32 = MANGLISH_MODEL_CONFIDENT_RELIEF;
/// A rare spelling with one medial vowel sign more than a word at least this
/// much more probable is a loose spelling of it: loose typing puts a vowel
/// between consonants (`malayalam::vowel_sign_drops`; a final sign is
/// inflection, വന്നോ against വന്ന). The common word is offered
/// (`Engine::add_standard_neighbour`): `avishyam` reads as ആവിശ്യം (−12.53) but
/// means ആവശ്യം (−9.44), which the model never reaches from `avishyam`, and
/// from `avshyam` only past `MODEL_WIDTH`. A big gap only: words a vowel apart
/// are often different words (കിളി, കളി). At 2.0, കപ്പൽ pushed the wanted
/// കാപ്പിൽ out of `kaappil`'s bar. Swapping a sign as well as dropping it, at
/// 1.0, put ലൂസി beside ലിസി and കുട്ടി beside കാട്ടി. At 2.5, medial drops
/// only, no old eval row changes.
pub const MANGLISH_NEIGHBOUR_GAIN: f32 = 2.5;
/// Where the rare form would take the middle, the neighbour takes it instead,
/// scoring this much above the rare form, which keeps a side slot. It inherits
/// the rare form's evidence (score, `spelled`): a far commoner word one vowel
/// away is likelier the intended spelling than the rare one. User follow-up on
/// device: ആവിശ്യം was a typo; `avishyam` and `avshyam` mean ആവശ്യം. With
/// final signs dropped too, promotion cost chat top-1 0.781 → 0.765 (വന്നോ →
/// വന്ന, കണ്ടോ → കണ്ട, മിക്കി → മിക്ക); medial only, nothing changed.
pub const MANGLISH_NEIGHBOUR_PROMOTION: f32 = 0.01;
/// Otherwise (the rare form is only a side slot) the neighbour scores this
/// much below it: side slot only, but ahead of the long completions and junk
/// that fill them.
pub const MANGLISH_NEIGHBOUR_PENALTY: f32 = 1.0;
/// Score of the model's free spelling (off the lexicon), which replaces the
/// rule-based one: side slot only, and only when no Malayalam word was found.
/// It is often a near-duplicate of a lexicon word (കൊല്ലമൊ beside കൊല്ലമോ)
/// and may outscore it, so it never competes for the middle.
pub const MANGLISH_MODEL_FREE: f32 = UNKNOWN;
/// A pack's prior from the previous word (`Engine::priors`): after the pack's own word, after
/// English, at the start. English's prior is 0.
pub const CONTEXT_AFTER_MALAYALAM: f32 = 2.0;
pub const CONTEXT_AFTER_ENGLISH: f32 = -2.0;
pub const CONTEXT_AT_START: f32 = -1.0;

/// LM context window in tokens (design §2): the oldest whole words are dropped past it.
pub const LM_WINDOW: usize = 32;
/// Words before the cursor read for the LM context.
pub const LM_WORDS: usize = 40;
/// A tier-0 correction is clamped to ±this before its weight (design §6). Sweep (tune sets,
/// objective = the mean of next-word top-3 and KS over the three sets, typos-in-context and
/// language-choice top-1; `mlkb.eval.sweep`, at en = ml = 1): 0.4017 at 2, 0.4034 at 3, 0.4041
/// at 4, 0.4042 at 6. A wide clip lets a strong context reach a word the lexicon finds rare.
pub const LM_CLIP: f32 = 6.0;
/// Weight of the tier-0 correction for English candidates. Sweep (objective as `LM_CLIP`):
/// 0.3930 at 0, 0.3975 at 0.25, 0.4017 at 0.5, 0.4018 at 0.75, 0.4030 at 1, all from English
/// keystroke savings (0.3231 → 0.3881); typos-in-context top-1 was best at 0.5 (0.8592 against
/// 0.8451). Kept at 0.5: on the gate files 1 cost language_choice top-1 0.8614 → 0.8317 (with
/// `LM_WEIGHT_ML` at 1; 0.8515, M3a's, with it at 0.5), because after Manglish words the LM's
/// English estimate for a Latin piece also holds Manglish's mass (`p_en` is global), so a
/// whole-weight English correction takes Manglish slots: `innu bhayankara pani` offered
/// `pain` over പണി. The tune split's language-choice rows (0.9000 at every point) can't see it.
pub const LM_WEIGHT_EN: f32 = 0.5;
/// Weight of the tier-0 correction for Malayalam candidates. Sweep (objective as `LM_CLIP`, en
/// = 1): 0.4009 at 0, 0.4021 at 0.25, 0.4030 at 0.5, 0.4038 at 0.75, 0.4041 at 1. Kept at 0.5
/// with `LM_WEIGHT_EN`: at 1, `ഇന്ന് ഭയങ്കര pani` gave പനി over പണി and `ഒരു vada` gave വദ
/// over വട on the gate files (language_choice top-1 0.8317, M3a 0.8515).
pub const LM_WEIGHT_ML: f32 = 0.5;
/// `manglish_bias` from the LM: this × (ln P_ml − ln P_en) + `LANG_OFFSET`, clamped to the
/// range of the old previous-word constants (`CONTEXT_AFTER_ENGLISH`..`CONTEXT_AFTER_MALAYALAM`).
/// Sweep (objective as `LM_CLIP`, at en = ml = 0.5): 0.4010 at 0.5 (3 English wrong
/// replacements, M3a 2: not eligible), 0.4017 at 1, 1.5 and 2; the smallest of those is kept.
pub const LANG_WEIGHT: f32 = 1.0;
/// See `LANG_WEIGHT`. Sweep: 0.3990 at −2 (chat_golden top-1 0.7981, past the tolerance), 0.4017
/// at −1.5, −1 and −0.5, 0.4015 at 0 (3 English wrong replacements); the middle is kept.
pub const LANG_OFFSET: f32 = CONTEXT_AT_START;
/// Next words: word-start pieces read from the LM.
pub const NEXT_PIECES: usize = 64;
/// Next words: the score is ln((1 − this) × P_lm + this × P_bigram), the `next.fst` bigram's
/// probability (a follower's count over the previous word's) mixed in. Sweep (objective as
/// `LM_CLIP`): 0.3990 at 0, 0.4009 at 0.03, 0.4025 at 0.07, 0.4035 at 0.15, 0.4044 at 0.3,
/// mostly Malayalam and Manglish top-3 (0.1059 → 0.1208, 0.1011 → 0.1172). At 0.5 next_ml
/// gate top-3 fell below M3a's (6 hits of 436 against 7): the bigram's followers are few.
pub const NEXT_BIGRAM_WEIGHT: f32 = 0.3;

/// Personal learning (`personal.rs`; the eval runs with an empty store, so none of these
/// moves it). Next words: at most this many of the user's own continuations of the previous
/// words lead the bar, best first (`personal::Followed::score`, then the most recent), and
/// the LM's fill the rest: one slot always shows what the model expects.
///
/// Between two words in a lexicon (or learned), one sighting is enough. User report: after
/// typing "hello how are you" twice the bar still offered "kitty" after "hello". A next word
/// is never put in on its own, so a wrong guess costs a glance; waiting for two sightings
/// would miss most phrases, which are typed a few times at most.
pub const OWN_NEXT_WORDS: usize = 2;
/// After a word in no lexicon (and not learned: often a typo, `thnaks`), a pair must be seen
/// this often first, as a new word must be typed twice to be learned (`user::TIMES_TO_LEARN`).
/// The word offered next must always be known or learned.
pub const OWN_PAIR_AFTER_UNKNOWN: u32 = crate::user::TIMES_TO_LEARN;
/// Score of an own continuation that the typed letters begin (`hello h` → how): ahead of
/// lexicon and learned completions (`LEARNED_COMPLETION`) in the side slots, even after one
/// letter. A completion never takes the middle, and with one this likely the letters are
/// an unfinished word rather than a typo, so it holds back a correction too.
pub const OWN_COMPLETION: f32 = LEARNED_COMPLETION + 1.0;
/// Added to a correction or Malayalam word that is one of the user's continuations of the
/// previous words: a slight lean, half of `MARGIN`. It is taken out again for `MIN_REPLACE`.
pub const OWN_PHRASE_BONUS: f32 = 1.0;
/// Word uses: + this × ln(1 + uses), at most `USE_MAX` (reached at 54 uses), on every
/// candidate and the typed word itself. At most below one half-edit (`EDIT_PENALTY`), so it
/// never outweighs a slip between words equally common, and a typo's clear correction
/// (`teh` → the) stays; taken out for `MIN_REPLACE`, so it never makes a rare guess replace.
/// Used words are words the user committed: an English word kept as typed gains as much as
/// any candidate, so it is no likelier to be replaced. Completions gain more
/// (`USE_COMPLETION_WEIGHT`).
pub const USE_WEIGHT: f32 = 0.25;
pub const USE_MAX: f32 = 1.0;
/// A used word that the typed letters begin, offered as a completion, also gains this ×
/// ln(1 + uses), at most `USE_COMPLETION_MAX` (reached at 12 uses), in any context: a word
/// the user writes is likelier the word they are writing than a commoner one they never do
/// (user report: `appo` kept offering appointed, never appointment, however often it was
/// written). With `USE_WEIGHT` the uses add 1.6 after one use, 2.5 after two, 3.1 after
/// three, 4.0 after five and 5.4 after ten: two or three uses put a word ahead of one five
/// times as common (appointed), and ten ahead of one some 200 times as common. A completion
/// never takes the middle, and this is added after the middle is chosen, so it only reorders
/// the side slots: it never holds back a correction (`teh` → the stays) nor makes one
/// replace. Used words are completed even past the lexicon's precomputed lists
/// (`lexicon::TOP_K`). The eval runs with an empty store, so this never moves it.
pub const USE_COMPLETION_WEIGHT: f32 = 2.0;
pub const USE_COMPLETION_MAX: f32 = 5.0;
/// The language habit (`personal::Personal::habit`): + this × ln((pack + 1) / (English + 1))
/// to each pack's prior, within ±`HABIT_MAX`, once `HABIT_MIN_WORDS` words were counted. A
/// quarter of the previous-word constants' range: context (the LM, the previous word) says
/// more than habit. Never across `CONTEXT_AFTER_ENGLISH` (`engine::with_habit`).
pub const HABIT_WEIGHT: f32 = 0.25;
pub const HABIT_MAX: f32 = 0.5;
pub const HABIT_MIN_WORDS: u32 = 20;
/// With two or more active packs typing in Latin letters, the pack whose prior from the
/// previous word and the habit (not the LM's: it speaks for its own pack only) leads every
/// other's by this much is favoured (`Engine::favoured`): its word keeps the middle against
/// another pack's that scores less than `FAVOURED_MARGIN` more, and the side slots shown with
/// the middle hold English and its words, its best among them; other packs' words move
/// further along the bar. `HABIT_MAX`: a habit that has reached its limit on one side.
/// Lexicon frequencies don't compare across packs (Assamese's smaller corpus puts পানী ahead
/// of പണി for `pani`), so without this the smallest pack's words take the shown slots.
pub const FAVOURED_LEAD: f32 = HABIT_MAX;
/// A previous word's worth (`CONTEXT_AFTER_MALAYALAM`): another pack's word that beats the
/// favoured pack's by this much still takes the middle (`എന്റെ mera` → मेरा).
pub const FAVOURED_MARGIN: f32 = CONTEXT_AFTER_MALAYALAM;

/// Makes a suggestion follow the case of what was typed: `Hel` → `Hello`,
/// `HEL` → `HELLO`. Words stored with capitals (London, I, NASA) keep them.
pub fn apply_case(typed: &str, candidate: &str) -> String {
    if !candidate.is_ascii() {
        return candidate.to_string();
    }
    let letters = typed.chars().filter(char::is_ascii_alphabetic).count();
    if letters >= 2 && typed.chars().all(|c| !c.is_ascii_lowercase()) {
        return candidate.to_ascii_uppercase();
    }
    if typed.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return capitalize(candidate);
    }
    candidate.to_string()
}

pub fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_follows_typing() {
        assert_eq!(apply_case("Hel", "hello"), "Hello");
        assert_eq!(apply_case("hel", "hello"), "hello");
        assert_eq!(apply_case("HEL", "hello"), "HELLO");
        assert_eq!(
            apply_case("H", "hello"),
            "Hello",
            "one capital letter is a capitalised word"
        );
        assert_eq!(apply_case("lon", "London"), "London");
        assert_eq!(apply_case("i", "I"), "I");
        assert_eq!(apply_case("Ent", "എന്റെ"), "എന്റെ");
    }
}
