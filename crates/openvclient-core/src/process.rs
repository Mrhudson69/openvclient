//! Running external command-line tools (`pg_dump`, `mysqldump`, `az` …).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// A command for a console tool.
///
/// The Windows release build has no console (`windows_subsystem = "windows"`),
/// so every console program it starts would flash a new console window unless
/// it is created with `CREATE_NO_WINDOW`. Output is still captured as usual.
pub fn command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `<base>\<prefix><version>\bin` folders, newest version first.
///
/// Windows installers put client tools in versioned folders such as
/// `C:\Program Files\PostgreSQL\17\bin` or
/// `C:\Program Files\MySQL\MySQL Server 8.4\bin` without adding them to `PATH`.
pub fn versioned_bin_dirs(base: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(base) else { return Vec::new() };
    let mut found: Vec<(Vec<u64>, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let version = name.strip_prefix(prefix)?;
            let key: Vec<u64> = version.split(['.', '-', ' ']).map_while(|p| p.parse().ok()).collect();
            let bin = e.path().join("bin");
            (!key.is_empty() && bin.is_dir()).then_some((key, bin))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, p)| p).collect()
}

/// The Program Files folders (64-bit first) on Windows; empty elsewhere.
pub fn program_files() -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut v: Vec<PathBuf> = Vec::new();
    for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(p) = std::env::var_os(var).map(PathBuf::from)
            && !v.contains(&p)
        {
            v.push(p);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_dirs_newest_first() {
        let root = std::env::temp_dir().join(format!("ovc-verdirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["MySQL Server 8.0", "MySQL Server 8.4", "MySQL Server 10.1", "MySQL Workbench 8.0", "MySQL Server x"] {
            std::fs::create_dir_all(root.join(d).join("bin")).unwrap();
        }
        // A version folder without bin\ is not a candidate.
        std::fs::create_dir_all(root.join("MySQL Server 9.0")).unwrap();
        let got: Vec<String> = versioned_bin_dirs(&root, "MySQL Server ").iter().map(|p| p.parent().unwrap().file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(got, ["MySQL Server 10.1", "MySQL Server 8.4", "MySQL Server 8.0"]);
        assert!(versioned_bin_dirs(&root.join("missing"), "").is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
