use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use prism::driver::PRELUDE;
use prism::flags::DynFlags;
use prism::stdlib::STDLIB;
use prism::{Root, SearchPath, default_roots};

const PRELUDE_FILE: &str = "prelude.pr";
const EXT: &str = "pr";

/// The module search path and prelude for a file, resolved the way `prism check`
/// resolves them. A manifest that does not load leaves the file standalone.
pub fn search_for(file: &Path) -> SearchPath {
    prism::search_path(file, &DynFlags::from_env()).unwrap_or_else(|_| SearchPath {
        project: None,
        roots: default_roots(file.parent().unwrap_or(Path::new("."))),
        prelude: None,
    })
}

/// The files definitions are written in, the embedded ones included.
pub struct Files {
    stdlib: Option<PathBuf>,
}

impl Files {
    pub fn new() -> Self {
        Self {
            stdlib: materialize_stdlib(),
        }
    }

    /// The file a module's source is read from.
    pub fn module(&self, module: &str, roots: &[Root]) -> Option<PathBuf> {
        let rel = Path::new(&module.replace('.', "/")).with_extension(EXT);
        roots.iter().find_map(|root| match root {
            Root::Dir(dir) => Some(dir.join(&rel)).filter(|p| p.is_file()),
            Root::Embedded(table) => table
                .iter()
                .any(|(m, _)| *m == module)
                .then(|| self.stdlib.as_ref().map(|d| d.join(&rel)))
                .flatten(),
            Root::SourceBundle { .. } => None,
        })
    }

    /// The prelude a file is checked against: its project's own, or the embedded one.
    pub fn prelude(&self, search: &SearchPath) -> Option<PathBuf> {
        match search.project.as_ref().and_then(|p| p.prelude.clone()) {
            Some(own) => Some(own),
            None => Some(self.stdlib.as_ref()?.join(PRELUDE_FILE)),
        }
    }
}

// The embedded stdlib and prelude written to a cache directory, so goto-definition
// has real files to open. Keyed by content, so a new compiler gets a fresh copy.
fn materialize_stdlib() -> Option<PathBuf> {
    let mut h = DefaultHasher::new();
    (PRELUDE, STDLIB).hash(&mut h);
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cache")))?;
    let dir = base.join("prism-lsp").join(format!("{:016x}", h.finish()));
    let files = STDLIB
        .iter()
        .map(|(m, src)| (Path::new(&m.replace('.', "/")).with_extension(EXT), *src))
        .chain(std::iter::once((PathBuf::from(PRELUDE_FILE), PRELUDE)));
    for (rel, src) in files {
        let path = dir.join(rel);
        if !path.is_file() {
            std::fs::create_dir_all(path.parent()?).ok()?;
            std::fs::write(&path, src).ok()?;
        }
    }
    Some(dir)
}
