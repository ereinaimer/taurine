use directories::UserDirs;

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024; // 5MB limit

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

fn check_file(path: &Path) -> Option<File> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Failed to open file '{}': {}", path.display(), e);
            return None;
        }
    };
    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Failed to read metadata for '{}': {}", path.display(), e);
            return None;
        }
    };
    if metadata.len() > MAX_FILE_SIZE {
        tracing::warn!("File '{}' exceeds 5MB limit", path.display());
        return None;
    }
    Some(file)
}

fn read_file(path_str: &str) -> Option<String> {
    let path = match expand_path(path_str) {
        Some(p) => p,
        None => {
            tracing::warn!("Invalid file path: '{}'", path_str);
            return None;
        }
    };

    let mut file = check_file(&path)?;

    let mut contents = String::new();
    if let Err(e) = file.read_to_string(&mut contents) {
        tracing::warn!("Failed to read file '{}': {}", path.display(), e);
        return None;
    }
    Some(contents)
}

fn read_lines(path_str: &str, start: usize, end: usize) -> Option<String> {
    let path = match expand_path(path_str) {
        Some(p) => p,
        None => {
            tracing::warn!("Invalid file path: '{}'", path_str);
            return None;
        }
    };

    let file = check_file(&path)?;

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
        tracing::warn!(
            "Lines {}-{} out of bounds for '{}'",
            start,
            end,
            path.display()
        );
        return None;
    }

    Some(result.join("\n"))
}

/// Resolves the unified `file(...)` system variable.
///
/// `raw` is the argument list inside `file(...)`, bound as
/// `(op, path, start, end)` with a quote-aware split; an unquoted
/// comma inside the path shifts arity and fails.
pub fn resolve(raw: &str) -> Option<String> {
    let spec = crate::engine::variables::registry::param_spec("file")?;
    let bound = crate::engine::variables::parser::bind_call("file", raw, &spec).ok()?;
    let has_named = raw.contains('=');
    let (op, path_str, start_str, end_str) = if has_named {
        (
            bound.named.get("op").cloned().unwrap_or_default(),
            bound.named.get("path").cloned().unwrap_or_default(),
            bound.named.get("start").cloned().unwrap_or_default(),
            bound.named.get("end").cloned().unwrap_or_default(),
        )
    } else {
        if bound.positional.len() > spec.params.len() {
            return None;
        }
        let op = bound.positional.first().cloned().unwrap_or_default();
        let rest: Vec<String> = bound.positional.iter().skip(1).cloned().collect();
        let (path, start, end) = match (op.as_str(), rest.len()) {
            ("read", 1) => (rest[0].clone(), String::new(), String::new()),
            ("line", 2) => (rest[0].clone(), rest[1].clone(), String::new()),
            ("lines", 2) => (rest[0].clone(), rest[1].clone(), String::new()),
            ("lines", 3) => (rest[0].clone(), rest[1].clone(), rest[2].clone()),
            _ => return None,
        };
        (op, path, start, end)
    };

    match op.as_str() {
        "read" => {
            if path_str.trim().is_empty() || !start_str.is_empty() || !end_str.is_empty() {
                tracing::warn!("file.read called with missing path");
                return None;
            }
            read_file(path_str.trim())
        }
        "line" => {
            if path_str.trim().is_empty() || start_str.trim().is_empty() || !end_str.is_empty() {
                tracing::warn!("file.line needs path and line number");
                return None;
            }
            let line_num = match start_str.trim().parse::<usize>() {
                Ok(n) if n > 0 => n,
                _ => {
                    tracing::warn!("file.line invalid line number: '{}'", start_str);
                    return None;
                }
            };
            read_lines(path_str.trim(), line_num, line_num)
        }
        "lines" => {
            if path_str.trim().is_empty() || start_str.trim().is_empty() {
                tracing::warn!("file.lines needs path and start line");
                return None;
            }
            let start = match start_str.trim().parse::<usize>() {
                Ok(n) if n > 0 => n,
                _ => {
                    tracing::warn!("file.lines invalid start line: '{}'", start_str);
                    return None;
                }
            };
            let end = if end_str.trim().is_empty() {
                usize::MAX
            } else {
                match end_str.trim().parse::<usize>() {
                    Ok(n) if n >= start => n,
                    _ => {
                        tracing::warn!("file.lines invalid end line: '{}'", end_str);
                        return None;
                    }
                }
            };
            read_lines(path_str.trim(), start, end)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(content: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.txt");
        std::fs::write(&path, content).unwrap();
        let s = path.to_str().unwrap().to_string();
        (dir, s)
    }

    #[test]
    fn file_unified() {
        let (_dir, path) = write_temp("one\ntwo\nthree\nfour");
        assert_eq!(
            resolve(&format!("read, {path}")).unwrap(),
            "one\ntwo\nthree\nfour"
        );
        assert_eq!(
            resolve(&format!("op=read, path={path}")).unwrap(),
            "one\ntwo\nthree\nfour"
        );
        assert_eq!(resolve(&format!("line, {path}, 2")).unwrap(), "two");
        assert_eq!(
            resolve(&format!("lines, {path}, 1, 2")).unwrap(),
            "one\ntwo"
        );
        assert_eq!(resolve("bogus, /tmp/x"), None);
        assert_eq!(resolve(&format!("read, {path}, extra, args, oops")), None);
    }

    #[test]
    fn file_comma_path_quoted_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a,b.txt");
        std::fs::write(&path, "comma-ok").unwrap();
        let raw_path = path.to_str().unwrap();
        assert_eq!(
            resolve(&format!("read, \"{raw_path}\"")).unwrap(),
            "comma-ok"
        );
        assert_eq!(resolve(&format!("read, {raw_path}")), None); // unquoted comma → arity error
    }

    #[test]
    fn read_file_missing() {
        let result = read_file("/path/does/not/exist.txt");
        assert_eq!(result, None);
    }

    #[test]
    fn read_line_single() {
        let (_dir, path) = write_temp("one\ntwo\nthree\nfour");
        assert_eq!(read_lines(&path, 2, 2), Some("two".to_string()));
    }

    #[test]
    fn read_line_range() {
        let (_dir, path) = write_temp("one\ntwo\nthree\nfour");
        assert_eq!(read_lines(&path, 2, 3), Some("two\nthree".to_string()));
    }

    #[test]
    fn read_line_out_of_bounds() {
        let (_dir, path) = write_temp("one\ntwo");
        assert_eq!(read_lines(&path, 5, 6), None);
    }

    #[test]
    fn file_size_limit() {
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(MAX_FILE_SIZE + 1).unwrap();
        let path = file.path().to_str().unwrap();
        let result = read_file(path);
        assert_eq!(result, None);
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
