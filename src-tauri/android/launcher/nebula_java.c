/*
 * nebula_java: starts the Java runtime that ships inside the app.
 *
 * Android 10+ does not let an app execute files from its own data folder,
 * only the libraries the package installed (the native library folder). The
 * Java runtime is plain files copied to the data folder, so it cannot be run
 * as `bin/java`. This program is the one executable the package does carry:
 * it loads the runtime's launcher library (libjli) with dlopen, which is
 * allowed, and hands it the usual `java` arguments.
 *
 * Usage: spawn this file with argv[0] set to <runtime>/bin/java (or set
 * NEBULA_JRE_DIR); every other argument is a normal `java` argument.
 */

#define _GNU_SOURCE
#include <dlfcn.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef int (*jli_launch_t)(int argc, char **argv, int jargc, const char **jargv, int appclassc,
                            const char **appclassv, const char *fullversion, const char *dotversion,
                            const char *pname, const char *lname, unsigned char javaargs,
                            unsigned char cpwildcard, unsigned char javaw, int ergo);

static void *load(const char *dir, const char *name) {
  char path[PATH_MAX];
  snprintf(path, sizeof path, "%s/%s", dir, name);
  void *handle = dlopen(path, RTLD_NOW | RTLD_GLOBAL);
  if (!handle) fprintf(stderr, "nebula_java: cannot load %s: %s\n", path, dlerror());
  return handle;
}

int main(int argc, char **argv) {
  char home[PATH_MAX];
  const char *forced = getenv("NEBULA_JRE_DIR");
  if (forced && *forced) {
    snprintf(home, sizeof home, "%s", forced);
  } else {
    // argv[0] = <home>/bin/java
    snprintf(home, sizeof home, "%s", argv[0]);
    char *slash = strrchr(home, '/');
    if (slash) *slash = 0;           // .../bin
    slash = strrchr(home, '/');
    if (slash) *slash = 0;           // .../<home>
  }

  char extra[PATH_MAX];
  snprintf(extra, sizeof extra, "%s/../extra", home);

  setenv("JAVA_HOME", home, 1);

  // The runtime needs a few libraries Android does not provide (zlib, iconv,
  // shmem…). They live in <home>/../extra. The dynamic linker only reads
  // LD_LIBRARY_PATH when the process starts, so set it and start over once.
  if (!getenv("NEBULA_JAVA_REEXEC")) {
    const char *old = getenv("LD_LIBRARY_PATH");
    char path[PATH_MAX * 2];
    snprintf(path, sizeof path, "%s%s%s", extra, old && *old ? ":" : "", old ? old : "");
    setenv("LD_LIBRARY_PATH", path, 1);
    setenv("NEBULA_JAVA_REEXEC", "1", 1);
    execv("/proc/self/exe", argv);
    fprintf(stderr, "nebula_java: cannot restart\n");
    return 127;
  }

  char lib[PATH_MAX];
  snprintf(lib, sizeof lib, "%s/lib", home);
  void *jli = load(lib, "libjli.so");
  if (!jli) return 127;
  jli_launch_t launch = (jli_launch_t)dlsym(jli, "JLI_Launch");
  if (!launch) {
    fprintf(stderr, "nebula_java: JLI_Launch not found\n");
    return 127;
  }

  // argv[0] becomes "java" for the runtime's own messages.
  char name[] = "java";
  argv[0] = name;
  return launch(argc, argv, 0, NULL, 0, NULL, "21", "21", "java", "openjdk", 0, 1, 0, 0);
}
