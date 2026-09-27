use crate::error::Result;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub size_bytes: i64,
    /// Filesystem modification time (RFC 3339), when the platform reports one.
    pub modified_at: Option<String>,
    pub line_count: Option<i64>,
}

pub fn is_binary_file(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 8192];
    let bytes_read = file.read(&mut buffer)?;

    // Check for null bytes in initial sample
    Ok(buffer[..bytes_read].contains(&0))
}

pub fn count_lines(path: &Path) -> Result<Option<i64>> {
    if is_binary_file(path)? {
        return Ok(None);
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut lines = 0i64;
    let mut buffer = String::new();

    loop {
        buffer.clear();
        let bytes_read = match reader.read_line(&mut buffer) {
            Ok(b) => b,
            Err(_) => return Ok(None), // Not valid UTF-8, treat as binary
        };
        if bytes_read == 0 {
            break;
        }
        lines += 1;
    }

    Ok(Some(lines))
}

pub fn inspect_file_metadata(path: &Path) -> Result<FileMetadata> {
    let fs_meta = std::fs::metadata(path)?;
    let size_bytes = fs_meta.len() as i64;
    let modified_at = fs_meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    let line_count = count_lines(path).unwrap_or(None);

    Ok(FileMetadata {
        size_bytes,
        modified_at,
        line_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_text_and_binary_line_count() {
        let mut text_file = NamedTempFile::new().unwrap();
        writeln!(text_file, "line 1").unwrap();
        writeln!(text_file, "line 2").unwrap();
        writeln!(text_file, "line 3").unwrap();
        assert_eq!(count_lines(text_file.path()).unwrap(), Some(3));

        let mut bin_file = NamedTempFile::new().unwrap();
        bin_file
            .write_all(&[0x89, b'P', b'N', b'G', 0x00, 0x01, 0x02])
            .unwrap();
        assert_eq!(count_lines(bin_file.path()).unwrap(), None);
    }
}
