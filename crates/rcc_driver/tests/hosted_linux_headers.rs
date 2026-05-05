#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_os = "linux")]
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "linux")]
struct TempCFile {
    path: PathBuf,
}

#[cfg(target_os = "linux")]
impl TempCFile {
    fn new(name: &str, src: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("rcc-hosted-linux-headers-{}-{id}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(format!("{name}.c"));
        fs::write(&path, src).expect("write temp C source");
        Self { path }
    }

    fn output(&self) -> PathBuf {
        self.path.with_extension("hir")
    }
}

#[cfg(target_os = "linux")]
impl Drop for TempCFile {
    fn drop(&mut self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

#[cfg(target_os = "linux")]
fn rcc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rcc"))
}

#[cfg(target_os = "linux")]
struct Fixture {
    name: &'static str,
    reason: &'static str,
    source: &'static str,
    args: &'static [&'static str],
}

#[cfg(target_os = "linux")]
#[test]
fn hosted_linux_header_gate_lowers_representative_fixtures() {
    let fixtures = [
        Fixture {
            name: "c99-hosted-core",
            reason: "inih, cJSON, Lua, and MuJS exercise stdio/stdlib/string/time declarations",
            args: &[],
            source: r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

int probe(const char *s) {
    char buf[32];
    time_t now = time(0);
    snprintf(buf, sizeof buf, "%ld:%s", (long)now, s ? s : "");
    return atoi(buf) + (strstr(buf, ":") != 0);
}
"#,
        },
        Fixture {
            name: "posix-filesystem",
            reason: "GNU coreutils true/ls/stat family depends on sys/types, stat, fcntl, dirent, and unistd",
            args: &[],
            source: r#"
#include <sys/types.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <dirent.h>
#include <unistd.h>

int probe(const char *path) {
    struct stat st;
    DIR *dir;
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return 1;
    if (fstat(fd, &st) != 0)
        return 2;
    dir = fdopendir(fd);
    if (dir)
        closedir(dir);
    return S_ISREG(st.st_mode) ? 0 : 3;
}
"#,
        },
        Fixture {
            name: "pthread-dlfcn",
            reason: "QuickJS and hosted plugin probes use pthread and dynamic-loader declarations",
            args: &["-pthread", "-ldl"],
            source: r#"
#include <pthread.h>
#include <dlfcn.h>

static void *worker(void *arg) {
    return arg;
}

int probe(const char *name) {
    pthread_t thread;
    void *handle = dlopen(0, RTLD_NOW | RTLD_LOCAL);
    void *symbol = handle ? dlsym(handle, name) : 0;
    if (pthread_create(&thread, 0, worker, symbol) != 0)
        return 1;
    pthread_join(thread, 0);
    if (handle)
        dlclose(handle);
    return symbol != 0 ? 0 : 2;
}
"#,
        },
    ];

    for fixture in fixtures {
        let input = TempCFile::new(fixture.name, fixture.source);
        let output = input.output();
        let mut command = Command::new(rcc_bin());
        command
            .arg("--target=x86_64-unknown-linux-gnu")
            .arg("--linux-gnu-hosted")
            .arg("--emit=hir")
            .arg("-o")
            .arg(&output);
        command.args(fixture.args);
        command.arg(&input.path);

        let result = command.output().expect("run rcc");
        assert!(
            result.status.success(),
            "fixture `{}` failed (reason: {})\nstdout:\n{}\nstderr:\n{}",
            fixture.name,
            fixture.reason,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(output.exists(), "fixture `{}` did not write HIR output", fixture.name);
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn hosted_linux_header_gate_is_linux_only() {
    // The gate intentionally runs only on Linux CI/WSL because it validates the
    // Linux hosted policy. Windows hosts still compile this test target.
}
