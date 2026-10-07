//! Center-pane syntax highlighting: script grammars via tree-sitter
//! (real parsers, error-tolerant mid-typing) mapped onto the Taurine
//! [`Theme`] palette, plus a Taurine template overlay (`[vars]`,
//! `| transformers`, directives) that wins inside `[...]` regions.
//! Runs are char-indexed so they survive the word-wrap in `detail.rs`.

use std::ops::Range;
use std::sync::{Mutex, OnceLock};

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use taurine_core::engine::ScriptInterpreter;
use taurine_core::engine::variables::system::transformers;
use taurine_core::engine::variables::{parse_system_call, tags};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

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
    Variable,
    Invalid,
}

/// Recognized tree-sitter capture names, index-aligned with [`kind_for`].
/// Unrecognized captures (operators, punctuation) fall through to plain
/// body text. Matching is longest-prefix on dot parts, so
/// `function.builtin` lands on `function`.
const NAMES: [&str; 8] = [
    "comment", "string", "number", "keyword", "function", "type", "variable", "error",
];

fn kind_for(index: usize) -> Kind {
    const KINDS: [Kind; 8] = [
        Kind::Comment,
        Kind::String,
        Kind::Number,
        Kind::Keyword,
        Kind::Function,
        Kind::Type,
        Kind::Variable,
        Kind::Invalid,
    ];
    KINDS[index.min(KINDS.len() - 1)]
}

fn kind_style(kind: Kind, theme: &Theme) -> Style {
    match kind {
        Kind::Comment => Style::default().fg(theme.syntax_comment),
        Kind::String => Style::default().fg(theme.syntax_string),
        Kind::Number => Style::default().fg(theme.syntax_constant),
        Kind::Keyword => Style::default().fg(theme.syntax_keyword),
        Kind::Function => Style::default().fg(theme.syntax_entity),
        Kind::Type => Style::default().fg(theme.syntax_type),
        Kind::Variable => Style::default().fg(theme.syntax_variable),
        Kind::Invalid => Style::default().fg(theme.error),
    }
}

/// Highlight configurations, one per interpreter, query-compiled once.
/// A language whose query fails to compile resolves to `None` and the
/// content falls back to plain text plus the Taurine overlay.
static CONFIGS: OnceLock<Vec<(ScriptInterpreter, HighlightConfiguration)>> = OnceLock::new();

fn configs() -> &'static [(ScriptInterpreter, HighlightConfiguration)] {
    CONFIGS.get_or_init(|| {
        ScriptInterpreter::ALL
            .iter()
            .filter_map(|interpreter| {
                build_config(*interpreter).map(|config| (*interpreter, config))
            })
            .collect()
    })
}

fn build_config(interpreter: ScriptInterpreter) -> Option<HighlightConfiguration> {
    use tree_sitter::{Language, Parser};

    // honey: grammar crates pin their own query sources; empty injection
    // and locals (no cross-language injection or locals tracking in a
    // 16-row preview).
    let (language, name, query): (Language, &str, &str) = match interpreter {
        ScriptInterpreter::Bash => (
            tree_sitter_bash::LANGUAGE.into(),
            "bash",
            tree_sitter_bash::HIGHLIGHT_QUERY,
        ),
        ScriptInterpreter::PowerShell => (
            tree_sitter_powershell::LANGUAGE.into(),
            "powershell",
            tree_sitter_powershell::HIGHLIGHTS_QUERY,
        ),
        ScriptInterpreter::Python => (
            tree_sitter_python::LANGUAGE.into(),
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY,
        ),
        ScriptInterpreter::Node => (
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
        ),
        ScriptInterpreter::Cmd => (
            tree_sitter_batch::LANGUAGE.into(),
            "batch",
            tree_sitter_batch::HIGHLIGHTS_QUERY,
        ),
    };
    // honey: fail fast on ABI drift — a grammar the linked runtime can't
    // load must surface here, not as mis-highlighted text.
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let mut config = HighlightConfiguration::new(language, name, query, "", "").ok()?;
    config.configure(&NAMES);
    Some(config)
}

/// Highlight configuration per Taurine script interpreter.
fn config_for(interpreter: ScriptInterpreter) -> Option<&'static HighlightConfiguration> {
    configs()
        .iter()
        .find(|(candidate, _)| *candidate == interpreter)
        .map(|(_, config)| config)
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
    let runs: Vec<Vec<Run>> = match interpreter.and_then(config_for) {
        Some(config) => script_runs(lines, config, theme),
        None => lines
            .iter()
            .map(|line| {
                let mut runs = vec![Run {
                    start: 0,
                    end: line.chars().count(),
                    style: Style::default().fg(theme.text),
                }];
                overlay_taurine_tags(line, &mut runs, theme);
                runs
            })
            .collect(),
    };
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

