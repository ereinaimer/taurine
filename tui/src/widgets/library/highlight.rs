//! Center-pane syntax highlighting: script grammars via syntect
//! (pure-Rust `fancy-regex`, no native deps) mapped onto the Taurine
//! [`Theme`] palette, plus a Taurine template overlay (`[vars]`,
//! `| transformers`, directives) that wins inside `[...]` regions.
//! Runs are char-indexed so they survive the word-wrap in `detail.rs`.

use std::ops::Range;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use syntect::highlighting::ScopeSelectors;
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use taurine_core::engine::ScriptInterpreter;
use taurine_core::engine::variables::system::transformers;
use taurine_core::engine::variables::{parse_system_call, tags};

use crate::theme::Theme;

/// One styled run tiling a source line; `start..end` in char units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) style: Style,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Comment,
    String,
    Number,
    Keyword,
    Function,
    Type,
    Invalid,
}

/// Pre-parsed Sublime scope selectors, most specific first. Parsed once;
/// the [`Theme`] colors resolve per render.
static SELECTORS: OnceLock<Vec<(ScopeSelectors, Kind)>> = OnceLock::new();

fn selectors() -> &'static [(ScopeSelectors, Kind)] {
    SELECTORS.get_or_init(|| {
        [
            ("invalid", Kind::Invalid),
            ("comment", Kind::Comment),
            ("string", Kind::String),
            ("constant.numeric", Kind::Number),
            ("entity.name.function, support.function", Kind::Function),
            (
                "entity.name.type, entity.name.class, support.type, support.class",
                Kind::Type,
            ),
            ("keyword, storage", Kind::Keyword),
        ]
        .into_iter()
        .filter_map(|(source, kind)| ScopeSelectors::from_str(source).ok().map(|sel| (sel, kind)))
        .collect()
    })
}

fn kind_style(kind: Kind, theme: &Theme) -> Style {
    match kind {
        Kind::Comment => Style::default().fg(theme.text_muted),
        Kind::String => Style::default().fg(theme.success),
        Kind::Number => Style::default().fg(theme.warning),
        Kind::Keyword => Style::default().fg(theme.primary),
        Kind::Function => Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
        Kind::Type => Style::default().fg(theme.accent),
        Kind::Invalid => Style::default().fg(theme.error),
    }
}

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();

/// Bat-curated Sublime grammar dump (fancy-regex compatible build).
/// honey: ~1MB static, loaded once; per-frame cost is parsing only.
fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

/// Sublime grammar extension per Taurine script interpreter.
fn syntax_for(set: &SyntaxSet, interpreter: ScriptInterpreter) -> Option<&SyntaxReference> {
    let extension = match interpreter {
        ScriptInterpreter::Bash => "sh",
        ScriptInterpreter::PowerShell => "ps1",
        ScriptInterpreter::Python => "py",
        ScriptInterpreter::Node => "js",
        ScriptInterpreter::Cmd => "bat",
    };
    set.find_syntax_by_extension(extension)
}

/// Highlight source lines: script grammar base plus the Taurine overlay
/// inside `[...]` regions. Text content (`None`) gets the overlay only.
///
/// Results cache on content hash: renders repeat every frame while edits
/// invalidate via the hash. honey: single entry, fine while one pane
/// paints at a time; shard by key if callers ever overlap.
pub(crate) fn highlight_lines(
    lines: &[String],
    interpreter: Option<ScriptInterpreter>,
    theme: &Theme,
) -> Vec<Vec<Run>> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
    struct Cached {
        hash: u64,
        interpreter: Option<ScriptInterpreter>,
        theme: &'static str,
        runs: Vec<Vec<Run>>,
    }

    let mut hasher = DefaultHasher::new();
    lines.len().hash(&mut hasher);
    for line in lines {
        line.hash(&mut hasher);
    }
    let hash = hasher.finish();
    if let Ok(cache) = CACHE.lock()
        && let Some(hit) = cache.as_ref()
        && hit.hash == hash
        && hit.interpreter == interpreter
        && hit.theme == theme.name
    {
        return hit.runs.clone();
    }
    let set = syntax_set();
    let syntax = interpreter.and_then(|interpreter| syntax_for(set, interpreter));
    let runs: Vec<Vec<Run>> = lines
        .iter()
        .map(|line| highlight_line(line, syntax, set, theme))
        .collect();
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some(Cached {
            hash,
            interpreter,
            theme: theme.name,
            runs: runs.clone(),
        });
    }
    runs
}

