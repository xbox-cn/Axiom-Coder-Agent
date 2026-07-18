use crate::models::{
    FileEntry, FileMutation, GitFileChange, GitSummary, PermissionMode, SearchMatch, ShellResult,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Instant,
};
use tokio::process::Command;

#[cfg(windows)]
pub(crate) mod process_tree {
    use std::{ffi::c_void, mem::size_of, ptr::null};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        },
    };

    pub struct KillOnDropJob(isize);

    impl KillOnDropJob {
        pub fn attach(process_id: u32) -> Result<Self, String> {
            unsafe {
                let job = CreateJobObjectW(null(), null());
                if job.is_null() {
                    return Err(format!(
                        "无法创建 Windows Job Object: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) == 0
                {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(format!("无法配置 Windows Job Object: {error}"));
                }
                let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id);
                if process.is_null() {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(format!("无法打开 Shell 进程: {error}"));
                }
                let assigned = AssignProcessToJobObject(job, process);
                CloseHandle(process);
                if assigned == 0 {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(format!("无法将 Shell 进程加入 Job Object: {error}"));
                }
                Ok(Self(job as isize))
            }
        }
    }

    impl Drop for KillOnDropJob {
        fn drop(&mut self) {
            unsafe {
                if self.0 != 0 {
                    CloseHandle(self.0 as HANDLE);
                }
            }
        }
    }
}

const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

pub fn guard_path(root: &Path, requested: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("无法解析工作区: {e}"))?;
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let resolved = if absolute.exists() {
        absolute
            .canonicalize()
            .map_err(|e| format!("无法解析路径: {e}"))?
    } else {
        let parent = absolute
            .parent()
            .ok_or_else(|| "路径没有父目录".to_string())?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|e| format!("无法解析父目录: {e}"))?;
        canonical_parent.join(
            absolute
                .file_name()
                .ok_or_else(|| "无效文件名".to_string())?,
        )
    };
    if !path_starts_with(&resolved, &root) {
        return Err("操作路径超出工作区，必须经过单独审批".to_string());
    }
    Ok(resolved)
}

fn path_starts_with(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let p = path.to_string_lossy().replace('/', "\\").to_lowercase();
        let mut r = root.to_string_lossy().replace('/', "\\").to_lowercase();
        if !r.ends_with('\\') {
            r.push('\\');
        }
        return p == r.trim_end_matches('\\') || p.starts_with(&r);
    }
    #[cfg(not(windows))]
    path.starts_with(root)
}

pub fn list_files(root: &Path, requested: Option<&str>) -> Result<Vec<FileEntry>, String> {
    let dir = guard_path(root, Path::new(requested.unwrap_or(".")))?;
    if !dir.is_dir() {
        return Err("目标不是目录".to_string());
    }
    let mut items = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | "node_modules" | "target" | "dist") {
            continue;
        }
        items.push(FileEntry {
            name,
            path: entry.path().to_string_lossy().to_string(),
            is_directory: metadata.is_dir(),
            size: metadata.len(),
        });
    }
    items.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(items)
}

pub fn read_file(root: &Path, requested: &str) -> Result<String, String> {
    let path = guard_path(root, Path::new(requested))?;
    let metadata = path.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("目标不是文件".to_string());
    }
    if metadata.len() as usize > MAX_TEXT_BYTES {
        return Err("文件超过 2MB，MVP 只预览较小文本文件".to_string());
    }
    std::fs::read_to_string(path).map_err(|_| "文件不是有效 UTF-8 文本".to_string())
}

pub fn search_files(
    root: &Path,
    query: &str,
    requested: Option<&str>,
) -> Result<Vec<SearchMatch>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let start = guard_path(root, Path::new(requested.unwrap_or(".")))?;
    let needle = query.to_lowercase();
    let mut matches = Vec::new();
    let walker = ignore::WalkBuilder::new(start)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true)
        .build();
    for entry in walker.filter_map(Result::ok) {
        if matches.len() >= 200 {
            break;
        }
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() as usize > MAX_TEXT_BYTES {
            continue;
        }
        let Ok(content) = fs::read_to_string(entry.path()) else {
            continue;
        };
        for (line_index, line) in content.lines().enumerate() {
            let lower = line.to_lowercase();
            let Some(column) = lower.find(&needle) else {
                continue;
            };
            matches.push(SearchMatch {
                path: entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or(entry.path())
                    .to_string_lossy()
                    .to_string(),
                line: line_index as u64 + 1,
                column: column as u64 + 1,
                preview: line.chars().take(300).collect(),
            });
            if matches.len() >= 200 {
                break;
            }
        }
    }
    Ok(matches)
}

