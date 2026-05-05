/*
 * Minimal link support for the GNU coreutils src/true.c oracle.
 *
 * This file is intentionally outside the upstream worktree.  It exists only
 * so the real-world probe can compare the code generated from the selected
 * translation unit (`src/true.c`) without requiring a full libcoreutils host
 * build.  Runtime behavior for libc functions still comes from host glibc.
 */

#define _GNU_SOURCE

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <uchar.h>
#include <wchar.h>

const char Version[] = "rcc-oracle";
const char *program_name = "true";
int exit_failure = 1;

void initialize_main(int *argc, char ***argv) {
    (void)argc;
    (void)argv;
}

void set_program_name(const char *argv0) {
    program_name = argv0 ? argv0 : "true";
}

void close_stdout(void) {}

const char *proper_name_lite(const char *name_ascii, const char *name_utf8) {
    (void)name_utf8;
    return name_ascii;
}

void emit_bug_reporting_address(void) {}

void version_etc(
    FILE *stream,
    const char *command_name,
    const char *package,
    const char *version,
    const char *author,
    ...
) {
    (void)package;
    (void)author;
    fprintf(stream, "%s %s\n", command_name ? command_name : "true", version ? version : "");
}

char *imaxtostr(intmax_t value, char *buf) {
    sprintf(buf, "%jd", value);
    return buf;
}

bool streq(const char *a, const char *b) {
    return strcmp(a, b) == 0;
}

bool memeq(const void *a, const void *b, size_t n) {
    return memcmp(a, b, n) == 0;
}

int c32isblank(char32_t c) {
    return c == ' ' || c == '\t';
}

size_t rpl_mbrtoc32(char32_t *pc32, const char *s, size_t n, mbstate_t *ps) {
    return mbrtoc32(pc32, s, n, ps);
}

void mbszero(mbstate_t *ps) {
    memset(ps, 0, sizeof *ps);
}

int rpl_vasprintf(char **result, const char *format, va_list args) {
    return vasprintf(result, format, args);
}

int fpurge(FILE *stream) {
    return fflush(stream);
}
