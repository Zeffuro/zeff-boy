use std::path::{Path, PathBuf};

pub(crate) fn resolve_repo_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("ZEFF_REPO_ROOT")
        .map(PathBuf::from)
        .filter(|path| repo_root_is_valid(path))
    {
        return Some(root);
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(root) = manifest_dir
        .parent()
        .and_then(Path::parent)
        .filter(|path| repo_root_is_valid(path))
    {
        return Some(root.to_path_buf());
    }

    if let Ok(current_dir) = std::env::current_dir()
        && let Some(root) = find_repo_root_ancestor(&current_dir)
    {
        return Some(root);
    }

    if let Ok(current_exe) = std::env::current_exe()
        && let Some(parent) = current_exe.parent()
        && let Some(root) = find_repo_root_ancestor(parent)
    {
        return Some(root);
    }

    None
}

fn find_repo_root_ancestor(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|path| repo_root_is_valid(path))
        .map(Path::to_path_buf)
}

pub(crate) fn repo_root_is_valid(path: &Path) -> bool {
    path.join("Cargo.toml").is_file() && path.join("crates").join("zeff-mcp").is_dir()
}
