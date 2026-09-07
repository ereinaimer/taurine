//! Conversational diagnostic builder with multi-tier intent matching.

pub mod matcher;

#[cfg(test)]
mod tests;

/// A fluent diagnostic builder that renders clean, unquoted, actionable terminal messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    problem: String,
    did_you_mean: Option<String>,
    help: Option<String>,
    options: Vec<(String, Vec<String>)>,
    examples: Vec<String>,
}

impl Diagnostic {
    /// Create a new diagnostic with the given problem description.
    pub fn problem(msg: impl Into<String>) -> Self {
        Self {
            problem: msg.into(),
            did_you_mean: None,
            help: None,
            options: Vec::new(),
            examples: Vec::new(),
        }
    }

    /// Alias for `problem` constructor.
    pub fn new(msg: impl Into<String>) -> Self {
        Self::problem(msg)
    }

    /// Run intent matching on `input` against `candidates` and attach a suggestion if found.
    pub fn suggest(mut self, input: &str, candidates: &[&str]) -> Self {
        if let Some(best) = matcher::find_best_match(input, candidates) {
            self.did_you_mean = Some(best.to_string());
        }
        self
    }

    /// Explicitly provide a "Did you mean" suggestion.
    pub fn did_you_mean(mut self, suggestion: impl Into<String>) -> Self {
        self.did_you_mean = Some(suggestion.into());
        self
    }

    /// Provide conversational guidance or an actionable next step.
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Provide a labeled list of valid choices or alternatives.
    pub fn options(mut self, label: &str, items: &[&str]) -> Self {
        if !items.is_empty() {
            self.options.push((
                label.to_string(),
                items.iter().map(|s| s.to_string()).collect(),
            ));
        }
        self
    }

    /// Provide a command or usage example.
    pub fn example(mut self, ex: impl Into<String>) -> Self {
        self.examples.push(ex.into());
        self
    }

    /// Returns the suggestion if one was found or set.
    pub fn suggestion(&self) -> Option<&str> {
        self.did_you_mean.as_deref()
    }

    /// Returns the problem message.
    pub fn problem_message(&self) -> &str {
        &self.problem
    }

    /// Render the diagnostic into clean, conversational terminal text.
    pub fn render(&self) -> String {
        let mut sections: Vec<String> = Vec::new();

        // 1. Problem
        let prob = self.problem.trim();
        if !prob.is_empty() {
            sections.push(prob.to_string());
        }

        // 2. Did you mean?
        if let Some(ref suggestion) = self.did_you_mean
            && !suggestion.trim().is_empty()
        {
            let s = suggestion.trim();
            if s.starts_with("Did you mean") {
                sections.push(s.to_string());
            } else {
                sections.push(format!("Did you mean {}?", s.trim_end_matches('?')));
            }
        }

        // 3. Help
        if let Some(ref help) = self.help
            && !help.trim().is_empty()
        {
            sections.push(help.trim().to_string());
        }

        // 4. Options
        for (label, items) in &self.options {
            if !items.is_empty() {
                let clean_label = label.trim().trim_end_matches(':');
                sections.push(format!("{clean_label}: {}", items.join(", ")));
            }
        }

        // 5. Examples
        if !self.examples.is_empty() {
            if self.examples.len() == 1 {
                sections.push(format!("Example:\n  {}", self.examples[0].trim()));
            } else {
                let mut ex_sec = String::from("Examples:\n");
                for (idx, ex) in self.examples.iter().enumerate() {
                    ex_sec.push_str(&format!("  {}", ex.trim()));
                    if idx + 1 < self.examples.len() {
                        ex_sec.push('\n');
                    }
                }
                sections.push(ex_sec);
            }
        }

        sections.join("\n\n")
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render())
    }
}

impl From<Diagnostic> for String {
    fn from(d: Diagnostic) -> Self {
        d.render()
    }
}

impl std::error::Error for Diagnostic {}
