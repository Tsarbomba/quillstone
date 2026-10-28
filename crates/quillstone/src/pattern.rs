//! The files a picture name with `{variable}` parts can stand for (`assets/places/{place}.png`):
//! `check` validates every one of them and `package` ships every one of them.

use std::path::Path;

use quillstone_script::{pattern_matches, pieces};

/// Every file under `root` that `pattern` can build, as game-folder paths with `/`, sorted.
/// A `{variable}` matches one safe file-name fragment inside its own folder (never across
/// `/`). Symbolic links are skipped, so nothing outside the game folder can be reached.
pub fn expand(root: &Path, pattern: &str) -> Vec<String> {
    let Ok(parts) = pieces(pattern) else { return Vec::new() };
    if parts.is_empty() {
        return Vec::new();
    }
    // Folders before the first variable are plain: start in the last of them.
    let head = &pattern[..pattern.find('{').unwrap_or(0)];
    let base = head.rfind('/').map_or("", |i| &head[..i]);
    let mut out = Vec::new();
    walk(root, base, pattern, &mut out);
    out.sort();
    out
}

/// Look in `dir` (relative to `root`), going into folders only while they can still lead to a match.
fn walk(root: &Path, dir: &str, pattern: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(if dir.is_empty() { root.to_path_buf() } else { root.join(dir) }) else { return };
    for entry in entries.filter_map(Result::ok) {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        let Ok(kind) = entry.file_type() else { continue };
        let path = if dir.is_empty() { name } else { format!("{dir}/{name}") };
        if kind.is_file() && pattern_matches(pattern, &path) {
            out.push(path);
        } else if kind.is_dir() && folder_may_match(pattern, &path) {
            walk(root, &path, pattern, out);
        }
    }
}

/// Whether some file below `folder` could match: the folder must match the same number of
/// leading path parts of the pattern.
fn folder_may_match(pattern: &str, folder: &str) -> bool {
    let depth = folder.matches('/').count() + 1;
    // The pattern's leading parts, with variables kept (they hold no `/`, so splitting on `/` is exact).
    let head: Vec<&str> = pattern.split('/').take(depth).collect();
    head.len() == depth && pattern.split('/').count() > depth && pattern_matches(&format!("{}/x.png", head.join("/")), &format!("{folder}/x.png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str, files: &[&str]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("quillstone-pattern-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        root
    }

    #[test]
    fn finds_the_files_in_the_variables_folder_only() {
        let root = tree("one", &["assets/places/a.png", "assets/places/b c.png", "assets/places/x.jpg", "assets/places/sub/d.png", "assets/e.png", "assets/places/..x.png"]);
        assert_eq!(expand(&root, "assets/places/{place}.png"), ["assets/places/a.png", "assets/places/b c.png"]);
        assert_eq!(expand(&root, "assets/places/{place}.jpg"), ["assets/places/x.jpg"]);
        assert!(expand(&root, "assets/nothing/{p}.png").is_empty());
        assert!(expand(&root, "assets/places/{p}.gif").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn variables_may_be_in_folder_names_and_prefixes() {
        let root = tree("two", &["art/mika/face.png", "art/ren/face.png", "art/ren/other.png", "art/x/y/face.png", "m_happy.png", "m_sad.png", "n_happy.png"]);
        assert_eq!(expand(&root, "art/{who}/face.png"), ["art/mika/face.png", "art/ren/face.png"]);
        assert_eq!(expand(&root, "m_{pose}.png"), ["m_happy.png", "m_sad.png"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_not_followed() {
        let root = tree("three", &["assets/places/a.png"]);
        let outside = tree("three-out", &["secret.png"]);
        std::os::unix::fs::symlink(outside.join("secret.png"), root.join("assets/places/link.png")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("assets/places/dir")).unwrap();
        assert_eq!(expand(&root, "assets/places/{p}.png"), ["assets/places/a.png"]);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