fn highlight_line(
    line: &str,
    syntax: Option<&SyntaxReference>,
    set: &SyntaxSet,
    theme: &Theme,
) -> Vec<Run> {
    let chars = line.chars().count();
    let mut runs = match syntax {
        Some(syntax) => script_runs(line, syntax, set, theme),
        None => vec![Run {
            start: 0,
            end: chars,
            style: Style::default().fg(theme.text),
        }],
    };
    overlay_taurine_tags(line, &mut runs, theme);
    runs
}

/// Script grammar runs via syntect parse states, mapped onto [`Theme`].
/// Unmatched tokens fall back to body text so the line always tiles.
fn script_runs(line: &str, syntax: &SyntaxReference, set: &SyntaxSet, theme: &Theme) -> Vec<Run> {
    let plain = Style::default().fg(theme.text);
    let map = ByteMap::new(line);
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut runs = Vec::new();
    // honey: newline kept for the parser (heredocs fold lines),
    // stripped from the painted run.
    let parsed = format!("{line}\n");
    for token in LinesWithEndings::from(&parsed) {
        let Ok(ops) = state.parse_line(token, set) else {
            continue;
        };
        for (index, (start, op)) in ops.iter().enumerate() {
            let end = ops
                .get(index + 1)
                .map(|(next, _)| *next)
                .unwrap_or(token.len());
            let _ = stack.apply(op);
            if end <= *start || *start >= token.len() {
                continue;
            }
            let style = selectors()
                .iter()
                .find(|(sel, _)| sel.does_match(stack.as_slice()).is_some())
                .map(|(_, kind)| kind_style(*kind, theme))
                .unwrap_or(plain);
            let (start, end) = (map.to_char(*start), map.to_char(end.min(token.len())));
            if end > start {
                runs.push(Run { start, end, style });
            }
        }
    }
    if runs.is_empty() {
        runs.push(Run {
            start: 0,
            end: line.chars().count(),
            style: plain,
        });
    }
    runs
}

/// Byte-to-char index for one line; lookups only land on char
/// boundaries (parser token edges), clamped defensively.
struct ByteMap {
    map: Vec<usize>,
    chars: usize,
}

impl ByteMap {
    fn new(line: &str) -> Self {
        let mut map = vec![0; line.len() + 1];
        for (char_index, (byte_index, _)) in line.char_indices().enumerate() {
            map[byte_index] = char_index;
        }
        let chars = line.chars().count();
        map[line.len()] = chars;
        Self { map, chars }
    }

    fn to_char(&self, byte: usize) -> usize {
        self.map
            .get(byte.min(self.map.len() - 1))
            .copied()
            .unwrap_or(self.chars)
    }
}

/// Taurine template overlay: outermost `[...]` regions re-tokenized with
/// core parsers, replacing script runs underneath.
fn overlay_taurine_tags(line: &str, runs: &mut Vec<Run>, theme: &Theme) {
    let map = ByteMap::new(line);
    let mut overlay = Vec::new();
    let mut covered = Vec::new();
    let mut cursor = 0;
    while let Some(tag) = tags::find_next_tag(line, cursor) {
        covered.push((map.to_char(tag.start), map.to_char(tag.end + 1)));
        overlay.extend(tokenize_tag(line, tag.start, tag.end, &map, theme));
        cursor = tag.end + 1;
    }
    if covered.is_empty() {
        return;
    }
    // honey: trim (not drop) base runs around covered ranges so text
    // outside tags keeps its script styling.
    let mut kept = Vec::with_capacity(runs.len());
    for run in runs.drain(..) {
        let mut start = run.start;
        for (cover_start, cover_end) in covered.iter() {
            if *cover_end <= start || *cover_start >= run.end {
                continue;
            }
            if *cover_start > start {
                kept.push(Run {
                    start,
                    end: *cover_start,
                    style: run.style,
                });
            }
            start = start.max(*cover_end);
        }
        if start < run.end {
            kept.push(Run {
                start,
                end: run.end,
                style: run.style,
            });
        }
    }
    *runs = kept;
    runs.extend(overlay);
    runs.sort_by_key(|run| (run.start, run.end));
}

/// Argument span styles, resolved from [`Theme`] once per tag.
struct ArgStyles {
    plain: Style,
    muted: Style,
    string: Style,
    number: Style,
}

