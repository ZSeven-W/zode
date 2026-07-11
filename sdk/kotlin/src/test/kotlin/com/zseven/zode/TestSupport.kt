package com.zseven.zode

import java.io.File
import java.nio.file.Files
import kotlinx.serialization.json.Json

/** Json configured exactly like the client's (jsonrpc emitted, nulls omitted). */
internal val testJson =
    Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
        explicitNulls = false
    }

/**
 * Resolves `sdk/fixtures/jsonrpc`. Gradle runs tests with the working directory
 * set to `sdk/kotlin`, so `../fixtures/jsonrpc` is the primary candidate; a few
 * fallbacks make the lookup robust to other runners.
 */
internal fun fixturesDir(): File {
    val candidates =
        listOf(
            File("../fixtures/jsonrpc"),
            File("fixtures/jsonrpc"),
            File("sdk/fixtures/jsonrpc"),
            File("../../sdk/fixtures/jsonrpc"),
        )
    for (c in candidates) {
        if (c.isDirectory) return c.canonicalFile
    }
    error("could not locate sdk/fixtures/jsonrpc from ${File(".").canonicalPath}")
}

/**
 * Writes an executable `/bin/sh` stub into a unique temp directory and returns
 * its path, so it can stand in for the zode binary. Each call gets its own
 * directory so parallel tests never collide.
 */
internal fun scriptedChild(body: String): String {
    val dir = Files.createTempDirectory("zode-child").toFile()
    dir.deleteOnExit()
    val script = File(dir, "zode")
    script.writeText("#!/bin/sh\n$body\n")
    check(script.setExecutable(true)) { "could not chmod +x ${script.path}" }
    script.deleteOnExit()
    return script.absolutePath
}