/// Script grammar runs for the whole document (multi-line strings and
/// heredocs fold lines), sliced per source line. Unmatched spans fall
/// back to body text so every line tiles.
fn script_runs(lines: &[String], config: &HighlightConfiguration, theme: &Theme) -> Vec<Vec<Run>> {
    let plain = Style::default().fg(theme.text);
    let content = lines.join("\n");
    // honey: line byte ranges over the joined text; the `\r` of a CRLF
    // pair lands in the gap and never paints (invisible anyway).
    let mut starts = Vec::with_capacity(lines.len());
    let mut cursor = 0usize;
    for line in lines {
        starts.push(cursor);
        cursor += line.len() + 1;
    }
    let mut highlighter = Highlighter::new();
    let mut segments: Vec<(usize, usize, Style)> = Vec::new();
    let highlight = highlighter.highlight(config, content.as_bytes(), None, None, |_| None);
    // honey: a document the parser rejects wholesale still shows
    // text; per-line tiling below covers the gap.
    if let Ok(events) = highlight {
        let mut stack = vec![plain];
        for event in events {
            let Ok(event) = event else {
                continue;
            };
            match event {
                HighlightEvent::Source { start, end } => {
                    if let Some(style) = stack.last() {
                        segments.push((start, end, *style));
                    }
                }
                HighlightEvent::HighlightStart(index) => {
                    stack.push(kind_style(kind_for(index.0), theme));
                }
                HighlightEvent::HighlightEnd => {
                    stack.pop();
                    if stack.is_empty() {
                        stack.push(plain);
                    }
                }
            }
        }
    }
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let map = ByteMap::new(line);
            let (line_start, line_end) = (starts[index], starts[index] + line.len());
            let mut runs: Vec<Run> = segments
                .iter()
                .filter_map(|(start, end, style)| {
                    let (start, end) = (*start, *end);
                    let (start, end) = (
                        map.to_char(start.max(line_start) - line_start),
                        map.to_char(end.min(line_end).max(line_start) - line_start),
                    );
                    (end > start).then_some(Run {
                        start,
                        end,
                        style: *style,
                    })
                })
                .collect();
            if runs.is_empty() {
                runs.push(Run {
                    start: 0,
                    end: line.chars().count(),
                    style: plain,
                });
            }
            overlay_taurine_tags(line, &mut runs, theme);
            runs
        })
        .collect()
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
/// Quoted text reads green, bare values orange, names blue — the One
/// Dark roles; punctuation reuses the muted UI grey.
struct ArgStyles {
    plain: Style,
    punct: Style,
    name: Style,
    string: Style,
    constant: Style,
}

/// Tokenize one `[...]` region (byte `start..=end`) into tiling runs.
fn tokenize_tag(line: &str, start: usize, end: usize, map: &ByteMap, theme: &Theme) -> Vec<Run> {
    let styles = ArgStyles {
        plain: Style::default().fg(theme.text),
        punct: Style::default().fg(theme.text_muted),
        name: Style::default().fg(theme.syntax_entity),
        string: Style::default().fg(theme.syntax_string),
        constant: Style::default().fg(theme.syntax_constant),
    };
    let mut runs = vec![
        Run {
            start: map.to_char(start),
            end: map.to_char(start + 1),
            style: styles.punct,
        },
        Run {
            start: map.to_char(end),
            end: map.to_char(end + 1),
            style: styles.punct,
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

/// Gap between segments: `|` separates, everything else plain text.
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
                styles.punct
            } else {
                styles.plain
            },
        });
    }
}

