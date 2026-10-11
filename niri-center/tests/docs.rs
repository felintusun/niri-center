//! The docs name files; each named file exists (AGENTS.md [maps]).

use std::path::Path;

/// Every backticked repo path in the contributing guide, the README and the
/// decision records: a token with a `/` and a file extension, or naming a
/// folder of the repo.
#[test]
fn every_path_the_docs_name_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut docs = vec![root.join("CONTRIBUTING.md"), root.join("README.md")];
    for entry in std::fs::read_dir(root.join("docs")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "md") {
            docs.push(path);
        }
    }
    let mut missing = Vec::new();
    for doc in &docs {
        let text = std::fs::read_to_string(doc).unwrap();
        for (index, quoted) in text.split('`').enumerate() {
            // odd pieces sit between backticks
            let looks_like_path = index % 2 == 1
                && quoted.contains('/')
                && !quoted.contains(' ')
                && !quoted.starts_with('$')
                && !quoted.starts_with('~')
                && !quoted.contains('*')
                && !quoted.contains("://")
                && (quoted.ends_with(".rs")
                    || quoted.ends_with(".md")
                    || quoted.ends_with(".kdl")
                    || quoted.ends_with(".txt")
                    || quoted.ends_with(".yml")
                    || quoted.ends_with('/'));
            if looks_like_path && !root.join(quoted).exists() {
                missing.push(format!("{}: {quoted}", doc.display()));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "named but missing:\n{}",
        missing.join("\n")
    );
}
