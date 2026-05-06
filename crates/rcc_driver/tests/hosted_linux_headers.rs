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
    off64_t large_offset = 0;
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return 1;
    if (fstat(fd, &st) != 0)
        return 2;
    large_offset = (off64_t) st.st_size;
    dir = fdopendir(fd);
    if (dir)
        closedir(dir);
    return S_ISREG(st.st_mode) && large_offset >= 0 ? 0 : 3;
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
#include <time.h>

static void *worker(void *arg) {
    return arg;
}

int probe(const char *name) {
    pthread_t thread;
    pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
    pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
    struct timespec timeout = { 0, 0 };
    void *handle = dlopen(0, RTLD_NOW | RTLD_LOCAL);
    void *symbol = handle ? dlsym(handle, name) : 0;
    if (pthread_create(&thread, 0, worker, symbol) != 0)
        return 1;
    pthread_join(thread, 0);
    pthread_cond_timedwait(&cond, &mutex, &timeout);
    if (handle)
        dlclose(handle);
    return symbol != 0 ? 0 : 2;
}
"#,
        },
        Fixture {
            name: "stdatomic-quickjs-surface",
            reason: "QuickJS uses stdatomic _Atomic(T) casts and fetch/exchange helpers under CONFIG_ATOMICS",
            args: &["-std=c11", "-fgnu-statement-expressions"],
            source: r#"
#include <stdint.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <time.h>

int probe(uint32_t *p) {
    time_t now = 0;
    struct tm tm;
    void *stack = alloca(16);
    uint32_t old = atomic_fetch_add((_Atomic(uint32_t) *)p, 3);
    uint32_t seen = atomic_exchange((_Atomic(uint32_t) *)p, old);
    uint32_t expected = seen;
    localtime_r(&now, &tm);
    return atomic_compare_exchange_strong((_Atomic(uint32_t) *)p, &expected, 7)
        + (stack != 0)
        + (int)tm.tm_gmtoff;
}
"#,
        },
        Fixture {
            name: "threads-c11-surface",
            reason: "C11 hosted projects may use thread_local plus thrd/mtx/cnd/tss declarations",
            args: &["-std=c11", "-pthread"],
            source: r#"
#include <threads.h>

thread_local int tls_counter;

static int worker(void *arg) {
    tls_counter = arg != 0;
    return tls_counter;
}

int probe(void) {
    thrd_t thread;
    mtx_t mutex;
    cnd_t cond;
    tss_t key;
    once_flag once = ONCE_FLAG_INIT;
    int result = 0;
    mtx_init(&mutex, mtx_plain);
    cnd_init(&cond);
    tss_create(&key, 0);
    call_once(&once, thrd_yield);
    if (thrd_create(&thread, worker, &result) == thrd_success)
        thrd_join(thread, &result);
    cnd_signal(&cond);
    cnd_destroy(&cond);
    mtx_destroy(&mutex);
    tss_delete(key);
    return result;
}
"#,
        },
        Fixture {
            name: "uchar-c11-surface",
            reason: "C11 hosted projects may use Unicode literals plus uchar conversion declarations",
            args: &["-std=c11"],
            source: r#"
#include <uchar.h>

_Static_assert(sizeof(u"x"[0]) == sizeof(char16_t), "char16_t literal width");
_Static_assert(sizeof(U"x"[0]) == sizeof(char32_t), "char32_t literal width");
_Static_assert(sizeof(u8"x"[0]) == 1, "u8 literal width");

int probe(char *buf, const char *src, mbstate_t *st) {
    char16_t c16 = u'x';
    char32_t c32 = U'x';
    size_t n = mbrtoc16(&c16, src, 4, st);
    n = n + c16rtomb(buf, c16, st);
    n = n + mbrtoc32(&c32, src, 4, st);
    n = n + c32rtomb(buf, c32, st);
    return (int)n;
}
"#,
        },
        Fixture {
            name: "c11-library-header-sweep",
            reason: "C11 resource headers should lower together without GNU syntax flags",
            args: &["-std=c11", "-pthread"],
            source: r#"
#include <assert.h>
#include <float.h>
#include <stdalign.h>
#include <stdatomic.h>
#include <stdnoreturn.h>
#include <stdlib.h>
#include <threads.h>
#include <time.h>
#include <uchar.h>

static_assert(FLT_DECIMAL_DIG >= FLT_DIG, "float decimal digits");
static_assert(DBL_DECIMAL_DIG >= DBL_DIG, "double decimal digits");
static_assert(LDBL_DECIMAL_DIG >= LDBL_DIG, "long double decimal digits");

struct c11_probe {
    alignas(16) atomic_int counter;
    atomic_flag flag;
    char16_t c16;
    char32_t c32;
};

noreturn void c11_fatal(int code) {
    thrd_exit(code);
}

int probe(struct c11_probe *p) {
    struct timespec ts = { 0, 0 };
    void *storage = aligned_alloc(16, 16);
    p->flag = (atomic_flag)ATOMIC_FLAG_INIT;
    p->c16 = u'x';
    p->c32 = U'x';
    atomic_store(&p->counter, atomic_load(&p->counter));
    if (storage)
        free(storage);
    return timespec_get(&ts, TIME_UTC) >= 0
        && atomic_is_lock_free(&p->counter)
        && kill_dependency(p->c16) == u'x';
}
"#,
        },
        Fixture {
            name: "coreutils-posix-declaration-sweep",
            reason: "GNU coreutils true probe reaches gnulib wrappers for unlocked stdio, at-functions, errno, and wide-char width",
            args: &[],
            source: r#"
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wchar.h>

int probe(FILE *fp, const char *path, int fd, wchar_t wc, struct stat *st, va_list ap) {
    char *allocated = 0;
    int status = 0;
    status += wcwidth(wc);
    status += fchownat(fd, path, 0, 0, AT_SYMLINK_NOFOLLOW);
    status += fchmodat(AT_FDCWD, path, 0644, 0);
    status += fputs_unlocked(path, fp);
    status += (int) fwrite_unlocked(path, 1, 1, fp);
    status += fflush_unlocked(fp);
    clearerr_unlocked(fp);
    status += fpurge(fp);
    status += vasprintf(&allocated, "%s", ap);
    status += S_TYPEISSHM(st);
    status += S_TYPEISTMO(st);
    return status == EOPNOTSUPP || status == ENOTSUP;
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
