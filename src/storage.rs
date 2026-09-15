use std::path::Path;

pub fn safe_filename(filename: &str) -> Result<&str, &'static str> {
    let path = Path::new(filename);
    if filename.trim().is_empty()
        || path.is_absolute()
        || path.components().count() != 1
        || path.file_name().and_then(|value| value.to_str()) != Some(filename)
        || filename == "."
        || filename == ".."
    {
        return Err("invalid filename");
    }
    Ok(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 危険なファイル名を拒否する() {
        assert!(safe_filename("report.pdf").is_ok());
        assert!(safe_filename("../secret.txt").is_err());
        assert!(safe_filename("/tmp/secret.txt").is_err());
    }
}
