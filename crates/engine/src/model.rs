//! Raw model ids → provider / family / version, so views can filter at any level:
//! everything, all Anthropic, all Claude Opus, or exactly Claude Opus 5.5.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

/// A model id split into the levels views filter by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    /// Who makes it: Anthropic, OpenAI, Meta, …
    pub provider: String,
    /// The product line ("Claude Opus", "GPT Mini", …).
    pub family: String,
    /// Human version like "5.5". Empty when the id carries none.
    pub version: String,
    /// The id as the source recorded it.
    pub raw: String,
}

impl Model {
    /// "Claude Opus 5.5", or just the family when there's no version.
    pub fn display(&self) -> String {
        if self.version.is_empty() {
            self.family.clone()
        } else {
            format!("{} {}", self.family, self.version)
        }
    }
}

static DATE_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[-@](20\d{6}|\d{4}-\d{2}-\d{2}|latest|preview.*|exp.*)$").unwrap());
static CLAUDE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^claude-(?:(\d+)(?:[-.](\d))?-)?([a-z]+)(?:-(\d+)(?:[-.](\d+))?)?").unwrap());
static GPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:chatgpt-|gpt-)(\d+(?:\.\d+)?)(o)?(?:-([a-z]+))?").unwrap());
static O_SERIES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^o(\d+)(?:-([a-z]+))?").unwrap());
static GEMINI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^gemini-(\d+(?:\.\d+)?)-([a-z]+)").unwrap());
static LOCAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(llama|qwen|mistral|mixtral|gemma|phi|deepseek|codellama|gpt-oss|granite|command-r|olmo|smollm|kimi|glm)[-_ ]?(\d+(?:\.\d+)?)?").unwrap()
});

/// Splits a raw model id into provider / family / version. `None` for placeholders and blanks;
/// ids it doesn't recognize come back under provider "Other".
pub fn normalize_model(raw: &str) -> Option<Model> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('<') {
        return None; // e.g. Claude Code's "<synthetic>" placeholder replies
    }
    // Local runners prefix ids with an org or path ("lmstudio-community/Meta-Llama-3-8B-Instruct-GGUF").
    let base = raw.rsplit('/').next().unwrap_or(raw).to_ascii_lowercase();
    let base = base.trim_start_matches("meta-");
    let id = DATE_SUFFIX.replace(base, "");
    let id = id.as_ref();
    let m = |provider: &str, family: String, version: String| {
        Some(Model { provider: provider.into(), family, version, raw: raw.into() })
    };

    if let Some(c) = CLAUDE.captures(id) {
        let tier = title(&c[3]);
        // Old ids put the version first (claude-3-5-sonnet); new ids put it last (claude-opus-5-5).
        let (major, minor) = match (c.get(1), c.get(4)) {
            (_, Some(maj)) => (maj.as_str(), c.get(5).map(|x| x.as_str())),
            (Some(maj), None) => (maj.as_str(), c.get(2).map(|x| x.as_str())),
            (None, None) => return m("Anthropic", format!("Claude {tier}"), String::new()),
        };
        let version = match minor {
            Some(min) if min.len() < 3 => format!("{major}.{min}"),
            _ => major.to_string(),
        };
        return m("Anthropic", format!("Claude {tier}"), version);
    }
    if let Some(c) = GPT.captures(id) {
        let mut version = c[1].to_string();
        if c.get(2).is_some() {
            version.push('o');
        }
        let family = match c.get(3).map(|x| x.as_str()) {
            Some(v) => format!("GPT {}", title(v)), // mini, nano, codex, astra, …
            None => "GPT".to_string(),
        };
        return m("OpenAI", family, version);
    }
    if let Some(c) = O_SERIES.captures(id) {
        let family = match c.get(2).map(|x| x.as_str()) {
            Some(v @ ("mini" | "pro")) => format!("o-series {}", title(v)),
            _ => "o-series".to_string(),
        };
        return m("OpenAI", family, format!("o{}", &c[1]));
    }
    if let Some(c) = GEMINI.captures(id) {
        return m("Google", format!("Gemini {}", title(&c[2])), c[1].to_string());
    }
    if let Some(c) = LOCAL.captures(id) {
        let fam = &c[1];
        let provider = match fam {
            "llama" | "codellama" => "Meta",
            "qwen" => "Alibaba",
            "mistral" | "mixtral" => "Mistral",
            "gemma" => "Google",
            "phi" => "Microsoft",
            "deepseek" => "DeepSeek",
            "gpt-oss" => "OpenAI",
            _ => "Open model",
        };
        let family = if fam == "gpt-oss" { "gpt-oss".to_string() } else { title(fam) };
        return m(provider, family, c.get(2).map_or(String::new(), |v| v.as_str().to_string()));
    }
    m("Other", raw.to_string(), String::new())
}

