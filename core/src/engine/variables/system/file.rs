use directories::UserDirs;

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024; // 5MB limit

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileInvocation {
    pub variant: String,
    pub raw_args: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileParseError {
    MissingVariant,
    MissingParentheses,
    UnbalancedParentheses,
    InvalidTrailingSyntax,
}

pub(crate) fn parse_invocation(key: &str) -> Result<FileInvocation, FileParseError> {
    let rest = key
        .strip_prefix("file.")
        .ok_or(FileParseError::MissingVariant)?;

    let variant_end = rest.find('(').unwrap_or(rest.len());
    let variant = rest[..variant_end].trim();
    if variant.is_empty() {
        return Err(FileParseError::MissingVariant);
    }

    let (raw_args, trailing) = if variant_end == rest.len() {
        if rest.contains(')') {
            return Err(FileParseError::UnbalancedParentheses);
        }
        (String::new(), "")
    } else {
        scan_parenthesized(&rest[variant_end..])?
    };

    if !trailing.trim().is_empty() {
        return Err(FileParseError::InvalidTrailingSyntax);
    }

    Ok(FileInvocation {
        variant: variant.to_string(),
        raw_args,
    })
}

fn scan_parenthesized(input: &str) -> Result<(String, &str), FileParseError> {
    if !input.starts_with('(') {
        return Err(FileParseError::MissingParentheses);
    }

    let mut depth = 0usize;
    let mut start = None;

    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => {
                if depth == 0 {
                    start = Some(idx + ch.len_utf8());
                }
                depth += 1;
            }
            ')' => {
                if depth == 0 {
                    return Err(FileParseError::UnbalancedParentheses);
                }
                depth -= 1;
                if depth == 0 {
                    let start = start.ok_or(FileParseError::MissingParentheses)?;
                    return Ok((input[start..idx].trim().to_string(), &input[idx + 1..]));
                }
            }
            _ => {}
        }
    }

    Err(FileParseError::UnbalancedParentheses)
}

pub(crate) fn expand_path(path_str: &str) -> Option<PathBuf> {
    if let Some(rest) = path_str.strip_prefix("~/") {
        UserDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .map(|mut p| {
                p.push(rest);
                p
            })
    } else if let Some(rest) = path_str.strip_prefix("~\\") {
        UserDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .map(|mut p| {
                p.push(rest);
                p
            })
    } else if path_str == "~" {
        UserDirs::new().map(|d| d.home_dir().to_path_buf())
    } else {
        Some(PathBuf::from(path_str))
    }
}

fn check_file(path: &Path) -> Result<File, String> {
    let file = File::open(path).map_err(|e| format!("[Error: {}]", e))?;
    let metadata = file.metadata().map_err(|e| format!("[Error: {}]", e))?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err("[Error: File exceeds 5MB limit]".to_string());
    }
    Ok(file)
}

fn read_file(path_str: &str) -> String {
    let path = match expand_path(path_str) {
        Some(p) => p,
        None => return "[Error: Invalid path]".to_string(),
    };

    let mut file = match check_file(&path) {
        Ok(f) => f,
        Err(e) => return e,
    };

    let mut contents = String::new();
    if let Err(e) = file.read_to_string(&mut contents) {
        return format!("[Error: {}]", e);
    }
    contents
}

fn read_lines(path_str: &str, start: usize, end: usize) -> String {
    let path = match expand_path(path_str) {
        Some(p) => p,
        None => return "[Error: Invalid path]".to_string(),
    };

    let file = match check_file(&path) {
        Ok(f) => f,
        Err(e) => return e,
    };

    let reader = BufReader::new(file);
    let mut result = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        let line_num = i + 1;
        if let Ok(l) = line
            && line_num >= start
            && line_num <= end
        {
            result.push(l);
        }
        if line_num >= end {
            break;
        }
    }

    if result.is_empty() {
        return "[Error: Lines out of bounds]".to_string();
    }

    result.join("\n")
}

