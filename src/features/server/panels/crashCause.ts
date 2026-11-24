/**
 * Why a server crashed, in a word, read from the end of its console: the
 * handful of causes a first-time host meets, each with what to do. Anything
 * else stays "see the console".
 */

import type { ConsoleLine } from "@/types/api";

export type CrashCause = "port" | "memory" | "java" | "dependency" | "agreement" | "world";

const PATTERNS: [CrashCause, RegExp][] = [
  ["port", /FAILED TO BIND TO PORT|Address already in use|BindException/i],
  ["memory", /OutOfMemoryError|Could not reserve enough space|Java heap space/i],
  ["java", /UnsupportedClassVersionError|compiled by a more recent version|requires running the server with Java|Unsupported Java/i],
  ["dependency", /Missing or unsupported mandatory dependencies|requires .* but none is available|Unknown dependency|Could not find required mod|depends on .* which is missing/i],
  ["agreement", /You need to agree to the EULA/i],
  ["world", /Failed to load level|Exception reading .*level\.dat|Corrupted chunk/i],
];

/** The first known cause in the last lines of a run, newest last. */
export function crashCause(lines: ConsoleLine[] | undefined): CrashCause | null {
  if (!lines?.length) return null;
  const tail = lines.slice(-400);
  for (const [cause, pattern] of PATTERNS) {
    if (tail.some((line) => pattern.test(line.message))) return cause;
  }
  return null;
}
