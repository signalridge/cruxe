use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub(crate) struct IndexLaunchRequest<'a> {
    pub workspace: &'a Path,
    pub force: bool,
    pub ref_name: Option<&'a str>,
    pub config_path: Option<&'a Path>,
    pub project_id: Option<&'a str>,
    pub storage_data_dir: Option<&'a str>,
    pub job_id: Option<&'a str>,
}

pub(crate) fn spawn_index_process(request: &IndexLaunchRequest<'_>) -> std::io::Result<Child> {
    let mut cmd = build_index_command(request);
    cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn()
}

fn build_index_command(request: &IndexLaunchRequest<'_>) -> Command {
    let mut cmd = Command::new(resolve_index_binary());
    cmd.arg("index")
        .arg("--path")
        .arg(request.workspace.to_string_lossy().as_ref());

    if request.force {
        cmd.arg("--force");
    }
    if let Some(ref_name) = request.ref_name {
        cmd.arg("--ref").arg(ref_name);
    }
    if let Some(config_path) = request.config_path {
        cmd.arg("--config").arg(config_path);
    }
    if let Some(project_id) = request.project_id {
        cmd.env("CRUXE_PROJECT_ID", project_id);
    }
    if let Some(storage_data_dir) = request.storage_data_dir {
        cmd.env("CRUXE_STORAGE_DATA_DIR", storage_data_dir);
    }
    if let Some(job_id) = request.job_id {
        cmd.env("CRUXE_JOB_ID", job_id);
    }

    cmd
}

pub(crate) fn resolve_index_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CRUXE_INDEX_BIN") {
        return PathBuf::from(path);
    }

    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            let mut base = parent.to_path_buf();
            if base
                .file_name()
                .is_some_and(|name| name == std::ffi::OsStr::new("deps"))
            {
                let _ = base.pop();
            }
            let candidate_name = if cfg!(windows) { "cruxe.exe" } else { "cruxe" };
            let candidate = base.join(candidate_name);
            if candidate.exists() {
                return candidate;
            }
        }
        return current;
    }

    PathBuf::from("cruxe")
}

pub(crate) fn generate_job_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
