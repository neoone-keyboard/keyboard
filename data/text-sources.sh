# Sourced by data/build-<lang>-text.sh (and data/build-pack-lexicons.sh): how many words each
# source gave a language's text, so a source that swamps the others shows before the lexicon is
# built from it. The counts live beside the text, in data/raw/text/<lang>.sources.tsv
# (source<TAB>words), because the text is cached and the sources' extracts are not kept.
#
#   reset_sources <lang>                 before extracting: forget the old counts
#   count_source <lang> <source> <file>  after extracting one source: record its words
#   source_words <lang>                  one line: every source's words and share
#   report_sources <lang>                that line, and a warning when one source is most of the text
#
# Needs $TEXT (data/raw/text).

# A source with more than this share of the words gets a warning.
SOURCE_SHARE_WARN=60

reset_sources() {
  rm -f "$TEXT/$1.sources.tsv"
}

count_source() {
  printf '%s\t%s\n' "$2" "$(wc -w < "$3" | tr -d ' ')" >> "$TEXT/$1.sources.tsv"
}

source_words() {
  local counts="$TEXT/$1.sources.tsv"
  if [ ! -s "$counts" ]; then
    echo "$1 text by source: not recorded (the text was extracted before counts were kept; delete $TEXT/$1.txt to re-extract)"
    return
  fi
  awk -F'\t' -v lang="$1" '
    { name[NR] = $1; words[NR] = $2; total += $2 }
    END {
      line = lang " text by source:"
      for (i = 1; i <= NR; i++)
        line = line sprintf("%s %s %d words (%.0f%%)", (i > 1 ? "," : ""), name[i], words[i], total ? 100 * words[i] / total : 0)
      print line
    }' "$counts"
}

report_sources() {
  source_words "$1"
  local counts="$TEXT/$1.sources.tsv"
  [ -s "$counts" ] || return 0
  awk -F'\t' -v warn="$SOURCE_SHARE_WARN" '
    { name[NR] = $1; words[NR] = $2; total += $2 }
    END {
      for (i = 1; i <= NR; i++)
        if (NR > 1 && total && 100 * words[i] / total > warn)
          printf "warning: %s is %.0f%% of the words, so its style will lead the lexicon and the next-word pairs. If it is not how people write today (old literature, scanned books), keep only part of it: docs/make-a-language.md, step 3.\n", name[i], 100 * words[i] / total
    }' "$counts" >&2
}
