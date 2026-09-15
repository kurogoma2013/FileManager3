use std::{fs, path::Path};

const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "csv", "json", "xml", "html", "htm", "yaml", "yml", "toml", "log",
];

pub fn is_text_extension(filename: &str) -> bool {
    Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| TEXT_EXTENSIONS.contains(&value.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn extract_text(path: &Path, filename: &str) -> std::io::Result<Option<String>> {
    if is_text_extension(filename) {
        return fs::read_to_string(path).map(Some);
    }
    if filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
    {
        return pdf_extract::extract_text(path)
            .map(Some)
            .map_err(std::io::Error::other);
    }
    Ok(None)
}

pub fn fts_query(value: &str) -> String {
    value
        .split_whitespace()
        .filter_map(|term| {
            let sanitized: String = term
                .chars()
                .filter(|character| character.is_alphanumeric() || *character == '_')
                .collect();
            (!sanitized.is_empty()).then(|| format!("{sanitized}*"))
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn テキスト拡張子を判定できる() {
        assert!(is_text_extension("notes.md"));
        assert!(is_text_extension("DATA.JSON"));
        assert!(!is_text_extension("photo.png"));
    }

    #[test]
    fn fts検索語を安全に整形する() {
        assert_eq!(fts_query("alpha beta"), "alpha* AND beta*");
        assert_eq!(fts_query("  "), "");
    }
}
