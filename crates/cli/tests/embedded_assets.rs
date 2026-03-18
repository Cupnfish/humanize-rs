use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn collect_files(root: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    collect_into(root, root, &mut files);
    files
}

fn collect_into(base: &Path, current: &Path, out: &mut BTreeMap<String, String>) {
    let entries = fs::read_dir(current).unwrap();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_into(base, &path, out);
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("failed to read {}", path.display()));
            out.insert(rel, content);
        }
    }
}

#[test]
fn embedded_prompt_templates_match_repo_assets() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");
    let asset_root = manifest_dir.join("assets");

    let repo_dir = repo_root.join("prompt-template");
    let asset_dir = asset_root.join("prompt-template");

    assert!(
        repo_dir.is_dir(),
        "missing repo asset dir: {}",
        repo_dir.display()
    );
    assert!(
        asset_dir.is_dir(),
        "missing embedded asset dir: {}",
        asset_dir.display()
    );

    let repo_files = collect_files(&repo_dir);
    let asset_files = collect_files(&asset_dir);
    assert_eq!(
        repo_files, asset_files,
        "embedded prompt templates drifted from repo source"
    );
}