/// First pipeline segment: system call, user variable, or invalid.
/// System names share one pastel tone; values carry their own.
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
    let Some(relative) = segment.find(namespace) else {
        return;
    };
    runs.push(Run {
        start: to_char(offset + relative),
        end: to_char(offset + relative + namespace.len()),
        style: styles.name,
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
    let name_style = styles.name;
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

/// Argument span: quoted text reads green, bare words and numbers
/// orange, parens and commas stay muted.
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
                style: styles.constant,
            });
            index = end;
        } else {
            let ch = args[index..].chars().next().unwrap_or(' ');
            runs.push(Run {
                start: to_char(offset + index),
                end: to_char(offset + index + ch.len_utf8()),
                style: if "(),".contains(ch) {
                    styles.punct
                } else {
                    styles.constant
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
    fn powershell_highlights_cmdlet_and_variable() {
        let line = "Start-Process $Env:USERPROFILE\\Downloads";
        let lines = [line.to_string()];
        let highlighted = highlight_lines(&lines, Some(ScriptInterpreter::PowerShell), &DARK_THEME);
        assert_tiles(line, &highlighted[0]);
        assert!(
            highlighted[0]
                .iter()
                .any(|run| run.style.fg == Some(DARK_THEME.syntax_entity)),
            "cmdlet must entity-tone, got {:?}",
            highlighted[0]
        );
        assert!(
            highlighted[0]
                .iter()
                .any(|run| run.style != Style::default().fg(DARK_THEME.text)),
            "grammar must add color, got {:?}",
            highlighted[0]
        );
    }

    #[test]
    fn every_interpreter_resolves_a_config() {
        for interpreter in ScriptInterpreter::ALL {
            assert!(
                config_for(interpreter).is_some(),
                "{interpreter:?} has no tree-sitter config"
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
                .any(|run| run.style.fg == Some(DARK_THEME.syntax_comment)),
            "comment must recede, got {:?}",
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
                .any(|run| run.style.fg == Some(DARK_THEME.syntax_entity)),
            "names must entity-tone, got {runs:?}"
        );
        assert!(
            runs.iter()
                .any(|run| run.style.fg == Some(DARK_THEME.syntax_constant)),
            "bare value must constant-tone, got {runs:?}"
        );
    }

    #[test]
    fn quoted_values_read_green() {
        let runs = &plain_runs("[env(\"HOME\")]")[0];
        assert!(
            runs.iter()
                .any(|run| run.style.fg == Some(DARK_THEME.syntax_string)),
            "quoted value must string-tone, got {runs:?}"
        );
    }

    #[test]
    fn brackets_and_pipes_highlight() {
        let runs = &plain_runs("[clip | case(upper)]")[0];
        for marker in ["[", "]", "|", "(", ")"] {
            assert!(
                runs.iter()
                    .any(|run| run.style.fg == Some(DARK_THEME.text_muted)),
                "{marker} must recede, got {runs:?}"
            );
        }
    }

    #[test]
    fn directive_name_and_value_highlight() {
        let line = "cd C:\\Projects\\taurine[key(enter)]";
        let runs = &plain_runs(line)[0];
        assert!(
            runs.iter()
                .any(|run| run.style.fg == Some(DARK_THEME.syntax_entity)),
            "name must highlight, got {runs:?}"
        );
        assert!(
            runs.iter()
                .any(|run| run.style.fg == Some(DARK_THEME.syntax_constant)),
            "value must highlight, got {runs:?}"
        );
        assert!(
            runs.iter()
                .all(|run| run.style.fg != Some(DARK_THEME.warning)),
            "no saturated yellow, got {runs:?}"
        );
    }

    #[test]
    fn palette_stays_restrained() {
        // honey: the rainbow guard — a rich mixed line may use at most
        // four accent tones besides body text and error red.
        use std::collections::HashSet;
        let line = "echo [clip | case(upper)] # done [key(enter)] [nope(1)]";
        let lines = [line.to_string()];
        let highlighted = highlight_lines(&lines, Some(ScriptInterpreter::Bash), &DARK_THEME);
        let tones: HashSet<_> = highlighted[0]
            .iter()
            .map(|run| run.style.fg)
            .filter(|fg| *fg != Some(DARK_THEME.text) && *fg != Some(DARK_THEME.error))
            .collect();
        assert!(tones.len() <= 4, "rainbow creep: {tones:?}");
    }

    #[test]
    fn no_bold_anywhere_in_highlighting() {
        let samples = [
            (Some(ScriptInterpreter::Bash), "echo hello # comment"),
            (
                Some(ScriptInterpreter::PowerShell),
                "Start-Process $Env:USERPROFILE\\Downloads",
            ),
            (Some(ScriptInterpreter::Python), "def f(x): return x  # hi"),
            (Some(ScriptInterpreter::Node), "console.log('hi'); // hi"),
            (Some(ScriptInterpreter::Cmd), "@echo off & REM hi"),
            (
                None,
                "[clip | case(upper)] [key(enter)] [nope(1)] hi [name]",
            ),
        ];
        for (interpreter, sample) in samples {
            let lines = [sample.to_string()];
            let highlighted = highlight_lines(&lines, interpreter, &DARK_THEME);
            assert!(
                highlighted[0]
                    .iter()
                    .all(|run| !run.style.add_modifier.contains(Modifier::BOLD)),
                "bold found in {sample:?}: {:?}",
                highlighted[0]
            );
        }
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
                .any(|run| run.style.fg == Some(DARK_THEME.syntax_entity)),
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
                .all(|run| run.style.fg != Some(DARK_THEME.syntax_entity)),
            "escaped tag must not highlight, got {runs:?}"
        );
    }
}
