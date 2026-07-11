package com.zseven.zode

import java.io.File
import java.nio.file.Files
import java.util.Collections
import org.junit.jupiter.api.Assumptions.assumeTrue
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Opt-in end-to-end test against a real zode binary; runs only when ZODE_BIN is
 * set. The config disables the sandbox and selects the anthropic provider while
 * the child env drops provider API keys, so no live turn can succeed: we only
 * assert the server reaches turn/failed.
 */
class E2eTest {
    @Test
    fun stdioBasicRun() {
        val binary = System.getenv("ZODE_BIN")
        assumeTrue(!binary.isNullOrEmpty(), "ZODE_BIN unset; skipping stdio e2e")

        val configDir = Files.createTempDirectory("zode-e2e").toFile()
        configDir.deleteOnExit()
        val config =
            buildJsonObject {
                putJsonObject("provider") { put("type", "anthropic") }
                putJsonObject("sandbox") { put("enabled", false) }
            }
        File(configDir, "config.json").writeText(config.toString())

        // Isolate config and strip provider keys so no live turn can succeed.
        val env =
            System.getenv().toMutableMap().apply {
                remove("ANTHROPIC_API_KEY")
                remove("OPENAI_API_KEY")
                remove("ZODE_CONFIG_DIR")
                put("ZODE_CONFIG_DIR", configDir.absolutePath)
            }

        val client = ZodeClient(binary = binary!!, env = env)
        try {
            val seen = Collections.synchronizedSet(mutableSetOf<String>())
            client.onNotification { method, _ -> seen.add(method) }

            val init = client.initialize("zode-sdk-kotlin", "0.0.0", "auto")
            assertEquals("auto", init.jsonObject["approvalPolicy"]!!.jsonPrimitive.content)

            val cwd = File(".").canonicalPath
            val started = client.request(ProtocolMethod.ThreadStart, buildJsonObject { put("cwd", cwd) })
            val threadId = started.jsonObject["thread"]!!.jsonObject["id"]!!.jsonPrimitive.content
            assertTrue(threadId.isNotEmpty(), "thread/start returned empty thread id")

            client.request(
                ProtocolMethod.TurnStart,
                buildJsonObject {
                    put("threadId", threadId)
                    put("input", "echo hi")
                },
            )

            val deadline = System.currentTimeMillis() + 15_000
            while (System.currentTimeMillis() < deadline) {
                if (seen.contains("turn/started") && seen.contains("turn/failed")) break
                Thread.sleep(10)
            }
            assertTrue(
                seen.contains("turn/started") && seen.contains("turn/failed"),
                "did not observe turn/started + turn/failed; saw $seen",
            )

            val command =
                client.request(
                    ProtocolMethod.CommandExec,
                    buildJsonObject {
                        putJsonArray("command") {
                            add("sh")
                            add("-c")
                            add("printf hi")
                        }
                    },
                )
            assertEquals("hi", command.jsonObject["stdout"]!!.jsonPrimitive.content)
            assertEquals(0L, command.jsonObject["exitCode"]!!.jsonPrimitive.longOrNull)
        } finally {
            client.close()
        }
    }
}
