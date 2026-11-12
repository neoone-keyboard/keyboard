//! Builds a phrase-conversion pack's dictionary and connection matrix (engine 9,
//! `mlengine_core::convert`) from the Mozc OSS dictionary (`src/data/dictionary_oss/` of
//! google/mozc): `dictionary00.txt` to `dictionary09.txt` (reading, left id, right id, cost,
//! surface), `id.def` (the part-of-speech ids) and `connection_single_column.txt` (the ids
//! count, then the cost of every right id followed by every left id, by right id).
//!
//!     lexbuild-ja <mozc dictionary_oss folder> <conversion.dict> <connection.bin>
//!
//! The ids that join the segment before them (for the converter's segments) are those of
//! particles, auxiliaries, suffixes, non-independent verbs and adjectives, and closing marks;
//! the id of a kana no entry covers is the general noun's. Both come from `id.def`'s names.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use mlengine_core::convert::{connection, dict};

/// Whether a word of this part of speech (id.def's comma-separated name) joins the segment
/// before it.
fn joins_before(pos: &str) -> bool {
    let fields: Vec<&str> = pos.split(',').collect();
    let (major, minor) = (fields[0], fields.get(1).copied().unwrap_or("*"));
    matches!(major, "助詞" | "助動詞")
        || minor == "接尾"
        || (matches!(major, "動詞" | "形容詞") && minor == "非自立")
        || (major == "記号" && matches!(minor, "句点" | "読点" | "括弧閉"))
}

fn read_ids(path: &Path) -> std::io::Result<Vec<String>> {
    let mut ids = Vec::new();
    for (i, line) in BufReader::new(fs::File::open(path)?).lines().enumerate() {
        let line = line?;
        let Some((id, pos)) = line.split_once(' ') else {
            continue;
        };
        if id.parse::<usize>().ok() != Some(i) {
            return Err(std::io::Error::other(format!(
                "id.def line {}: id {id}",
                i + 1
            )));
        }
        ids.push(pos.to_string());
    }
    Ok(ids)
}

fn read_rows(dir: &Path) -> std::io::Result<Vec<dict::Row>> {
    let mut rows = Vec::new();
    for n in 0..10 {
        let path = dir.join(format!("dictionary0{n}.txt"));
        for line in BufReader::new(fs::File::open(&path)?).lines() {
            let line = line?;
            // A sixth column marks a spelling correction (アヴァンシア typed as あばんしあ):
            // not a word anyone means, so it is left out.
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() != 5 {
                continue;
            }
            let bad = || std::io::Error::other(format!("{}: {line}", path.display()));
            rows.push(dict::Row {
                reading: f[0].to_string(),
                lid: f[1].parse().map_err(|_| bad())?,
                rid: f[2].parse().map_err(|_| bad())?,
                cost: f[3].parse().map_err(|_| bad())?,
                surface: f[4].to_string(),
            });
        }
    }
    Ok(rows)
}

fn read_matrix(path: &Path) -> std::io::Result<(u16, Vec<i32>)> {
    let text = fs::read_to_string(path)?;
    let mut numbers = text.lines().map(|l| l.trim().parse::<i32>());
    let bad = |what: &str| std::io::Error::other(format!("{}: {what}", path.display()));
    let n = numbers
        .next()
        .ok_or_else(|| bad("empty"))?
        .map_err(|_| bad("size"))?;
    let n = u16::try_from(n).map_err(|_| bad("size"))?;
    let costs: Vec<i32> = numbers
        .collect::<Result<_, _>>()
        .map_err(|_| bad("a cost"))?;
    Ok((n, costs))
}

fn main() -> std::io::Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [mozc, dict_out, conn_out] = args.as_slice() else {
        eprintln!(
            "usage: lexbuild-ja <mozc dictionary_oss folder> <conversion.dict> <connection.bin>"
        );
        std::process::exit(2);
    };
    let ids = read_ids(&mozc.join("id.def"))?;
    let unknown =
        ids.iter()
            .position(|p| p == "名詞,一般,*,*,*,*,*")
            .ok_or_else(|| std::io::Error::other("id.def has no general noun"))? as u16;
    let joins: Vec<u16> = (0..ids.len())
        .filter(|&i| joins_before(&ids[i]))
        .map(|i| i as u16)
        .collect();
    let rows = read_rows(mozc)?;
    let (n, costs) = read_matrix(&mozc.join("connection_single_column.txt"))?;
    if usize::from(n) != ids.len() {
        return Err(std::io::Error::other(format!(
            "{} ids in id.def, the matrix has {n}",
            ids.len()
        )));
    }
    let dict_bytes =
        dict::build(&rows, u32::from(n), unknown, &joins).map_err(std::io::Error::other)?;
    let conn_bytes = connection::build(n, &costs).map_err(std::io::Error::other)?;
    fs::write(dict_out, &dict_bytes)?;
    fs::write(conn_out, &conn_bytes)?;
    let d = dict::Dictionary::from_bytes(dict_bytes).map_err(std::io::Error::other)?;
    eprintln!(
        "{}: {} rows, {} readings, {} ids ({} join the segment before), {:.1} MB",
        dict_out.display(),
        rows.len(),
        d.readings(),
        n,
        joins.len(),
        fs::metadata(dict_out)?.len() as f64 / 1_048_576.0
    );
    eprintln!(
        "{}: {n} × {n}, {:.1} MB",
        conn_out.display(),
        fs::metadata(conn_out)?.len() as f64 / 1_048_576.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::joins_before;

    #[test]
    fn particles_auxiliaries_and_suffixes_join() {
        for pos in [
            "助詞,格助詞,一般,*,*,*,が",
            "助動詞,*,*,*,特殊・マス,基本形,ます",
            "名詞,接尾,人名,*,*,*,*",
            "動詞,非自立,*,*,一段,連用形,いる",
            "記号,句点,*,*,*,*,*",
        ] {
            assert!(joins_before(pos), "{pos}");
        }
        for pos in [
            "名詞,一般,*,*,*,*,*",
            "動詞,自立,*,*,一段,基本形,*",
            "BOS/EOS,*,*,*,*,*,*",
            "名詞,非自立,一般,*,*,*,*",
        ] {
            assert!(!joins_before(pos), "{pos}");
        }
    }
}