pub fn write_file(
    root: &Path,
    requested: &str,
    content: &str,
    permission: PermissionMode,
) -> Result<FileMutation, String> {
    if permission == PermissionMode::ReadOnly {
        return Err("Read-only mode blocks file changes".to_string());
    }
    if content.len() > MAX_TEXT_BYTES {
        return Err("File content exceeds the 2 MB safety limit".to_string());
    }
    let path = guard_path(root, Path::new(requested))?;
    if path.exists() && !path.is_file() {
        return Err("Target path is not a file".to_string());
    }
    let before = if path.exists() {
        Some(
            fs::read_to_string(&path)
                .map_err(|_| "Existing file is not valid UTF-8 text".to_string())?,
        )
    } else {
        None
    };
    fs::write(&path, content).map_err(|e| format!("Failed to write file: {e}"))?;
    Ok(FileMutation {
        path: path.to_string_lossy().to_string(),
        before,
        operation: "write".to_string(),
    })
}

pub fn apply_patch(
    root: &Path,
    requested: &str,
    patch_text: &str,
    permission: PermissionMode,
) -> Result<FileMutation, String> {
    if permission == PermissionMode::ReadOnly {
        return Err("Read-only mode blocks file changes".to_string());
    }
    let path = guard_path(root, Path::new(requested))?;
    let before =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read patch target: {e}"))?;
    let patch = diffy::Patch::from_str(patch_text).map_err(|e| format!("Invalid patch: {e}"))?;
    let after =
        diffy::apply(&before, &patch).map_err(|e| format!("Patch could not be applied: {e}"))?;
    write_file(root, requested, &after, permission)
}

pub fn delete_file(
    root: &Path,
    requested: &str,
    permission: PermissionMode,
) -> Result<FileMutation, String> {
    if permission == PermissionMode::ReadOnly {
        return Err("Read-only mode blocks file changes".to_string());
    }
    let path = guard_path(root, Path::new(requested))?;
    if !path.is_file() {
        return Err("Target file does not exist or is not a file".to_string());
    }
    let before =
        fs::read_to_string(&path).map_err(|_| "Target file is not valid UTF-8 text".to_string())?;
    fs::remove_file(&path).map_err(|e| format!("Failed to delete file: {e}"))?;
    Ok(FileMutation {
        path: path.to_string_lossy().to_string(),
        before: Some(before),
        operation: "delete".to_string(),
    })
}

pub fn restore_mutation(root: &Path, mutation: &FileMutation) -> Result<(), String> {
    let path = guard_path(root, Path::new(&mutation.path))?;
    match &mutation.before {
        Some(content) => {
            fs::write(path, content).map_err(|e| format!("Failed to delete file: {e}"))
        }
        None if path.exists() => {
            fs::remove_file(path).map_err(|e| format!("Failed to remove created file: {e}"))
        }
        None => Ok(()),
    }
}

pub async fn git_summary(root: &Path) -> Result<GitSummary, String> {
    let branch = run_git(root, &["branch", "--show-current"])
        .await
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let status_text = run_git(root, &["status", "--porcelain=v1"])
        .await
        .unwrap_or_default();
    let changed_files = status_text
        .lines()
        .filter_map(|line| {
            if line.len() < 3 {
                return None;
            }
            Some(GitFileChange {
                status: line[..2].trim().to_string(),
                path: line[3..].trim().to_string(),
            })
        })
        .collect();
    let mut diff = run_git(root, &["diff", "--no-ext-diff", "--unified=3"])
        .await
        .unwrap_or_default();
    if diff.len() > MAX_TEXT_BYTES {
        diff.truncate(MAX_TEXT_BYTES);
        diff.push_str("\n\n… diff 已截断 …");
    }
    Ok(GitSummary {
        branch,
        changed_files,
        diff,
    })
}

async fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub async fn execute_shell(
    root: &Path,
    command: &str,
    permission: PermissionMode,
    approved: bool,
) -> Result<ShellResult, String> {
    if permission == PermissionMode::ReadOnly && !approved {
        return Err("只读模式下 Shell 命令需要审批".to_string());
    }
    if permission != PermissionMode::FullAccess && shell_requires_approval(command) && !approved {
        return Err("该命令可能联网、越界或修改系统，需要单独审批".to_string());
    }
    let started = Instant::now();
    #[cfg(windows)]
    let mut child = Command::new("powershell");
    #[cfg(windows)]
    child.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        command,
    ]);
    #[cfg(not(windows))]
    let mut child = Command::new("sh");
    #[cfg(not(windows))]
    child.args(["-lc", command]);
    let child = child
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("无法启动 Shell: {error}"))?;
    #[cfg(windows)]
    let _process_tree = process_tree::KillOnDropJob::attach(
        child
            .id()
            .ok_or_else(|| "无法获取 Shell 进程 ID".to_string())?,
    )?;
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| format!("等待 Shell 进程失败: {error}"))?;
    Ok(ShellResult {
        command: command.to_string(),
        cwd: root.to_string_lossy().to_string(),
        exit_code: output.status.code(),
        stdout: truncate(String::from_utf8_lossy(&output.stdout).to_string()),
        stderr: truncate(String::from_utf8_lossy(&output.stderr).to_string()),
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

pub fn shell_requires_approval(command: &str) -> bool {
    let lower = command.to_lowercase();
    [
        "remove-item",
        " rm ",
        "del ",
        "rmdir",
        "format ",
        "diskpart",
        "reg ",
        "regedit",
        "invoke-webrequest",
        "curl ",
        "wget ",
        "ssh ",
        "scp ",
        "start-process",
        "set-executionpolicy",
        "stop-process",
        "taskkill",
        "shutdown",
        "restart-computer",
        "../",
        "..\\",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn truncate(mut value: String) -> String {
    if value.len() > MAX_TEXT_BYTES {
        value.truncate(MAX_TEXT_BYTES);
        value.push_str("\n… 输出已截断 …");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("axiom-tools-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn detects_sensitive_shell_commands() {
        assert!(shell_requires_approval("Remove-Item -Recurse foo"));
        assert!(shell_requires_approval("curl https://example.com"));
        assert!(!shell_requires_approval("pnpm test"));
    }

    #[test]
    fn rejects_parent_traversal_absolute_outside_and_unc_paths() {
        let base = TestDir::new();
        let workspace = base.0.join("workspace");
        let outside = base.0.join("outside.txt");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(&outside, "secret").unwrap();

        assert!(guard_path(&workspace, Path::new("../outside.txt")).is_err());
        assert!(guard_path(&workspace, &outside).is_err());
        #[cfg(windows)]
        assert!(!path_starts_with(
            Path::new(r"\\server\share\outside.txt"),
            &workspace.canonicalize().unwrap()
        ));
    }

    #[cfg(windows)]
    #[test]
    fn accepts_case_insensitive_workspace_paths() {
        let base = TestDir::new();
        let workspace = base.0.join("WorkspaceCase");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("File.txt"), "ok").unwrap();
        let upper_root = PathBuf::from(workspace.to_string_lossy().to_uppercase());
        let resolved = guard_path(&upper_root, Path::new("file.TXT")).unwrap();
        assert!(path_starts_with(
            &resolved,
            &workspace.canonicalize().unwrap()
        ));
    }

    #[cfg(windows)]
    #[test]
    fn rejects_symlink_escape_when_supported() {
        use std::os::windows::fs::symlink_file;
        let base = TestDir::new();
        let workspace = base.0.join("workspace");
        let outside = base.0.join("outside.txt");
        let link = workspace.join("link.txt");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(&outside, "secret").unwrap();
        if symlink_file(&outside, &link).is_err() {
            return;
        }
        assert!(guard_path(&workspace, Path::new("link.txt")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_junction_escape_when_supported() {
        let base = TestDir::new();
        let workspace = base.0.join("workspace");
        let outside = base.0.join("outside");
        let junction = workspace.join("junction");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let status = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .status()
            .expect("run mklink");
        if !status.success() {
            return;
        }
        let result = guard_path(&workspace, Path::new("junction"));
        let _ = fs::remove_dir(&junction);
        assert!(result.is_err());
    }

    #[test]
    fn file_mutations_patch_delete_and_restore_round_trip() {
        let base = TestDir::new();
        let workspace = base.0.join("workspace");
        fs::create_dir_all(&workspace).unwrap();

        let created = write_file(
            &workspace,
            "new.txt",
            "before\n",
            PermissionMode::WorkspaceAuto,
        )
        .unwrap();
        assert_eq!(created.before, None);
        assert!(write_file(&workspace, "blocked.txt", "x", PermissionMode::ReadOnly).is_err());

        let patch = diffy::create_patch("before\n", "after\n").to_string();
        let patched =
            apply_patch(&workspace, "new.txt", &patch, PermissionMode::WorkspaceAuto).unwrap();
        assert_eq!(read_file(&workspace, "new.txt").unwrap(), "after\n");
        restore_mutation(&workspace, &patched).unwrap();
        assert_eq!(read_file(&workspace, "new.txt").unwrap(), "before\n");

        let deleted = delete_file(&workspace, "new.txt", PermissionMode::WorkspaceAuto).unwrap();
        assert!(!workspace.join("new.txt").exists());
        restore_mutation(&workspace, &deleted).unwrap();
        assert_eq!(read_file(&workspace, "new.txt").unwrap(), "before\n");

        restore_mutation(&workspace, &created).unwrap();
        assert!(!workspace.join("new.txt").exists());
    }
}
