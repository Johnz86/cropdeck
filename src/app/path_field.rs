use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use directories::UserDirs;

use crate::filesystem::PathFacts;
use crate::image_io::is_supported_image;

pub(super) const PATH_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PathCommit {
    Clear,
    Use(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PathRole {
    Source,
    Destination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PathFieldState {
    Empty,
    Checking,
    Accepted { note: String, path: PathBuf },
    Rejected { reason: String },
}

impl PathFieldState {
    pub(super) fn note(&self) -> &str {
        match self {
            Self::Empty => "source folder",
            Self::Checking => "checking...",
            Self::Accepted { note, .. } => note,
            Self::Rejected { reason } => reason,
        }
    }

    pub(super) const fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }
}

#[derive(Debug, Default)]
pub(super) struct PathField {
    draft: String,
    state: Option<PathFieldState>,
    edited_at: Option<Instant>,
    probing: Option<PathBuf>,
    parent_probe: Option<PathBuf>,
    pending_commit: bool,
}

impl PathField {
    pub(super) fn from_committed(path: Option<&Path>) -> Self {
        let mut field = Self::default();
        field.set_committed(path);
        field
    }

    pub(super) fn set_committed(&mut self, path: Option<&Path>) {
        self.draft = path
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.state = None;
        self.edited_at = None;
        self.probing = None;
        self.parent_probe = None;
        self.pending_commit = false;
    }

    pub(super) fn draft_mut(&mut self) -> &mut String {
        &mut self.draft
    }

    pub(super) fn state(&self) -> PathFieldState {
        self.state
            .clone()
            .unwrap_or(if self.draft.trim().is_empty() {
                PathFieldState::Empty
            } else {
                PathFieldState::Checking
            })
    }

    pub(super) fn mark_edited(&mut self, now: Instant) {
        self.state = None;
        self.edited_at = Some(now);
        self.probing = None;
        self.parent_probe = None;
        self.pending_commit = false;
    }

    pub(super) fn submit_now(&mut self) {
        self.state = None;
        self.edited_at = Some(Instant::now() - PATH_DEBOUNCE);
        self.probing = None;
        self.parent_probe = None;
    }

    pub(super) fn request_commit(&mut self) {
        self.submit_now();
        self.pending_commit = true;
    }

    pub(super) fn take_commit(&mut self) -> Option<PathCommit> {
        if !self.pending_commit {
            return None;
        }
        match self.state.as_ref() {
            Some(PathFieldState::Accepted { path, .. }) => {
                self.pending_commit = false;
                Some(PathCommit::Use(path.clone()))
            }
            Some(PathFieldState::Empty) => {
                self.pending_commit = false;
                Some(PathCommit::Clear)
            }
            Some(PathFieldState::Rejected { .. }) => {
                self.pending_commit = false;
                None
            }
            Some(PathFieldState::Checking) | None => None,
        }
    }

    pub(super) fn due_probe(&mut self, now: Instant) -> Option<PathBuf> {
        let edited_at = self.edited_at?;
        if now.duration_since(edited_at) < PATH_DEBOUNCE {
            return None;
        }
        self.edited_at = None;
        let Some(path) = normalise_path_input(&self.draft) else {
            self.state = Some(PathFieldState::Empty);
            return None;
        };
        self.probing = Some(path.clone());
        Some(path)
    }

    pub(super) fn apply_probe(
        &mut self,
        path: &Path,
        facts: PathFacts,
        role: PathRole,
    ) -> Option<PathBuf> {
        if self.parent_probe.as_deref() == Some(path) {
            let target = self.probing.clone()?;
            self.state = Some(parent_outcome(&target, facts));
            self.parent_probe = None;
            return None;
        }
        if self.probing.as_deref() != Some(path) {
            return None;
        }
        match role {
            PathRole::Source => {
                self.state = Some(source_outcome(path, facts));
                None
            }
            PathRole::Destination => match destination_outcome(path, facts) {
                DestinationOutcome::Resolved(state) => {
                    self.state = Some(state);
                    None
                }
                DestinationOutcome::NeedsParent(parent) => {
                    self.parent_probe = Some(parent.clone());
                    Some(parent)
                }
            },
        }
    }
}

enum DestinationOutcome {
    Resolved(PathFieldState),
    NeedsParent(PathBuf),
}

pub(super) fn normalise_path_input(input: &str) -> Option<PathBuf> {
    let trimmed = input.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(trimmed)
        .trim();
    if unquoted.is_empty() {
        return None;
    }
    if unquoted == "~" {
        return home_directory();
    }
    if let Some(rest) = unquoted.strip_prefix("~/") {
        return home_directory().map(|home| home.join(rest));
    }
    Some(PathBuf::from(unquoted))
}

fn home_directory() -> Option<PathBuf> {
    UserDirs::new().map(|directories| directories.home_dir().to_path_buf())
}

fn source_outcome(path: &Path, facts: PathFacts) -> PathFieldState {
    if !facts.exists {
        return PathFieldState::Rejected {
            reason: String::from("not found"),
        };
    }
    if facts.is_directory {
        return PathFieldState::Accepted {
            note: String::from("folder"),
            path: path.to_path_buf(),
        };
    }
    if is_supported_image(path) {
        return PathFieldState::Accepted {
            note: String::from("image"),
            path: path.to_path_buf(),
        };
    }
    PathFieldState::Rejected {
        reason: String::from("not a JPEG, PNG, or WebP image"),
    }
}

fn destination_outcome(path: &Path, facts: PathFacts) -> DestinationOutcome {
    if !path.is_absolute() {
        return DestinationOutcome::Resolved(PathFieldState::Rejected {
            reason: String::from("enter an absolute path"),
        });
    }
    if facts.exists && facts.is_directory {
        return DestinationOutcome::Resolved(PathFieldState::Accepted {
            note: String::from("ready"),
            path: path.to_path_buf(),
        });
    }
    if facts.exists {
        return DestinationOutcome::Resolved(PathFieldState::Rejected {
            reason: String::from("a file already exists at this path"),
        });
    }
    match path.parent() {
        Some(parent) => DestinationOutcome::NeedsParent(parent.to_path_buf()),
        None => DestinationOutcome::Resolved(PathFieldState::Rejected {
            reason: String::from("parent folder does not exist"),
        }),
    }
}

fn parent_outcome(target: &Path, parent_facts: PathFacts) -> PathFieldState {
    if parent_facts.exists && parent_facts.is_directory {
        return PathFieldState::Accepted {
            note: String::from("will be created on first capture"),
            path: target.to_path_buf(),
        };
    }
    PathFieldState::Rejected {
        reason: String::from("parent folder does not exist"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute(relative: &str) -> PathBuf {
        std::env::temp_dir().join(relative)
    }

    fn facts(exists: bool, is_directory: bool) -> PathFacts {
        PathFacts {
            exists,
            is_directory,
        }
    }

    #[test]
    fn normalisation_trims_whitespace_and_matched_quotes() {
        assert_eq!(
            normalise_path_input("  /tmp/chapter 41  "),
            Some(PathBuf::from("/tmp/chapter 41"))
        );
        assert_eq!(
            normalise_path_input("'/tmp/chapter 41'"),
            Some(PathBuf::from("/tmp/chapter 41"))
        );
        assert_eq!(
            normalise_path_input("\"/tmp/chapter 41\""),
            Some(PathBuf::from("/tmp/chapter 41"))
        );
    }

    #[test]
    fn normalisation_rejects_blank_input() {
        assert_eq!(normalise_path_input(""), None);
        assert_eq!(normalise_path_input("   "), None);
        assert_eq!(normalise_path_input("''"), None);
    }

    #[test]
    fn normalisation_keeps_unmatched_quotes_verbatim() {
        assert_eq!(
            normalise_path_input("/tmp/it's"),
            Some(PathBuf::from("/tmp/it's"))
        );
    }

    #[test]
    fn normalisation_expands_a_leading_home_shorthand() {
        let Some(home) = home_directory() else {
            return;
        };

        assert_eq!(normalise_path_input("~"), Some(home.clone()));
        assert_eq!(
            normalise_path_input("~/exports"),
            Some(home.join("exports"))
        );
    }

    #[test]
    fn source_accepts_folders_and_supported_images() {
        assert!(matches!(
            source_outcome(Path::new("/tmp/chapter"), facts(true, true)),
            PathFieldState::Accepted { .. }
        ));
        assert!(matches!(
            source_outcome(Path::new("/tmp/page1.webp"), facts(true, false)),
            PathFieldState::Accepted { .. }
        ));
    }

    #[test]
    fn source_rejects_missing_paths_and_unsupported_files() {
        assert!(source_outcome(Path::new("/tmp/absent"), facts(false, false)).is_rejected());
        assert!(source_outcome(Path::new("/tmp/notes.txt"), facts(true, false)).is_rejected());
    }

    #[test]
    fn destination_accepts_an_existing_directory() {
        assert!(matches!(
            destination_outcome(&absolute("exports"), facts(true, true)),
            DestinationOutcome::Resolved(PathFieldState::Accepted { .. })
        ));
    }

    #[test]
    fn destination_rejects_an_existing_file_and_relative_paths() {
        assert!(matches!(
            destination_outcome(&absolute("exports.txt"), facts(true, false)),
            DestinationOutcome::Resolved(state)
                if state.note() == "a file already exists at this path"
        ));
        assert!(matches!(
            destination_outcome(Path::new("exports"), facts(false, false)),
            DestinationOutcome::Resolved(state) if state.is_rejected()
        ));
    }

    #[test]
    fn destination_defers_to_its_parent_when_missing() {
        assert!(matches!(
            destination_outcome(&absolute("exports/chapter"), facts(false, false)),
            DestinationOutcome::NeedsParent(parent) if parent == absolute("exports")
        ));
    }

    #[test]
    fn a_missing_destination_with_an_existing_parent_is_accepted() {
        let state = parent_outcome(Path::new("/tmp/exports/chapter"), facts(true, true));

        assert!(matches!(state, PathFieldState::Accepted { .. }));
        assert_eq!(state.note(), "will be created on first capture");
    }

    #[test]
    fn a_missing_destination_parent_is_rejected() {
        assert!(parent_outcome(Path::new("/a/b/c"), facts(false, false)).is_rejected());
    }

    #[test]
    fn probes_are_ignored_once_the_draft_moves_on() {
        let mut field = PathField::default();
        field.draft_mut().push_str("/tmp/first");
        field.submit_now();
        let probed = field
            .due_probe(Instant::now())
            .expect("the debounced draft should be probed");
        field.draft_mut().push_str("-edited");
        field.mark_edited(Instant::now());

        let parent = field.apply_probe(&probed, facts(true, true), PathRole::Source);

        assert!(parent.is_none());
        assert_eq!(field.state(), PathFieldState::Checking);
    }

    #[test]
    fn a_commit_waits_for_validation_and_never_fires_for_a_rejected_path() {
        let mut field = PathField::default();
        field.draft_mut().push_str("/tmp/absent");
        field.request_commit();
        let probed = field
            .due_probe(Instant::now())
            .expect("the draft should be probed");

        assert_eq!(field.take_commit(), None);
        field.apply_probe(&probed, facts(false, false), PathRole::Source);
        assert_eq!(field.take_commit(), None);
        assert!(field.state().is_rejected());
    }

    #[test]
    fn a_commit_yields_the_accepted_path() {
        let mut field = PathField::default();
        field.draft_mut().push_str("/tmp/chapter");
        field.request_commit();
        let probed = field
            .due_probe(Instant::now())
            .expect("the draft should be probed");
        field.apply_probe(&probed, facts(true, true), PathRole::Source);

        assert_eq!(
            field.take_commit(),
            Some(PathCommit::Use(PathBuf::from("/tmp/chapter")))
        );
        assert_eq!(field.take_commit(), None);
    }

    #[test]
    fn an_empty_draft_commits_a_clear() {
        let mut field = PathField::from_committed(Some(Path::new("/tmp/exports")));
        field.draft_mut().clear();
        field.request_commit();

        assert!(field.due_probe(Instant::now()).is_none());
        assert_eq!(field.take_commit(), Some(PathCommit::Clear));
    }

    #[test]
    fn the_debounce_defers_a_probe_until_it_elapses() {
        let mut field = PathField::default();
        field.draft_mut().push_str("/tmp/chapter");
        let now = Instant::now();
        field.mark_edited(now);

        assert!(field.due_probe(now).is_none());
        assert_eq!(
            field.due_probe(now + PATH_DEBOUNCE),
            Some(PathBuf::from("/tmp/chapter"))
        );
    }
}
