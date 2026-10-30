//! The plugin this app ships, and the check that what Claude Code downloaded is that plugin. A plugin's
//! hooks, shim and skill run on the person's computer, and a plugin can add tools, commands, agents,
//! monitors and settings just by having a file in the right place, so the download is held to an
//! allowlist: it may hold the files this app ships, byte for byte, and nothing else.
//!
//! Line endings don't count: a Windows checkout may turn `\n` into `\r\n` in either copy, and
//! `.gitattributes` only keeps that from happening to ours.

use std::borrow::Cow;
use std::path::Path;

/// The icon. A picture, so it is compared exactly (a line ending change would corrupt it).
const ICON: &str = ".claude-plugin/icon.png";

/// Every file the plugin has, as (path inside the plugin, our copy): the shim, the hook, the skill, the
/// manifest (which could name more things to run) and the icon. The download must hold each one.
pub(super) const SHIPPED: [(&str, &[u8]); 5] = [
    ("bin/na", include_bytes!("../../../../plugins/grade/bin/na")),
    ("hooks/hooks.json", include_bytes!("../../../../plugins/grade/hooks/hooks.json")),
    ("skills/me/SKILL.md", include_bytes!("../../../../plugins/grade/skills/me/SKILL.md")),
    (".claude-plugin/plugin.json", include_bytes!("../../../../plugins/grade/.claude-plugin/plugin.json")),
    (ICON, include_bytes!("../../../../plugins/grade/.claude-plugin/icon.png")),
];

/// The only folders the plugin has. A folder that isn't here is not ours, empty or not.
const FOLDERS: [&str; 5] = [".claude-plugin", "bin", "hooks", "skills", "skills/me"];

/// Files Claude Code itself leaves at the root of a plugin's folder, which hold no code and are never
/// run or read as instructions. Dotfiles only, as plain files.
const MARKERS: [&str; 1] = [".orphaned_at"];

/// Files a file manager leaves in any folder it has opened (macOS Finder, Windows Explorer). They are
/// never run or read, so they are allowed in any of the plugin's folders, as plain files with exactly
/// these names.
const INERT: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

/// `text` with Windows line endings (`\r\n`) as Unix ones.
pub(super) fn lf(text: &str) -> Cow<'_, str> {
    if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// [`lf`] for bytes that may not be text.
fn lf_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for (i, &b) in bytes.iter().enumerate() {
        if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
            out.push(b);
        }
    }
    out
}

/// How the plugin in a folder differs from the one this app ships.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Difference {
    /// A shipped file is missing or changed, or something is a link or can't be read. Code could run that
    /// the person was never shown, so the plugin can't stay.
    Changed(String),
    /// Every shipped file is there and the same, and the folder also holds files or folders this app
    /// doesn't ship. Switching the plugin off keeps them from running.
    Extra(String),
}

/// How the plugin in `dir` differs from the one this app ships, or `None` if it doesn't: nothing is in
/// it but the [`FOLDERS`], the [`SHIPPED`] files, the [`MARKERS`] and the [`INERT`] files, and every
/// shipped file is there and the same. A folder or an entry that can't be read counts as changed: this
/// app's own install never has one. Only a plugin that is otherwise exactly ours is called [`Difference::Extra`].
pub(super) fn compare(dir: &Path) -> Option<Difference> {
    let found = unexpected(dir, "");
    match &found {
        Some(Found::Odd(path)) => {
            return Some(Difference::Changed(format!(
                "{} has {path}, which is a link or not a plain file",
                dir.display()
            )))
        }
        Some(Found::Unreadable(what)) => {
            return Some(Difference::Changed(format!("{}: {what} couldn't be read", dir.display())))
        }
        _ => {}
    }
    for (file, ours) in SHIPPED {
        let theirs = std::fs::read(dir.join(file)).ok();
        // The icon is compared exactly, size and all; the text files with their line endings made alike.
        let same = theirs.is_some_and(
            |theirs| {
                if file == ICON {
                    theirs == ours
                } else {
                    lf_bytes(&theirs) == lf_bytes(ours)
                }
            },
        );
        if !same {
            return Some(Difference::Changed(format!("{} differs from this app's copy", dir.join(file).display())));
        }
    }
    match found {
        Some(Found::Extra(extra)) => {
            Some(Difference::Extra(format!("{} has {extra}, which this app's plugin doesn't", dir.display())))
        }
        _ => None,
    }
}

/// What [`unexpected`] found, as a path inside the plugin with `/`s (`""` is the plugin's own folder).
enum Found {
    /// A file or folder the plugin doesn't have.
    Extra(String),
    /// A link, or something that is neither a file nor a folder.
    Odd(String),
    /// A folder or entry that couldn't be read, so can't be said to be fine.
    Unreadable(String),
}

/// The worst thing under `dir` (which is `rel` inside the plugin, `""` for its root) that the plugin
/// doesn't have, or that can't be read: a link or an unreadable entry first, else the first extra file
/// or folder. A link is never followed and is never allowed, and nothing is entered but the
/// [`FOLDERS`], so a deep tree or a loop costs nothing. Names are compared exactly: `Hooks` is not `hooks`.
fn unexpected(dir: &Path, rel: &str) -> Option<Found> {
    let here = || if rel.is_empty() { "the plugin's folder".to_string() } else { rel.to_string() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Some(Found::Unreadable(here())) };
    let mut names = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => names.push(entry),
            Err(_) => return Some(Found::Unreadable(here())),
        }
    }
    names.sort_by_key(std::fs::DirEntry::file_name);
    let mut extra = None;
    for entry in names {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let Ok(kind) = entry.file_type() else { return Some(Found::Unreadable(path)) };
        if !kind.is_dir() && !kind.is_file() {
            return Some(Found::Odd(path));
        }
        let ours = if kind.is_dir() {
            FOLDERS.contains(&path.as_str())
        } else {
            SHIPPED.iter().any(|(file, _)| *file == path)
                || INERT.contains(&name.as_str())
                || (rel.is_empty() && MARKERS.contains(&name.as_str()))
        };
        if !ours {
            extra.get_or_insert(path);
        } else if kind.is_dir() {
            match unexpected(&entry.path(), &path) {
                Some(Found::Extra(found)) => {
                    extra.get_or_insert(found);
                }
                Some(worse) => return Some(worse),
                None => {}
            }
        }
    }
    extra.map(Found::Extra)
}
