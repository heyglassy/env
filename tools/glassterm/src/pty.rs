use std::{
    cell::RefCell,
    env,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process,
    rc::Rc,
    sync::mpsc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::config::Config;

pub(crate) struct PtySession {
    master: Box<dyn portable_pty::MasterPty + Send>,
    pub(crate) writer: Rc<RefCell<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    pub(crate) output_rx: mpsc::Receiver<Vec<u8>>,
    _shell_integration: ShellIntegration,
}

impl PtySession {
    pub(crate) fn spawn(command: &[String], cols: u16, rows: u16, config: &Config) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let program = command
            .first()
            .ok_or_else(|| anyhow!("missing command to spawn"))?;
        let shell_integration = ShellIntegration::new(command, config.shell_integration)?;
        let mut builder = CommandBuilder::new(shell_integration.program().unwrap_or(program));
        for arg in shell_integration.args() {
            builder.arg(arg);
        }
        for arg in command.iter().skip(1) {
            builder.arg(arg);
        }
        if let Ok(cwd) = env::current_dir() {
            builder.cwd(&cwd);
            builder.env("PWD", cwd.as_os_str());
        }
        builder.env("TERM", &config.child_term);
        builder.env("GLASSTERM", "1");
        if let Some(terminfo_dirs) = existing_terminfo_dirs() {
            builder.env("TERMINFO_DIRS", terminfo_dirs);
        }
        for (key, value) in shell_integration.envs() {
            builder.env(key, value);
        }

        let child = pair
            .slave
            .spawn_command(builder)
            .with_context(|| format!("failed to spawn {:?}", command))?;
        drop(pair.slave);

        let writer = Rc::new(RefCell::new(pair.master.take_writer()?));
        let reader = pair.master.try_clone_reader()?;
        let (output_tx, output_rx) = mpsc::channel();

        thread::Builder::new()
            .name("glassterm-pty-reader".to_string())
            .spawn(move || read_pty(reader, output_tx))
            .context("failed to spawn PTY reader")?;

        Ok(Self {
            master: pair.master,
            writer,
            child,
            output_rx,
            _shell_integration: shell_integration,
        })
    }

    pub(crate) fn write_all(&self, bytes: &[u8]) -> Result<()> {
        let mut writer = self.writer.borrow_mut();
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    pub(crate) fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub(crate) fn child_is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }
}

#[derive(Default)]
struct ShellIntegration {
    program: Option<String>,
    envs: Vec<(String, String)>,
    args: Vec<String>,
    cleanup: Vec<PathBuf>,
}

impl ShellIntegration {
    fn new(command: &[String], enabled: bool) -> Result<Self> {
        if !enabled || command.len() != 1 {
            return Ok(Self::default());
        }

        let Some(program) = command.first() else {
            return Ok(Self::default());
        };
        let Some(shell) = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
        else {
            return Ok(Self::default());
        };

        match shell {
            "sh" | "dash" | "ksh" => Self::for_posix_shell(program),
            "bash" => Self::for_bash(),
            "zsh" => Self::for_zsh(program),
            _ => Ok(Self::default()),
        }
    }

    fn for_posix_shell(program: &str) -> Result<Self> {
        let script = write_temp_file("sh", shell_integration_script(false))?;
        let wrapper = write_temp_file(
            "sh-wrapper",
            format!(
                "ENV={}\nexport ENV\nexec {} -i\n",
                shell_quote(&script.display().to_string()),
                shell_quote(program)
            ),
        )?;
        Ok(Self {
            program: Some("/bin/sh".to_string()),
            envs: Vec::new(),
            args: vec![wrapper.display().to_string()],
            cleanup: vec![script, wrapper],
        })
    }

    fn for_bash() -> Result<Self> {
        let script = write_temp_file("bash", shell_integration_script(true))?;
        let wrapper = write_temp_file(
            "bash-wrapper",
            format!(
                "exec bash --rcfile {} -i\n",
                shell_quote(&script.display().to_string())
            ),
        )?;
        Ok(Self {
            program: Some("/bin/sh".to_string()),
            envs: Vec::new(),
            args: vec![wrapper.display().to_string()],
            cleanup: vec![script, wrapper],
        })
    }

    fn for_zsh(program: &str) -> Result<Self> {
        let dir = temp_path("zsh");
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let script = dir.join(".zshrc");
        fs::write(&script, shell_integration_script(true))
            .with_context(|| format!("write {}", script.display()))?;
        let wrapper = write_temp_file(
            "zsh-wrapper",
            format!(
                "ZDOTDIR={}\nexport ZDOTDIR\nexec {} -i\n",
                shell_quote(&dir.display().to_string()),
                shell_quote(program)
            ),
        )?;
        Ok(Self {
            program: Some("/bin/sh".to_string()),
            envs: Vec::new(),
            args: vec![wrapper.display().to_string()],
            cleanup: vec![dir, wrapper],
        })
    }

    fn program(&self) -> Option<&str> {
        self.program.as_deref()
    }

    fn envs(&self) -> &[(String, String)] {
        &self.envs
    }

    fn args(&self) -> &[String] {
        &self.args
    }
}

impl Drop for ShellIntegration {
    fn drop(&mut self) {
        for path in &self.cleanup {
            if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            } else {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn write_temp_file(kind: &str, contents: String) -> Result<PathBuf> {
    let path = temp_path(kind);
    fs::write(&path, contents).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

fn temp_path(kind: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    env::temp_dir().join(format!("glassterm-{kind}-{}-{nanos}", process::id()))
}

fn shell_integration_script(source_user_rc: bool) -> String {
    let mut script = String::new();
    if source_user_rc {
        script.push_str(
            r#"
if [ -n "${BASH_VERSION:-}" ] && [ -r "$HOME/.bashrc" ]; then
  . "$HOME/.bashrc"
elif [ -n "${ZSH_VERSION:-}" ] && [ -r "$HOME/.zshrc" ]; then
  . "$HOME/.zshrc"
fi
"#,
        );
    }
    script.push_str(
        r#"
__glassterm_emit_pwd() {
  printf '\033]7;file://localhost%s\033\\' "$PWD"
}
cd() {
  command cd "$@" && __glassterm_emit_pwd
}
__glassterm_emit_pwd
"#,
    );
    script
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn read_pty(mut reader: Box<dyn Read + Send>, output_tx: mpsc::Sender<Vec<u8>>) {
    let mut buf = [0_u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if output_tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        }
    }
}

fn existing_terminfo_dirs() -> Option<String> {
    existing_terminfo_dirs_from(env::var_os("TERMINFO_DIRS")?)
}

fn existing_terminfo_dirs_from(dirs: OsString) -> Option<String> {
    let existing = env::split_paths(&dirs)
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

    if existing.is_empty() {
        Some(String::new())
    } else {
        env::join_paths(existing)
            .ok()
            .map(|paths| paths.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminfo_dirs_filter_drops_missing_entries() {
        let existing = tempfile::tempdir().unwrap();
        let missing = existing.path().join("missing");
        let dirs = env::join_paths([missing.as_path(), existing.path()]).unwrap();

        assert_eq!(
            existing_terminfo_dirs_from(dirs),
            Some(existing.path().to_string_lossy().into_owned())
        );
    }
}