pub fn resolve(key: &str) -> Option<String> {
    let invocation = parse_invocation(key).ok()?;
    let variant = invocation.variant.as_str();

    match variant {
        "read" => {
            if invocation.raw_args.is_empty() {
                return Some("[Error: Missing path]".to_string());
            }
            Some(read_file(&invocation.raw_args))
        }
        "read_line" => {
            if invocation.raw_args.is_empty() {
                return Some("[Error: Missing path]".to_string());
            }

            // Format: path, start, [end]
            let parts: Vec<&str> = invocation.raw_args.rsplitn(3, ',').collect();

            if parts.len() < 2 {
                return Some("[Error: read_line needs path and start line]".to_string());
            }

            let path_str: &str;
            let start_str: &str;
            let mut end_str: Option<&str> = None;

            if parts.len() == 3 {
                end_str = Some(parts[0].trim());
                start_str = parts[1].trim();
                path_str = parts[2].trim();
            } else {
                start_str = parts[0].trim();
                path_str = parts[1].trim();
            }

            let start = match start_str.parse::<usize>() {
                Ok(n) if n > 0 => n,
                _ => return Some("[Error: invalid start line]".to_string()),
            };

            let end = if let Some(e) = end_str {
                match e.parse::<usize>() {
                    Ok(n) if n >= start => n,
                    _ => return Some("[Error: invalid end line]".to_string()),
                }
            } else {
                start
            };

            Some(read_lines(path_str, start, end))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        file
    }

    #[test]
    fn parses_invocations() {
        assert_eq!(
            parse_invocation("file.read(/path/to/file.txt)").unwrap(),
            FileInvocation {
                variant: "read".to_string(),
                raw_args: "/path/to/file.txt".to_string(),
            }
        );
        assert_eq!(
            parse_invocation("file.read_line(/path/with, comma.txt, 1, 5)").unwrap(),
            FileInvocation {
                variant: "read_line".to_string(),
                raw_args: "/path/with, comma.txt, 1, 5".to_string(),
            }
        );
    }

    #[test]
    fn read_file_success() {
        let file = create_temp_file("hello world");
        let path = file.path().to_str().unwrap();
        assert_eq!(read_file(path), "hello world");
    }

    #[test]
    fn read_file_missing() {
        let result = read_file("/path/does/not/exist.txt");
        assert!(result.starts_with("[Error: "));
    }

    #[test]
    fn read_line_single() {
        let file = create_temp_file("one\ntwo\nthree\nfour");
        let path = file.path().to_str().unwrap();
        assert_eq!(read_lines(path, 2, 2), "two");
    }

    #[test]
    fn read_line_range() {
        let file = create_temp_file("one\ntwo\nthree\nfour");
        let path = file.path().to_str().unwrap();
        assert_eq!(read_lines(path, 2, 3), "two\nthree");
    }

    #[test]
    fn read_line_out_of_bounds() {
        let file = create_temp_file("one\ntwo");
        let path = file.path().to_str().unwrap();
        assert_eq!(read_lines(path, 5, 6), "[Error: Lines out of bounds]");
    }

    #[test]
    fn resolve_read_line_args() {
        let file = create_temp_file("one\ntwo\nthree");
        let path = file.path().to_str().unwrap();

        let key_single = format!("file.read_line({}, 2)", path);
        assert_eq!(resolve(&key_single).unwrap(), "two");

        let key_range = format!("file.read_line({}, 1, 2)", path);
        assert_eq!(resolve(&key_range).unwrap(), "one\ntwo");
    }

    #[test]
    fn file_size_limit() {
        let file = NamedTempFile::new().unwrap();
        file.as_file().set_len(MAX_FILE_SIZE + 1).unwrap();
        let path = file.path().to_str().unwrap();
        let result = read_file(path);
        assert_eq!(result, "[Error: File exceeds 5MB limit]");
    }

    #[test]
    fn expand_tilde() {
        let path = expand_path("~/test.txt").unwrap();
        let home = directories::UserDirs::new()
            .unwrap()
            .home_dir()
            .to_path_buf();
        assert!(path.starts_with(&home));
        assert!(path.ends_with("test.txt"));
    }
}