/// Tokenize one `[...]` region (byte `start..=end`) into tiling runs.
fn tokenize_tag(line: &str, start: usize, end: usize, map: &ByteMap, theme: &Theme) -> Vec<Run> {
    let styles = ArgStyles {
        plain: Style::default().fg(theme.text),
        muted: Style::default().fg(theme.text_muted),
        string: Style::default().fg(theme.success),
        number: Style::default().fg(theme.warning),
    };
    let mut runs = vec![
        Run {
            start: map.to_char(start),
            end: map.to_char(start + 1),
            style: styles.muted,
        },
        Run {
            start: map.to_char(end),
            end: map.to_char(end + 1),
            style: styles.muted,
        },
    ];
    let inner = &line[start + 1..end];
    let offset = start + 1;
    let to_char = |byte: usize| map.to_char(byte);
    let pipeline = transformers::split_pipeline(inner);
    // honey: trimmed segments lose positions; re-locate in order.
    let mut cursor = 0usize;
    for (index, segment) in pipeline.iter().enumerate() {
        let Some(relative) = inner[cursor..].find(segment) else {
            break;
        };
        let seg_start = cursor + relative;
        paint_gap(inner, cursor, seg_start, &styles, &mut runs, |byte| {
            map.to_char(offset + byte)
        });
        if index == 0 {
            paint_base_segment(
                segment,
                offset + seg_start,
                &styles,
                theme,
                &mut runs,
                &to_char,
            );
        } else {
            paint_transformer_segment(
                segment,
                offset + seg_start,
                &styles,
                theme,
                &mut runs,
                &to_char,
            );
        }
        cursor = seg_start + segment.len();
    }
    paint_gap(inner, cursor, inner.len(), &styles, &mut runs, |byte| {
        map.to_char(offset + byte)
    });
    runs
}

/// Gap between segments: `|` muted, everything else plain text.
fn paint_gap(
    inner: &str,
    from: usize,
    to: usize,
    styles: &ArgStyles,
    runs: &mut Vec<Run>,
    to_char: impl Fn(usize) -> usize,
) {
    for (offset, ch) in inner[from..to].char_indices() {
        let byte = from + offset;
        runs.push(Run {
            start: to_char(byte),
            end: to_char(byte + ch.len_utf8()),
            style: if ch == '|' {
                styles.muted
            } else {
                styles.plain
            },
        });
    }
}

/// Directives and executables that deserve attention over plain roots.
fn is_attention_root(root: &str) -> bool {
    matches!(
        root,
        "key" | "delay" | "mouse" | "cursor" | "image" | "execute"
    )
}

/// First pipeline segment: system call, user variable, or invalid.
fn paint_base_segment(
    segment: &str,
    offset: usize,
    styles: &ArgStyles,
    theme: &Theme,
    runs: &mut Vec<Run>,
    to_char: &dyn Fn(usize) -> usize,
) {
    let Some((namespace, _)) = parse_system_call(segment) else {
        paint_user_or_invalid(segment, offset, styles, theme, runs, to_char);
        return;
    };
    let root = namespace.split('.').next().unwrap_or(namespace);
    let style = if is_attention_root(&root.to_ascii_lowercase()) {
        Style::default().fg(theme.warning)
    } else {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    };
    let Some(relative) = segment.find(namespace) else {
        return;
    };
    runs.push(Run {
        start: to_char(offset + relative),
        end: to_char(offset + relative + namespace.len()),
        style,
    });
    paint_args(
        &segment[relative + namespace.len()..],
        offset + relative + namespace.len(),
        styles,
        runs,
        to_char,
    );
}

/// Bare `[name]` / `[name=default]` user variables; anything else errors.
fn paint_user_or_invalid(
    segment: &str,
    offset: usize,
    styles: &ArgStyles,
    theme: &Theme,
    runs: &mut Vec<Run>,
    to_char: &dyn Fn(usize) -> usize,
) {
    let error = Style::default().fg(theme.error);
    let (key, default) = tags::split_key_default(segment);
    let key = key.trim();
    let valid = !key.is_empty()
        && key
            .chars()
            .enumerate()
            .all(|(index, ch)| ch == '_' || ch.is_alphabetic() || (index > 0 && ch.is_numeric()));
    if !valid {
        runs.push(Run {
            start: to_char(offset),
            end: to_char(offset + segment.len()),
            style: error,
        });
        return;
    }
    if let Some(relative) = segment.find(key) {
        runs.push(Run {
            start: to_char(offset + relative),
            end: to_char(offset + relative + key.len()),
            style: styles.plain,
        });
    }
    if let Some(default) = default
        && let Some(relative) = segment.find(default)
    {
        paint_args(default, offset + relative, styles, runs, to_char);
    }
}

