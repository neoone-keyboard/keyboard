"""Per-language training config: `langs/<id>.json`.

Each file names the pack's script block (the transliterator's target vocabulary), its lexicon,
its romaniser for synthetic spellings (`"code"` for Malayalam's hand-written one, else a spelling
table such as `langs/hi-spelling.json`), its
Dakshina and Aksharantar data, and where its eval sets live. `ml` is the default everywhere, so
the Malayalam commands run as before.
"""

import json
from dataclasses import dataclass
from functools import cache
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DEFAULT = "ml"


@dataclass(frozen=True)
class Lang:
    id: str
    name: str
    script: str
    block: tuple[int, int]
    pack: Path
    lexicon: Path
    romaniser: str
    dakshina: str
    aksharantar_dir: Path
    aksharantar_prefix: str
    eval_dir: Path
    reference: tuple[tuple[str, str], ...]

    @property
    def romanisation(self) -> Path | None:
        """The spelling table (`table_romanise` format), or None for a romaniser written as code."""
        return None if self.romaniser == "code" else ROOT / self.romaniser

    @property
    def dakshina_lexicons(self) -> Path:
        return ROOT / f"data/raw/dakshina/dakshina_dataset_v1.0/{self.dakshina}/lexicons"

    def in_block(self, c: str) -> bool:
        return self.block[0] <= ord(c) <= self.block[1]


@cache
def load(lang_id: str = DEFAULT) -> Lang:
    """The config in `langs/<lang_id>.json`; FileNotFoundError for an unknown language."""
    raw = json.loads((Path(__file__).with_name(f"{lang_id}.json")).read_text(encoding="utf-8"))
    pack = ROOT / raw["pack"]
    return Lang(
        id=raw["id"],
        name=raw["name"],
        script=raw["script"],
        block=(int(raw["block"][0], 16), int(raw["block"][1], 16)),
        pack=pack,
        lexicon=pack / raw["lexicon"],
        romaniser=raw["romaniser"],
        dakshina=raw["dakshina"],
        aksharantar_dir=ROOT / raw["aksharantar"]["dir"],
        aksharantar_prefix=raw["aksharantar"]["prefix"],
        eval_dir=ROOT / raw["eval"],
        reference=tuple(tuple(r) for r in raw["reference"]),
    )


def ids() -> list[str]:
    """Every configured language; other files here (`hi-spelling.json`) have a hyphen."""
    return sorted(p.stem for p in Path(__file__).parent.glob("*.json") if "-" not in p.stem)