fn title(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(raw: &str) -> (String, String, String) {
        let m = normalize_model(raw).unwrap();
        (m.provider, m.family, m.version)
    }
    fn t(p: &str, f: &str, v: &str) -> (String, String, String) {
        (p.into(), f.into(), v.into())
    }

    #[test]
    fn claude_ids() {
        assert_eq!(n("claude-opus-5-5"), t("Anthropic", "Claude Opus", "5.5"));
        assert_eq!(n("claude-opus-5"), t("Anthropic", "Claude Opus", "5"));
        assert_eq!(n("claude-fable-5-1"), t("Anthropic", "Claude Fable", "5.1"));
        assert_eq!(n("claude-haiku-4-5-20251001"), t("Anthropic", "Claude Haiku", "4.5"));
        assert_eq!(n("claude-sonnet-4-20250514"), t("Anthropic", "Claude Sonnet", "4"));
        assert_eq!(n("claude-3-5-sonnet-20241022"), t("Anthropic", "Claude Sonnet", "3.5"));
        assert_eq!(n("claude-3-opus-20240229"), t("Anthropic", "Claude Opus", "3"));
    }

    #[test]
    fn openai_ids() {
        assert_eq!(n("gpt-5.1"), t("OpenAI", "GPT", "5.1"));
        assert_eq!(n("gpt-4o"), t("OpenAI", "GPT", "4o"));
        assert_eq!(n("gpt-4o-mini"), t("OpenAI", "GPT Mini", "4o"));
        assert_eq!(n("gpt-5-codex"), t("OpenAI", "GPT Codex", "5"));
        assert_eq!(n("gpt-6-astra"), t("OpenAI", "GPT Astra", "6"));
        assert_eq!(n("gpt-5.6-terra"), t("OpenAI", "GPT Terra", "5.6"));
        assert_eq!(n("gpt-4o-2024-08-06"), t("OpenAI", "GPT", "4o"));
        assert_eq!(n("o3"), t("OpenAI", "o-series", "o3"));
        assert_eq!(n("o4-mini"), t("OpenAI", "o-series Mini", "o4"));
    }

    #[test]
    fn google_and_local_ids() {
        assert_eq!(n("gemini-2.5-pro"), t("Google", "Gemini Pro", "2.5"));
        assert_eq!(n("gemini-2.0-flash-exp"), t("Google", "Gemini Flash", "2.0"));
        assert_eq!(n("llama3.1:8b"), t("Meta", "Llama", "3.1"));
        assert_eq!(n("lmstudio-community/Meta-Llama-3-8B-Instruct-GGUF"), t("Meta", "Llama", "3"));
        assert_eq!(n("qwen2.5-coder:7b"), t("Alibaba", "Qwen", "2.5"));
        assert_eq!(n("openai/gpt-oss-20b"), t("OpenAI", "gpt-oss", "20"));
    }

    #[test]
    fn placeholders_and_unknowns() {
        assert!(normalize_model("<synthetic>").is_none());
        assert!(normalize_model("").is_none());
        assert_eq!(n("my-finetune"), t("Other", "my-finetune", ""));
        assert_eq!(normalize_model("claude-opus-5-5").unwrap().display(), "Claude Opus 5.5");
    }
}