/// `name(args)` transformer: value errors surface in error red.
fn paint_transformer_segment(
    segment: &str,
    offset: usize,
    styles: &ArgStyles,
    theme: &Theme,
    runs: &mut Vec<Run>,
    to_char: &dyn Fn(usize) -> usize,
) {
    let error = Style::default().fg(theme.error);
    let name_style = Style::default().fg(theme.primary);
    let Some((name, _)) = transformers::transformer_call_parts(segment) else {
        runs.push(Run {
            start: to_char(offset),
            end: to_char(offset + segment.len()),
            style: error,
        });
        return;
    };
    let style = if transformers::transformer_value_error(segment).is_some() {
        error
    } else {
        name_style
    };
    let Some(relative) = segment.find(name) else {
        return;
    };
    runs.push(Run {
        start: to_char(offset + relative),
        end: to_char(offset + relative + name.len()),
        style,
    });
    paint_args(
        &segment[relative + name.len()..],
        offset + relative + name.len(),
        styles,
        runs,
        to_char,
    );
}

/// Argument span: quoted strings read as strings, bare numbers as
/// numbers, parens and commas stay muted.
fn paint_args(
    args: &str,
    offset: usize,
    styles: &ArgStyles,
    runs: &mut Vec<Run>,
    to_char: &dyn Fn(usize) -> usize,
) {
    let bytes = args.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\'' || byte == b'"' {
            let mut end = index + 1;
            while end < bytes.len() && (bytes[end] != byte || tags::is_escaped(bytes, end)) {
                end += 1;
            }
            end = (end + 1).min(bytes.len());
            runs.push(Run {
                start: to_char(offset + index),
                end: to_char(offset + end),
                style: styles.string,
            });
            index = end;
        } else if byte.is_ascii_digit() {
            let mut end = index;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            runs.push(Run {
                start: to_char(offset + index),
                end: to_char(offset + end),
                style: styles.number,
            });
            index = end;
        } else {
            let ch = args[index..].chars().next().unwrap_or(' ');
            runs.push(Run {
                start: to_char(offset + index),
                end: to_char(offset + index + ch.len_utf8()),
                style: if "(),".contains(ch) {
                    styles.muted
                } else {
                    styles.plain
                },
            });
            index += ch.len_utf8();
        }
    }
}

