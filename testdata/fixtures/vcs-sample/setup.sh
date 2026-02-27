#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <output-dir>" >&2
  exit 1
fi

OUT_DIR="$1"
REPO_DIR="${OUT_DIR}/vcs-sample"

rm -rf "${REPO_DIR}"
mkdir -p "${REPO_DIR}"
cd "${REPO_DIR}"

git init -b main
git config user.name "Cruxe Fixture"
git config user.email "fixture@cruxe.local"

mkdir -p src
cat > src/lib.rs <<'EOF'
pub fn shared() -> &'static str {
    "main"
}

pub fn keep_me() -> i32 {
    42
}
EOF
git add .
git commit -m "main: initial base"

# feat/add-file: add a new file
git checkout -b feat/add-file
cat > src/add_file.rs <<'EOF'
pub fn added_branch_file() -> &'static str {
    "added"
}
EOF
git add src/add_file.rs
git commit -m "feat/add-file: add src/add_file.rs"

# feat/modify-sig: modify function signature
git checkout main
git checkout -b feat/modify-sig
cat > src/lib.rs <<'EOF'
pub fn shared(mode: &str) -> String {
    format!("main-{mode}")
}

pub fn keep_me() -> i32 {
    42
}
EOF
git add src/lib.rs
git commit -m "feat/modify-sig: change shared signature"

# feat/delete-file: delete an existing file
git checkout main
git checkout -b feat/delete-file
git rm src/lib.rs
git commit -m "feat/delete-file: remove src/lib.rs"

# feat/rename-file: rename file
git checkout main
git checkout -b feat/rename-file
git mv src/lib.rs src/core.rs
git commit -m "feat/rename-file: rename lib.rs -> core.rs"

# feat/rebase-target: create history for ancestry/rebase testing
git checkout main
git checkout -b feat/rebase-target
cat > src/rebase_target.rs <<'EOF'
pub fn rebase_target() -> bool {
    true
}
EOF
git add src/rebase_target.rs
git commit -m "feat/rebase-target: add baseline commit"

# Add one more commit on main so rebase scenarios can be simulated.
git checkout main
cat > src/main_evolves.rs <<'EOF'
pub fn main_evolves() -> &'static str {
    "v2"
}
EOF
git add src/main_evolves.rs
git commit -m "main: evolve history"

echo "Fixture repository created at: ${REPO_DIR}"
echo "Branches:"
git branch --list
