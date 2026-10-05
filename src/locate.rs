use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use prism::driver::PRELUDE;
use prism::stdlib::STDLIB;
use prism::{Root, default_roots, project_roots};

use crate::outline::{Item, lookup, outline};

const MANIFEST: &str = "prism.toml";
const DEFAULT_SRC: &str = "src";
const PRELUDE_FILE: &str = "prelude.pr";
const EXT: &str = "pr";

/// The module search path for a file: its project's, or its own directory's.
pub fn roots_for(file: &Path) -> Vec<Root> {
    let dir = file.parent().unwrap_or(Path::new("."));
    match dir.ancestors().find(|d| d.join(MANIFEST).is_file()) {
        Some(proj) => {
            let manifest = read_manifest(proj);
            let deps = manifest
                .as_ref()
                .and_then(|m| m.get("dependencies")?.as_table().cloned())
                .unwrap_or_default()
                .values()
                .filter_map(|d| d.get("path")?.as_str().map(|p| proj.join(p)))
                .map(|d| src_dir(&d))
                .collect::<Vec<_>>();
            project_roots(&src_dir(proj), &deps)
        }
        None => default_roots(dir),
    }
}

fn read_manifest(proj: &Path) -> Option<toml::Table> {
    std::fs::read_to_string(proj.join(MANIFEST))
        .ok()?
        .parse()
        .ok()
}

fn src_dir(proj: &Path) -> PathBuf {
    let src = read_manifest(proj)
        .and_then(|m| m.get("package")?.get("src")?.as_str().map(str::to_string))
        .unwrap_or_else(|| DEFAULT_SRC.to_string());
    proj.join(src)
}

/// Finds the file and range that define a canonical name.
pub struct Locator {
    stdlib: Option<PathBuf>,
    outlines: HashMap<PathBuf, Vec<Item>>,
}

impl Locator {
    pub fn new() -> Self {
        Self {
            stdlib: materialize_stdlib(),
            outlines: HashMap::new(),
        }
    }

    /// Where `target` is defined: the current document first for a bare name,
    /// then the prelude; a qualified name in its module's file.
    pub fn define(
        &mut self,
        target: &str,
        roots: &[Root],
        here: &[Item],
    ) -> Option<(Option<PathBuf>, (usize, usize))> {
        match split(target) {
            None => lookup(here, target).map(|r| (None, r)).or_else(|| {
                let prelude = self.stdlib.as_ref()?.join(PRELUDE_FILE);
                self.find_in(prelude, target)
            }),
            Some((module, name)) => {
                let file = self.module_file(module, roots)?;
                self.find_in(file, name)
            }
        }
    }

    fn find_in(&mut self, file: PathBuf, name: &str) -> Option<(Option<PathBuf>, (usize, usize))> {
        if !self.outlines.contains_key(&file) {
            let items = outline(&std::fs::read_to_string(&file).ok()?).unwrap_or_default();
            self.outlines.insert(file.clone(), items);
        }
        let range = lookup(&self.outlines[&file], name)?;
        Some((Some(file), range))
    }

    fn module_file(&self, module: &str, roots: &[Root]) -> Option<PathBuf> {
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
}

// `Data.List.foldr` -> (`Data.List`, `foldr`); `Data.List@go` -> (`Data.List`, `go`).
fn split(target: &str) -> Option<(&str, &str)> {
    target.split_once('@').or_else(|| target.rsplit_once('.'))
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