/// Slice tiled runs to a char `range`, applying `REVERSED` over an
/// optional selection (also char units). Text owned for `'static` spans.
pub(crate) fn spans_for_range(
    line: &str,
    runs: &[Run],
    range: Range<usize>,
    selection: Option<Range<usize>>,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = line.chars().collect();
    let text = |range: Range<usize>| {
        chars[range.start.min(chars.len())..range.end.min(chars.len())]
            .iter()
            .collect::<String>()
    };
    let mut spans = Vec::new();
    for run in runs {
        let start = run.start.max(range.start);
        let end = run.end.min(range.end);
        if end <= start {
            continue;
        }
        let selected = selection
            .as_ref()
            .is_some_and(|sel| start < sel.end && end > sel.start);
        let style = if selected {
            run.style.add_modifier(Modifier::REVERSED)
        } else {
            run.style
        };
        spans.push(Span::styled(text(start..end), style));
    }
    if spans.is_empty() {
        spans.push(Span::raw(text(range)));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin::DARK_THEME;

    fn plain_runs(content: &str) -> Vec<Vec<Run>> {
        highlight_lines(
            &content.lines().map(str::to_string).collect::<Vec<_>>(),
            None,
            &DARK_THEME,
        )
    }

    fn assert_tiles(line: &str, runs: &[Run]) {
        let mut covered = vec![false; line.chars().count()];
        for run in runs {
            for cell in &mut covered[run.start..run.end] {
                assert!(!*cell, "overlapping runs in {line:?}");
                *cell = true;
            }
        }
        assert!(covered.iter().all(|covered| *covered), "gap in {line:?}");
    }

    #[test]
    fn render_cache_reuses_work() {
        let lines = ["echo [clip] # hi".to_string()];
        let first = highlight_lines(&lines, Some(ScriptInterpreter::Bash), &DARK_THEME);
        let second = highlight_lines(&lines, Some(ScriptInterpreter::Bash), &DARK_THEME);
        assert_eq!(first, second);
    }

    #[test]
    fn powershell_falls_back_to_plain_with_overlay() {
        // honey: two-face excludes PowerShell from its fancy-regex dump
        // (onig-only patterns); revisit if it gets re-included.
        let set = syntax_set();
        assert!(syntax_for(set, ScriptInterpreter::PowerShell).is_none());
        let line = "Write-Host [clip]";
        let lines = [line.to_string()];
        let highlighted = highlight_lines(&lines, Some(ScriptInterpreter::PowerShell), &DARK_THEME);
        assert_tiles(line, &highlighted[0]);
        assert!(
            highlighted[0]
                .iter()
                .any(|run| run.style.fg == Some(DARK_THEME.accent)),
            "overlay must still highlight tags, got {:?}",
            highlighted[0]
        );
    }

    #[test]
    fn other_interpreters_resolve_a_grammar() {
        let set = syntax_set();
        for interpreter in ScriptInterpreter::ALL {
            if interpreter == ScriptInterpreter::PowerShell {
                continue;
            }
            assert!(
                syntax_for(set, interpreter).is_some(),
                "{interpreter:?} has no syntect grammar"
            );
        }
    }

    #[test]
    fn script_tokens_highlight_and_tile() {
        let line = "echo hello # comment";
        let lines = [line.to_string()];
        let highlighted = highlight_lines(&lines, Some(ScriptInterpreter::Bash), &DARK_THEME);
        assert_tiles(line, &highlighted[0]);
        assert!(
            highlighted[0]
                .iter()
                .any(|run| run.style.fg == Some(DARK_THEME.text_muted)),
            "comment must dim, got {:?}",
            highlighted[0]
        );
    }

    #[test]
    fn all_five_grammars_tile() {
        let samples: [(ScriptInterpreter, &str); 5] = [
            (ScriptInterpreter::Bash, "echo \"$HOME\" # hi"),
            (ScriptInterpreter::PowerShell, "Write-Host 'hi' # hi"),
            (ScriptInterpreter::Python, "print('hi')  # hi"),
            (ScriptInterpreter::Node, "console.log('hi'); // hi"),
            (ScriptInterpreter::Cmd, "@echo off & REM hi"),
        ];
        for (interpreter, sample) in samples {
            let lines = [sample.to_string()];
            let highlighted = highlight_lines(&lines, Some(interpreter), &DARK_THEME);
            assert_tiles(sample, &highlighted[0]);
        }
    }

    #[test]
    fn system_root_and_transformer_highlight() {
        let runs = &plain_runs("[clip | case(upper)]")[0];
        assert!(
            runs.iter()
                .any(|run| run.style.fg == Some(DARK_THEME.accent)),
            "system root must accent, got {runs:?}"
        );
        assert!(
            runs.iter()
                .any(|run| run.style.fg == Some(DARK_THEME.primary)),
            "transformer must primary, got {runs:?}"
        );
    }

    #[test]
    fn invalid_tag_errors() {
        let runs = &plain_runs("[nope(1)]")[0];
        assert!(
            runs.iter()
                .any(|run| run.style.fg == Some(DARK_THEME.error)),
            "invalid tag must error, got {runs:?}"
        );
    }

    #[test]
    fn user_variable_stays_plain() {
        let runs = &plain_runs("hi [name] bye")[0];
        assert!(
            runs.iter()
                .all(|run| run.style.fg != Some(DARK_THEME.error)),
            "user var must not error, got {runs:?}"
        );
    }

    #[test]
    fn overlay_wins_over_script_grammar() {
        let lines = ["echo [clip]".to_string()];
        let highlighted = highlight_lines(&lines, Some(ScriptInterpreter::Bash), &DARK_THEME);
        assert!(
            highlighted[0]
                .iter()
                .any(|run| run.style.fg == Some(DARK_THEME.accent)),
            "tag must override bash, got {:?}",
            highlighted[0]
        );
    }

    #[test]
    fn spans_slice_and_reverse_selection() {
        let line = "ab [clip] cd";
        let runs = &plain_runs(line)[0];
        let full = spans_for_range(line, runs, 0..line.chars().count(), None);
        assert_eq!(
            full.iter()
                .map(|span| span.content.clone())
                .collect::<String>(),
            line
        );
        let selected = spans_for_range(line, runs, 0..line.chars().count(), Some(0..2));
        assert!(
            selected
                .iter()
                .any(|span| span.style.add_modifier.contains(Modifier::REVERSED))
        );
    }

    #[test]
    fn escaped_tags_stay_literal() {
        let line = "a \\[clip] b";
        let runs = &plain_runs(line)[0];
        assert!(
            runs.iter()
                .all(|run| run.style.fg != Some(DARK_THEME.accent)),
            "escaped tag must not highlight, got {runs:?}"
        );
    }
}
